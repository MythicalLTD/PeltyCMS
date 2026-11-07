use crate::common::*;
use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;

// These two tests change the same persisted chat settings and feature flag.
// SuiteGate permits tests within one module to run concurrently; without this
// lock one test can restore live_chat=false between another test's websocket
// upgrade and token validation, correctly revoking that connection mid-handshake.
// Keep all authentication/replay assertions intact and isolate only the shared
// settings lifecycle, rather than retrying a failed connection or delaying it.
static SETTINGS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                return serde_json::from_str(&text).unwrap();
            }
            socket.flush().await.unwrap();
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn live_chat_settings_assignment_and_atomic_conversion() {
    let _settings_guard = SETTINGS_LOCK.lock().await;
    let (app, s) = app().await;
    let admin = sess(&app, &s, "admin").await;
    let customer = sess(&app, &s, "customer").await;
    let original = pelty_database::live_chat::settings(&s.pool).await.unwrap();
    let mut general = pelty_database::general_settings::load(&s.pool)
        .await
        .unwrap();
    let original_enabled = general.support.live_chat;
    general.support.live_chat = true;
    pelty_database::general_settings::save(&s.pool, &general)
        .await
        .unwrap();
    let department = "00000000-0000-0000-0002-000000000001";
    let config = json!({"enabled":true,"departments":[department],"opening_hours":[]});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/chats/settings",
            Some(config.clone()),
            &customer.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/chats/settings",
            Some(config.clone()),
            &[("cookie", &admin.cookie)]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/chats/settings",
            Some(config),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(call(&app, "PUT", "/api/v1/admin/chats/settings", Some(json!({"enabled":true,"departments":[],"opening_hours":[{"day":8,"start_minute":0,"end_minute":1}]})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let email = format!("{}@chat.test", Uuid::new_v4());
    let body = json!({"name":"Visitor","email":email,"message":"Original <script>literal</script>","department_id":department});
    let mut wrong = body.clone();
    wrong["department_id"] = json!("00000000-0000-0000-0002-000000000002");
    assert_eq!(
        call(&app, "POST", "/api/v1/chat/sessions", Some(wrong), &[])
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut hints = s.chat_bus.tx.subscribe();
    let created = call(&app, "POST", "/api/v1/chat/sessions", Some(body), &[]).await;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.body);
    let id = created.body["id"].as_str().unwrap();
    let token = created.body["token"].as_str().unwrap();
    assert!(hints.try_recv().unwrap().invalidates(id.parse().unwrap()));
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1::citext")
        .bind(&admin.email)
        .fetch_one(&s.pool)
        .await
        .unwrap();
    let assign_url = format!("/api/v1/admin/chats/{id}/assignment");
    assert_eq!(
        call(
            &app,
            "PUT",
            &assign_url,
            Some(json!({"assigned_to":Uuid::new_v4()})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &assign_url,
            Some(json!({"assigned_to":uid})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/chats/{id}/messages"),
            Some(json!({"body":"Staff reply"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert!(hints.try_recv().unwrap().invalidates(id.parse().unwrap()));
    let profile = call(&app, "GET", "/api/v1/client/profile", None, &customer.h()).await;
    let cid = profile.body["id"].as_str().unwrap();
    let conversion =
        json!({"customer_id":cid,"department_id":department,"subject":"Chat follow-up"});
    let url = format!("/api/v1/admin/chats/{id}/ticket");
    assert_eq!(
        call(&app, "POST", &url, Some(conversion.clone()), &customer.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let ah = admin.h();
    let (a, b) = tokio::join!(
        call(&app, "POST", &url, Some(conversion.clone()), &ah),
        call(&app, "POST", &url, Some(conversion.clone()), &ah)
    );
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    assert_eq!(a.body, b.body);
    let ticket: Uuid = a.body["ticket_id"].as_str().unwrap().parse().unwrap();
    let messages: Vec<(String, bool)> = sqlx::query_as(
        "SELECT body, is_staff FROM ticket_messages WHERE ticket_id = $1 ORDER BY created_at",
    )
    .bind(ticket)
    .fetch_all(&s.pool)
    .await
    .unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages
        .iter()
        .any(|(b, staff)| !staff && b.ends_with("Original <script>literal</script>")));
    assert!(messages
        .iter()
        .any(|(b, staff)| *staff && b == "Staff reply"));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE target = $1 AND action = 'chat.converted'",
    )
    .bind(id)
    .fetch_one(&s.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/chat/sessions/{id}/messages"),
            Some(json!({"body":"too late"})),
            &[("x-chat-token", token)]
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert!(
        matches!(
            hints.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "failed posts must not publish hints"
    );
    pelty_database::live_chat::save_settings(&s.pool, &original)
        .await
        .unwrap();
    general.support.live_chat = original_enabled;
    pelty_database::general_settings::save(&s.pool, &general)
        .await
        .unwrap();
}

#[tokio::test]
async fn live_chat_redis_message_hints_cross_instances() {
    let Some(url) = std::env::var("TEST_REDIS_URL")
        .ok()
        .filter(|u| !u.is_empty())
    else {
        eprintln!("TEST_REDIS_URL unset; Redis chat hint test skipped");
        return;
    };
    use pelty_api::{chat_bus, shared::Shared};
    use std::sync::Arc;
    let (_, mut receiver) = app().await;
    receiver.shared = Arc::new(Shared::new(Some(url.clone())));
    let mut publisher = receiver.clone();
    publisher.shared = Arc::new(Shared::new(Some(url)));
    publisher.chat_bus = Arc::new(chat_bus::ChatBus::default());
    assert_ne!(publisher.shared.instance, receiver.shared.instance);
    let http = pelty_api::router(publisher.clone());
    let admin = sess(&http, &publisher, "admin").await;
    let (id, token) = pelty_database::live_chat::start(
        &receiver.pool,
        "Redis",
        &format!("{}@chat.test", Uuid::new_v4()),
        "/",
        None,
        "First",
    )
    .await
    .unwrap();
    let mut hints = receiver.chat_bus.tx.subscribe();
    chat_bus::start(&receiver);
    // Wait for actual subscription readiness, not a timing-sensitive fixed sleep.
    let probe = Uuid::new_v4();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            chat_bus::messages_changed(&publisher, probe).await;
            if let Ok(Ok(event)) =
                tokio::time::timeout(Duration::from_millis(100), hints.recv()).await
            {
                if event.invalidates(probe) {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let visitor_url = format!("/api/v1/chat/sessions/{id}/messages");
    let posted = call(
        &http,
        "POST",
        &visitor_url,
        Some(json!({"body":"Not published to Redis"})),
        &[("x-chat-token", token.as_str())],
    )
    .await;
    assert_eq!(posted.status, StatusCode::OK);
    let hint = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = hints.recv().await.unwrap();
            if event.invalidates(id) {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert!(hint.invalidates(id));
    assert!(hint.invalidates(Uuid::nil()));
    assert_eq!(
        serde_json::to_value(hint).unwrap(),
        json!({"kind":"messages_changed","chat_id":id})
    );
    let posted = call(
        &http,
        "POST",
        &format!("/api/v1/admin/chats/{id}/messages"),
        Some(json!({"body":"Staff reply"})),
        &admin.h(),
    )
    .await;
    assert_eq!(posted.status, StatusCode::OK);
    assert!(tokio::time::timeout(Duration::from_secs(3), hints.recv())
        .await
        .unwrap()
        .unwrap()
        .invalidates(id));
    assert_eq!(
        call(
            &http,
            "POST",
            &visitor_url,
            Some(json!({"body":"Rejected"})),
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), hints.recv())
            .await
            .is_err(),
        "failed post must not fan out"
    );
}

#[tokio::test]
async fn live_chat_websocket_origin_auth_and_replay() {
    let _settings_guard = SETTINGS_LOCK.lock().await;
    let (app, s) = app().await;
    let admin = sess(&app, &s, "admin").await;
    let customer = sess(&app, &s, "customer").await;
    let mut general = pelty_database::general_settings::load(&s.pool)
        .await
        .unwrap();
    let enabled = general.support.live_chat;
    general.support.live_chat = true;
    pelty_database::general_settings::save(&s.pool, &general)
        .await
        .unwrap();
    let original = pelty_database::live_chat::settings(&s.pool).await.unwrap();
    pelty_database::live_chat::save_settings(&s.pool, &Default::default())
        .await
        .unwrap();
    let (id, token) = pelty_database::live_chat::start(
        &s.pool,
        "Test",
        &format!("{}@chat.test", Uuid::new_v4()),
        "/",
        None,
        "First",
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let http = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let url = format!("ws://{address}/api/v1/chat/sessions/{id}/ws");
    let mut request = url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    let rejected = connect_async(request).await.unwrap_err();
    assert!(
        matches!(rejected, tokio_tungstenite::tungstenite::Error::Http(ref r) if r.status() == StatusCode::FORBIDDEN)
    );
    let origin = reqwest::Url::parse(&s.cfg.public_url)
        .unwrap()
        .origin()
        .ascii_serialization();
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    let (mut visitor, _) = connect_async(request).await.unwrap();
    visitor
        .send(Message::Text(
            json!({"token":token,"after":0}).to_string().into(),
        ))
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(6), visitor.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let payload: Value = serde_json::from_str(event.to_text().unwrap()).unwrap();
    assert_eq!(payload["messages"][0]["body"], "First");
    let staff_url = format!("ws://{address}/api/v1/admin/chats/{id}/ws");
    let mut request = staff_url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    request
        .headers_mut()
        .insert("cookie", customer.cookie.parse().unwrap());
    assert!(
        matches!(connect_async(request).await.unwrap_err(), tokio_tungstenite::tungstenite::Error::Http(ref r) if r.status() == StatusCode::FORBIDDEN)
    );
    let mut request = staff_url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    request
        .headers_mut()
        .insert("cookie", admin.cookie.parse().unwrap());
    let (mut staff, _) = connect_async(request).await.unwrap();
    staff
        .send(Message::Text(json!({"csrf":"wrong"}).to_string().into()))
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), staff.next())
        .await
        .unwrap();
    assert!(
        !matches!(event, Some(Ok(Message::Text(_)))),
        "wrong CSRF must never receive a transcript"
    );
    let mut request = format!("ws://{address}/api/v1/admin/chats/{id}/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    request
        .headers_mut()
        .insert("cookie", admin.cookie.parse().unwrap());
    let (mut operator, _) = connect_async(request).await.unwrap();
    operator
        .send(Message::Text(
            json!({"csrf":admin.csrf,"after":0}).to_string().into(),
        ))
        .await
        .unwrap();
    let event = next_json(&mut operator).await;
    assert_eq!(event["type"], "sync");
    visitor
        .send(Message::Text(json!({"typing":true}).to_string().into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let event = next_json(&mut operator).await;
            if event["type"] == "signal" && event["typing"] == true {
                break;
            }
        }
    })
    .await
    .unwrap();
    let posted = call(
        &http,
        "POST",
        &format!("/api/v1/chat/sessions/{id}/messages"),
        Some(json!({"body":"Persisted while connected"})),
        &[("x-chat-token", token.as_str())],
    )
    .await;
    assert_eq!(posted.status, StatusCode::OK);
    let mid = posted.body["id"].as_i64().unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let event = next_json(&mut operator).await;
            if event["messages"]
                .as_array()
                .is_some_and(|m| m.iter().any(|v| v["id"] == mid))
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let raw = admin.cookie.strip_prefix("pelty_session=").unwrap();
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(pelty_auth::token::hash(raw))
        .execute(&s.pool)
        .await
        .unwrap();
    pelty_database::live_chat::post(&s.pool, id, "visitor", None, "After revocation")
        .await
        .unwrap();
    pelty_api::chat_bus::messages_changed(&s, id).await;
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            match operator.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(Message::Text(text))) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    assert!(
                        !event["messages"].as_array().is_some_and(|items| items
                            .iter()
                            .any(|m| m["body"] == "After revocation")),
                        "hint delivery must revalidate a revoked session"
                    );
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    visitor.close(None).await.unwrap();
    server.abort();
    pelty_database::live_chat::save_settings(&s.pool, &original)
        .await
        .unwrap();
    general.support.live_chat = enabled;
    pelty_database::general_settings::save(&s.pool, &general)
        .await
        .unwrap();
}
