use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_api::notify::MemoryNotify;
use pelty_extensions::events::DomainEvent;
use serde_json::json;
use std::sync::Arc;

/// Outgoing webhook endpoints: validation, per-endpoint event filter, HMAC
/// signature, delivery log, retry-then-fail, permanent 4xx, redeliver, ping,
/// secret rotation and permission enforcement.
#[tokio::test]
async fn endpoints_filter_sign_retry_redeliver() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();
    let mem = Arc::new(MemoryNotify::default());
    let mut s2 = st.clone();
    s2.notify_transport = mem.clone();
    let tag = uuid::Uuid::new_v4().simple().to_string();

    // Catalog + validation.
    let l = call(&app, "GET", "/api/v1/admin/webhook-endpoints", None, &ah).await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    assert!(l.body["events"].as_array().unwrap().iter().any(|e| e["name"] == "invoice.paid"));
    for bad in [
        json!({"name": "x", "url": "http://insecure.example/x", "events": ["*"]}),
        json!({"name": "x", "url": "https://a.example/x", "events": []}),
        json!({"name": "x", "url": "https://a.example/x", "events": ["no.such_event"]}),
        json!({"name": "", "url": "https://a.example/x", "events": ["*"]}),
    ] {
        let r = call(&app, "POST", "/api/v1/admin/webhook-endpoints", Some(bad), &ah).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    }

    // Two endpoints with different filters.
    let a_url = format!("https://hooks.example/{tag}/a");
    let b_url = format!("https://hooks.example/{tag}/b");
    let a = call(&app, "POST", "/api/v1/admin/webhook-endpoints", Some(json!({"name": "A", "url": a_url, "events": ["invoice.*"]})), &ah).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let b = call(&app, "POST", "/api/v1/admin/webhook-endpoints", Some(json!({"name": "B", "url": b_url, "events": ["ticket.opened"]})), &ah).await;
    assert_eq!(b.status, StatusCode::OK, "{:?}", b.body);
    let secret = a.body["secret"].as_str().expect("secret shown once").to_string();
    assert!(secret.starts_with("whsec_"));
    let aid = a.body["endpoint"]["id"].as_str().unwrap().to_string();
    let bid = b.body["endpoint"]["id"].as_str().unwrap().to_string();
    assert!(a.body["endpoint"].get("secret_sealed").is_none(), "secret must not leak");

    // An invoice event reaches A only.
    pelty_api::webhooks_out::on_event(&st, &DomainEvent::new("invoice.paid", json!({ "invoice_id": tag }))).await;
    let da = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries?endpoint_id={aid}"), None, &ah).await;
    assert_eq!(da.body["items"].as_array().unwrap().len(), 1, "{:?}", da.body);
    let db_ = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries?endpoint_id={bid}"), None, &ah).await;
    assert_eq!(db_.body["items"].as_array().unwrap().len(), 0);
    let d1 = da.body["items"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(da.body["items"][0]["status"], "pending");
    drain_jobs(&s2, pelty_api::webhooks_out::JOB_KIND, &d1).await;

    let sent = mem.sent.lock().unwrap().clone();
    let heads = mem.headers.lock().unwrap().clone();
    let raws = mem.raw.lock().unwrap().clone();
    let i = sent.iter().position(|(u, _)| u == &a_url).expect("delivered to A");
    assert_eq!(sent[i].1["event"], "invoice.paid");
    assert_eq!(sent[i].1["data"]["invoice_id"], json!(tag));
    let get = |k: &str| heads[i].iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
    let ts: i64 = get("x-pelty-timestamp").parse().unwrap();
    assert_eq!(get("x-pelty-signature"), pelty_api::plugin_hooks::signature(&secret, ts, &raws[i]));
    assert_eq!(get("x-pelty-event"), "invoice.paid");
    assert_eq!(get("x-pelty-delivery"), d1);
    let one = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries/{d1}"), None, &ah).await;
    assert_eq!(one.body["delivery"]["status"], "delivered", "{:?}", one.body);
    assert_eq!(one.body["delivery"]["response_status"], 200);
    assert_eq!(one.body["delivery"]["payload"]["invoice_id"], json!(tag));

    // Redeliver creates a linked delivery.
    let r = call(&app, "POST", &format!("/api/v1/admin/webhook-deliveries/{d1}/redeliver"), None, &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d2 = r.body["delivery_id"].as_str().unwrap().to_string();
    drain_jobs(&s2, pelty_api::webhooks_out::JOB_KIND, &d2).await;
    let two = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries/{d2}"), None, &ah).await;
    assert_eq!(two.body["delivery"]["status"], "delivered");
    assert_eq!(two.body["delivery"]["redelivery_of"], json!(d1));

    // 500 retries with backoff until attempts run out, then fails.
    let flaky = Arc::new(MemoryNotify { status: 500, ..Default::default() });
    s2.notify_transport = flaky.clone();
    let t = call(&app, "POST", &format!("/api/v1/admin/webhook-endpoints/{aid}/test"), None, &ah).await;
    assert_eq!(t.status, StatusCode::OK, "{:?}", t.body);
    let d3 = t.body["delivery_id"].as_str().unwrap().to_string();
    let ran = drain_jobs(&s2, pelty_api::webhooks_out::JOB_KIND, &d3).await;
    assert_eq!(ran as i32, pelty_api::webhooks_out::MAX_ATTEMPTS);
    let three = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries/{d3}"), None, &ah).await;
    assert_eq!(three.body["delivery"]["status"], "failed", "{:?}", three.body);
    assert_eq!(three.body["delivery"]["event"], "webhook.ping");
    assert_eq!(three.body["delivery"]["attempts"], pelty_api::webhooks_out::MAX_ATTEMPTS);
    assert_eq!(three.body["delivery"]["last_error"], "HTTP 500");

    // 410 fails at once.
    let gone = Arc::new(MemoryNotify { status: 410, ..Default::default() });
    s2.notify_transport = gone.clone();
    let t = call(&app, "POST", &format!("/api/v1/admin/webhook-endpoints/{aid}/test"), None, &ah).await;
    let d4 = t.body["delivery_id"].as_str().unwrap().to_string();
    assert_eq!(drain_jobs(&s2, pelty_api::webhooks_out::JOB_KIND, &d4).await, 1);
    let failed = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries?endpoint_id={aid}&status=failed"), None, &ah).await;
    assert_eq!(failed.body["items"].as_array().unwrap().len(), 2, "{:?}", failed.body);

    // Disabled endpoints get no events; rotate gives a new secret.
    let u = call(&app, "PUT", &format!("/api/v1/admin/webhook-endpoints/{bid}"), Some(json!({"name": "B", "url": b_url, "events": ["ticket.opened"], "enabled": false})), &ah).await;
    assert_eq!(u.status, StatusCode::OK, "{:?}", u.body);
    assert_eq!(u.body["endpoint"]["enabled"], false);
    pelty_api::webhooks_out::on_event(&st, &DomainEvent::new("ticket.opened", json!({ "t": tag }))).await;
    let db_ = call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries?endpoint_id={bid}"), None, &ah).await;
    assert_eq!(db_.body["items"].as_array().unwrap().len(), 0);
    let rot = call(&app, "POST", &format!("/api/v1/admin/webhook-endpoints/{aid}/secret"), None, &ah).await;
    assert_eq!(rot.status, StatusCode::OK);
    assert_ne!(rot.body["secret"], json!(secret));

    // Staff without webhooks.manage are refused.
    let name = format!("nowh_{}", &tag[..8]);
    let r = call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": name, "permissions": ["admin.access", "customers.read"]})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let staff = sess(&app, &st, &name).await;
    assert_eq!(call(&app, "GET", "/api/v1/admin/webhook-endpoints", None, &staff.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/webhook-deliveries", None, &staff.h()).await.status, StatusCode::FORBIDDEN);

    // Audit trail + cleanup (deleting cascades deliveries).
    let au = call(&app, "GET", &format!("/api/v1/admin/audit-logs?action=webhook_endpoint.created&target={aid}"), None, &ah).await;
    assert!(au.body.to_string().contains(&aid), "{:?}", au.body);
    for id in [&aid, &bid] {
        assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/webhook-endpoints/{id}"), None, &ah).await.status, StatusCode::NO_CONTENT);
    }
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/webhook-deliveries/{d1}"), None, &ah).await.status, StatusCode::NOT_FOUND);
}
