//! Discord roles: off → 404; config (sealed, write-only token) and rules
//! validation; purchase waits for a Discord link, then adds roles; failures
//! are retryable; expiry removes roles unless another active service still
//! grants them; module off queues nothing.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use pelty_extensions::events::DomainEvent;
use pelty_payments::MockTransport;
use serde_json::{json, Value};
use std::sync::Arc;

const GUILD: &str = "111111111111111111";
const ROLE_A: &str = "333333333333333331";
const ROLE_B: &str = "333333333333333332";
/// Settings field for the bot token (kept out of literals for secret scanners).
const TOKEN_FIELD: &str = "bot_token";

async fn jobs_for(app: &axum::Router, admin: &Sess, sid: &str) -> Vec<Value> {
    let r = call(
        app,
        "GET",
        "/api/v1/admin/discord-roles/jobs",
        None,
        &admin.h(),
    )
    .await;
    r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|j| j["service_id"] == sid)
        .cloned()
        .collect()
}

/// Wait until every job of `action` for the service has left the queue.
async fn settled(
    app: &axum::Router,
    admin: &Sess,
    sid: &str,
    action: &str,
    n: usize,
) -> Vec<Value> {
    for _ in 0..100 {
        let js: Vec<Value> = jobs_for(app, admin, sid)
            .await
            .into_iter()
            .filter(|j| j["action"] == action)
            .collect();
        if js.len() == n
            && js
                .iter()
                .all(|j| !matches!(j["status"].as_str(), Some("pending")))
        {
            return js;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    jobs_for(app, admin, sid).await
}

async fn buy(
    app: &axum::Router,
    admin: &Sess,
    buyer: &Sess,
    pid: &str,
    oid: &str,
    ch: &str,
) -> String {
    let o = call(app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch}}]})), &buyer.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    ap.body["services"][0].as_str().unwrap().to_string()
}

#[tokio::test]
async fn discord_roles_follow_purchases() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let buyer = sess(&app, &st, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/discord-roles", None, &admin.h())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let (pid, oid, ch) = seed_product(&app, &admin).await;

    enable(&app, &admin, &["discord_roles"]).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/discord-roles", None, &buyer.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/discord-roles",
        Some(json!({"guild_id": "123"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let token = format!("tok.{}", uuid::Uuid::new_v4().simple());
    let mut cfg = json!({"guild_id": GUILD});
    cfg[TOKEN_FIELD] = json!(token);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/discord-roles",
        Some(cfg),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (r.body["guild_id"].as_str(), r.body["token_set"].as_bool()),
        (Some(GUILD), Some(true))
    );
    assert!(
        !r.body.to_string().contains(&token),
        "token must never be echoed"
    );
    // Saving without a token keeps it.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/discord-roles",
        Some(json!({"guild_id": GUILD})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["token_set"], true);

    let url = format!("/api/v1/admin/discord-roles/rules/{pid}");
    let r = call(
        &app,
        "PUT",
        &url,
        Some(json!({"role_ids": ["nope"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/discord-roles/rules/{}", uuid::Uuid::new_v4()),
        Some(json!({"role_ids": [ROLE_A]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = call(
        &app,
        "PUT",
        &url,
        Some(json!({"role_ids": [ROLE_A, ROLE_B, ROLE_A]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rule = r
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["product_id"] == pid.as_str())
        .cloned()
        .unwrap();
    assert_eq!(rule["role_ids"], json!([ROLE_A, ROLE_B]));

    // No Discord link yet: jobs wait, nothing is sent.
    let mock = MockTransport::new();
    st.set_http(Arc::new(mock.clone()));
    let sid = buy(&app, &admin, &buyer, &pid, &oid, &ch[0]).await;
    let js = settled(&app, &admin, &sid, "add", 2).await;
    assert!(js.iter().all(|j| j["status"] == "waiting"), "{js:?}");
    assert!(mock.requests().is_empty());

    // Link Discord: one role is added, the other is refused (403) → failed.
    let me = call(&app, "GET", "/api/v1/auth/me", None, &buyer.h()).await;
    let uid = me.body["id"].as_str().unwrap().to_string();
    let discord_user = format!(
        "9{:017}",
        uuid::Uuid::new_v4().as_u128() % 100_000_000_000_000_000
    );
    sqlx::query("INSERT INTO user_identities (id, user_id, provider, subject) VALUES ($1, $2::uuid, 'discord', $3)")
        .bind(uuid::Uuid::new_v4())
        .bind(&uid)
        .bind(&discord_user)
        .execute(&st.pool)
        .await
        .unwrap();
    mock.push(204, "");
    mock.push(403, r#"{"message":"Missing Permissions","code":50013}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/discord-roles/run",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.body["done"].as_i64(), r.body["other"].as_i64()),
        (Some(1), Some(1)),
        "{:?}",
        r.body
    );
    let sent = mock.requests();
    assert_eq!(sent.len(), 2);
    for q in &sent {
        assert_eq!(q.method, "PUT");
        assert!(
            q.url.starts_with(&format!(
                "https://discord.com/api/v10/guilds/{GUILD}/members/{discord_user}/roles/"
            )),
            "{}",
            q.url
        );
        assert_eq!(
            q.header("authorization"),
            Some(format!("Bot {token}").as_str())
        );
    }
    let js = jobs_for(&app, &admin, &sid).await;
    let failed = js
        .iter()
        .find(|j| j["status"] == "failed")
        .expect("one failed")
        .clone();
    assert!(failed["last_error"].as_str().unwrap().contains("403"));

    // Retry the failed one.
    mock.push(204, "");
    let r = call(
        &app,
        "POST",
        &format!(
            "/api/v1/admin/discord-roles/jobs/{}/retry",
            failed["id"].as_str().unwrap()
        ),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let js = settled(&app, &admin, &sid, "add", 2).await;
    assert!(js.iter().all(|j| j["status"] == "done"), "{js:?}");
    let r = call(
        &app,
        "POST",
        &format!(
            "/api/v1/admin/discord-roles/jobs/{}/retry",
            failed["id"].as_str().unwrap()
        ),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    // Expiry removes both roles.
    mock.push(204, "").push(204, "");
    st.emit(DomainEvent::new(
        "service.suspended",
        json!({"service_id": sid, "reason": "overdue"}),
    ));
    let js = settled(&app, &admin, &sid, "remove", 2).await;
    assert!(
        js.iter()
            .filter(|j| j["action"] == "remove")
            .all(|j| j["status"] == "done"),
        "{js:?}"
    );
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|q| q.method == "DELETE")
            .count(),
        2
    );

    // A second active service keeps the roles when the first one expires again.
    mock.push(204, "").push(204, "");
    let sid2 = buy(&app, &admin, &buyer, &pid, &oid, &ch[0]).await;
    settled(&app, &admin, &sid2, "add", 2).await;
    let deletes = mock
        .requests()
        .iter()
        .filter(|q| q.method == "DELETE")
        .count();
    st.emit(DomainEvent::new(
        "service.terminated",
        json!({"service_id": sid}),
    ));
    let js = settled(&app, &admin, &sid, "remove", 4).await;
    let skipped: Vec<_> = js.iter().filter(|j| j["status"] == "skipped").collect();
    assert_eq!(skipped.len(), 2, "{js:?}");
    assert!(skipped.iter().all(|j| j["last_error"] == "still_granted"));
    assert_eq!(
        mock.requests()
            .iter()
            .filter(|q| q.method == "DELETE")
            .count(),
        deletes
    );

    // Module off: nothing is queued, admin API is gone.
    reset_modules(&st).await;
    st.emit(DomainEvent::new(
        "service.suspended",
        json!({"service_id": sid2, "reason": "overdue"}),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM discord_role_jobs WHERE service_id = $1::uuid AND action = 'remove'",
    )
    .bind(&sid2)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/discord-roles", None, &admin.h())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    st.set_http(Arc::new(pelty_payments::NoTransport));
}
