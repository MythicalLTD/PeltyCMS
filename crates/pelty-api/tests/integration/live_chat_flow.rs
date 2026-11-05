use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_database::general_settings as gs;
use serde_json::json;

#[tokio::test]
async fn live_chat_visitor_and_staff() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let mut g = gs::load(&st.pool).await.unwrap();
    g.support.live_chat = false;
    gs::save(&st.pool, &g).await.unwrap();
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let email = format!("v{tag}@chat.test");
    let start = json!({"name": "Vera", "email": email, "message": "Hi, is DDoS protection included?", "page": "/store"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/chat/sessions",
            Some(start.clone()),
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "off by default"
    );
    g.support.live_chat = true;
    gs::save(&st.pool, &g).await.unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/public/general", None, &[])
            .await
            .body["support"]["live_chat"],
        true
    );

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/chat/sessions",
            Some(json!({"name": "", "email": email, "message": "x"})),
            &[]
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(&app, "POST", "/api/v1/chat/sessions", Some(start), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    let tok = r.body["token"].as_str().unwrap().to_string();
    let vh = [("x-chat-token", tok.as_str())];
    let bad = [("x-chat-token", "0".repeat(64))];
    let bad_h: Vec<(&str, &str)> = bad.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let murl = format!("/api/v1/chat/sessions/{id}/messages");
    assert_eq!(
        call(&app, "GET", &murl, None, &bad_h).await.status,
        StatusCode::NOT_FOUND,
        "wrong token"
    );
    assert_eq!(
        call(&app, "GET", &murl, None, &[]).await.status,
        StatusCode::NOT_FOUND,
        "no token"
    );

    // Staff inbox shows it unread; customers can't see the inbox.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/chats", None, &c.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let inbox = call(
        &app,
        "GET",
        "/api/v1/admin/chats?status=open",
        None,
        &admin.h(),
    )
    .await;
    let row = inbox.body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        (
            row["staff_unread"].as_i64(),
            row["last_body"].as_str(),
            row["page"].as_str()
        ),
        (
            Some(1),
            Some("Hi, is DDoS protection included?"),
            Some("/store")
        )
    );
    assert!(inbox.body["unread"].as_i64().unwrap() >= 1);

    // Staff reads (clears unread) and replies; visitor polls after the last id.
    let sm = call(
        &app,
        "GET",
        &format!("/api/v1/admin/chats/{id}/messages"),
        None,
        &admin.h(),
    )
    .await;
    let first = sm.body["messages"][0]["id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/chats/{id}/messages"),
        Some(json!({"body": "Yes, on every plan."})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inbox = call(&app, "GET", "/api/v1/admin/chats", None, &admin.h()).await;
    let row = inbox.body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["staff_unread"], 0);
    assert!(!row["assigned_to"].is_null(), "first reply assigns");
    let v = call(&app, "GET", &format!("{murl}?after={first}"), None, &vh).await;
    assert_eq!(v.status, StatusCode::OK);
    let msgs = v.body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(
        (msgs[0]["author"].as_str(), msgs[0]["body"].as_str()),
        (Some("staff"), Some("Yes, on every plan."))
    );
    let last = msgs[0]["id"].as_i64().unwrap();
    assert!(
        call(&app, "GET", &format!("{murl}?after={last}"), None, &vh)
            .await
            .body["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Visitor replies; too-long messages refused.
    assert_eq!(
        call(
            &app,
            "POST",
            &murl,
            Some(json!({"body": "x".repeat(2001)})),
            &vh
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "POST", &murl, Some(json!({"body": "Thanks!"})), &vh)
            .await
            .status,
        StatusCode::OK
    );

    // Staff suggestion cards: internal paths only.
    let bad_action = call(
        &app,
        "POST",
        &format!("/api/v1/admin/chats/{id}/actions"),
        Some(json!({"kind":"open_page","href":"https://evil.test","label":"Nope"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        bad_action.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        bad_action.body
    );
    let bad_slash = call(
        &app,
        "POST",
        &format!("/api/v1/admin/chats/{id}/actions"),
        Some(json!({"kind":"open_page","href":"//evil.test","label":"Nope"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad_slash.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok_action = call(
        &app,
        "POST",
        &format!("/api/v1/admin/chats/{id}/actions"),
        Some(json!({"kind":"open_page","href":"/store","label":"Browse the store"})),
        &admin.h(),
    )
    .await;
    assert_eq!(ok_action.status, StatusCode::OK, "{:?}", ok_action.body);
    let after_action = call(&app, "GET", &format!("{murl}?after={last}"), None, &vh).await;
    let action_msg = after_action.body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["author"] == "system")
        .unwrap();
    assert!(action_msg["body"]
        .as_str()
        .unwrap()
        .contains("\"kind\":\"open_page\""));
    assert!(action_msg["body"].as_str().unwrap().contains("/store"));

    // Closing ends the conversation for both sides.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/chats/{id}/close"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", &murl, Some(json!({"body": "Hello?"})), &vh)
            .await
            .status,
        StatusCode::CONFLICT
    );
    let v = call(&app, "GET", &format!("{murl}?after={last}"), None, &vh).await;
    assert_eq!(v.body["status"], "closed");
    assert_eq!(
        v.body["messages"].as_array().unwrap().last().unwrap()["body"],
        "closed_by_staff"
    );

    // Max 3 open sessions per email.
    for i in 0..3 {
        let r = call(
            &app,
            "POST",
            "/api/v1/chat/sessions",
            Some(json!({"name": "V", "email": email, "message": format!("m{i}")})),
            &[],
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{i} {:?}", r.body);
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/chat/sessions",
            Some(json!({"name": "V", "email": email, "message": "m"})),
            &[]
        )
        .await
        .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    g.support.live_chat = false;
    gs::save(&st.pool, &g).await.unwrap();
}
