use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_api::notify::MemoryNotify;
use serde_json::json;
use std::sync::Arc;

async fn drain(s: &pelty_api::AppState, did: &str) {
    drain_jobs(s, "notify.deliver", did).await;
}

#[tokio::test]
async fn telegram_sms_and_signed_webhook_with_log() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let mem = Arc::new(MemoryNotify::default());
    let mut s2 = st.clone();
    s2.notify_transport = mem.clone();
    let tag = uuid::Uuid::new_v4().simple().to_string();

    // Validation: telegram must target api.telegram.org with a chat id.
    let bad = call(&app, "POST", "/api/v1/admin/notification-rules", Some(json!({"name": "t", "provider": "telegram", "url": "https://evil.example/bot1", "target": "1", "events": ["invoice.paid"]})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let tg = call(&app, "POST", "/api/v1/admin/notification-rules", Some(json!({"name": "tg", "provider": "telegram", "url": format!("https://api.telegram.org/bot1:{tag}/sendMessage"), "target": "-100200", "events": ["invoice.paid"]})), &admin.h()).await;
    assert_eq!(tg.status, StatusCode::OK, "{:?}", tg.body);
    let sms = call(&app, "POST", "/api/v1/admin/notification-rules", Some(json!({"name": "sms", "provider": "sms_webhook", "url": format!("https://sms.example/{tag}"), "target": "+40712345678", "events": ["invoice.paid"]})), &admin.h()).await;
    assert_eq!(sms.status, StatusCode::OK, "{:?}", sms.body);
    let sme = format!("0712{}@sms.example.com", &tag[..6]);
    let sme_r = call(&app, "POST", "/api/v1/admin/notification-rules", Some(json!({"name": "sme", "provider": "sms_email", "target": sme, "events": ["invoice.paid"]})), &admin.h()).await;
    assert_eq!(sme_r.status, StatusCode::OK, "{:?}", sme_r.body);
    let wh = call(&app, "POST", "/api/v1/admin/notification-rules", Some(json!({"name": "wh", "provider": "webhook", "url": format!("https://hooks.example/{tag}"), "events": ["invoice.paid"]})), &admin.h()).await;
    assert_eq!(wh.status, StatusCode::OK, "{:?}", wh.body);
    let secret = wh.body["secret"]
        .as_str()
        .expect("secret shown once")
        .to_string();
    let wid = wh.body["id"].as_str().unwrap().to_string();

    // Use "test" deliveries so parallel tests' invoice.paid events don't interfere.
    let mut dids = vec![];
    for r in [&tg, &sms, &sme_r, &wh] {
        let t = call(
            &app,
            "POST",
            &format!(
                "/api/v1/admin/notification-rules/{}/test",
                r.body["id"].as_str().unwrap()
            ),
            None,
            &admin.h(),
        )
        .await;
        assert_eq!(t.status, StatusCode::OK, "{:?}", t.body);
        dids.push(t.body["delivery_id"].as_str().unwrap().to_string());
    }
    for d in &dids {
        drain(&s2, d).await;
    }

    let sent = mem.sent.lock().unwrap().clone();
    let heads = mem.headers.lock().unwrap().clone();
    let raws = mem.raw.lock().unwrap().clone();
    let tgm = sent
        .iter()
        .find(|(u, _)| u.contains(&tag) && u.contains("telegram"))
        .expect("telegram sent");
    assert_eq!(tgm.1["chat_id"], "-100200");
    assert!(tgm.1["text"].as_str().unwrap().starts_with("test: "));
    let smm = sent
        .iter()
        .find(|(u, _)| u == &format!("https://sms.example/{tag}"))
        .expect("sms sent");
    assert_eq!(smm.1["to"], "+40712345678");
    let i = sent
        .iter()
        .position(|(u, _)| u == &format!("https://hooks.example/{tag}"))
        .expect("webhook sent");
    let h = &heads[i];
    let get = |k: &str| {
        h.iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let ts: i64 = get("x-pelty-timestamp").parse().unwrap();
    assert_eq!(
        get("x-pelty-signature"),
        pelty_api::plugin_hooks::signature(&secret, ts, &raws[i])
    );
    assert_eq!(get("x-pelty-event"), "test");
    // Email-to-SMS went through the mailer.
    assert_eq!(mails_to(&st, &sme).await.len(), 1);

    // Delivery log shows delivered.
    let log = call(
        &app,
        "GET",
        &format!("/api/v1/admin/notification-rules/{wid}/deliveries"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        log.body["items"][0]["status"], "delivered",
        "{:?}",
        log.body
    );
    assert_eq!(log.body["items"][0]["response_status"], 200);

    // 500 retries (pending), 404 fails permanently and is logged.
    let bad = Arc::new(MemoryNotify {
        status: 410,
        ..Default::default()
    });
    s2.notify_transport = bad.clone();
    let t = call(
        &app,
        "POST",
        &format!("/api/v1/admin/notification-rules/{wid}/test"),
        None,
        &admin.h(),
    )
    .await;
    let d = t.body["delivery_id"].as_str().unwrap().to_string();
    drain(&s2, &d).await;
    let log = call(
        &app,
        "GET",
        &format!("/api/v1/admin/notification-rules/{wid}/deliveries"),
        None,
        &admin.h(),
    )
    .await;
    let row = log.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == json!(d))
        .unwrap()
        .clone();
    assert_eq!(row["status"], "failed");
    assert_eq!(row["last_error"], "HTTP 410");

    // Rotation changes the secret.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/notification-rules/{wid}/secret"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_ne!(r.body["secret"].as_str().unwrap(), secret);
    let list = call(
        &app,
        "GET",
        "/api/v1/admin/notification-rules",
        None,
        &admin.h(),
    )
    .await;
    let it = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == json!(wid))
        .unwrap()
        .clone();
    assert_eq!(it["signed"], true);
    assert!(it["stats"]["delivered"].as_i64().unwrap() >= 1);
}
