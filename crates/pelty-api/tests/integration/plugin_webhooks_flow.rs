//! External plugin webhooks: durable signed delivery, retry after a 5xx,
//! delivery log + redeliver API, URL override validation and permissions.
use crate::common;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use common::*;
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Sink {
    hits: Arc<Mutex<Vec<(HeaderMap, String)>>>,
    /// Answer 500 to the first N requests.
    fail_first: Arc<Mutex<usize>>,
}

async fn hook(State(k): State<Sink>, h: HeaderMap, b: Bytes) -> StatusCode {
    k.hits.lock().unwrap().push((h, String::from_utf8_lossy(&b).into_owned()));
    let mut f = k.fail_first.lock().unwrap();
    if *f > 0 {
        *f -= 1;
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::NO_CONTENT
    }
}

async fn status_of(st: &pelty_api::AppState, marker: &str) -> Option<(String, i32)> {
    sqlx::query_as::<_, (String, i32)>("SELECT status, attempts FROM plugin_deliveries WHERE payload->>'marker' = $1").bind(marker).fetch_optional(&st.pool).await.unwrap()
}

async fn pump(st: &pelty_api::AppState, marker: &str, want: &str) -> (String, i32) {
    for _ in 0..100 {
        // Skip retry backoff in tests.
        sqlx::query("UPDATE jobs SET run_at = now() WHERE kind = 'plugin.webhook' AND status = 'queued'").execute(&st.pool).await.ok();
        pelty_api::worker::run_once(st, "hooks-test", 20).await.unwrap();
        if let Some(s) = status_of(st, marker).await {
            if s.0 == want {
                return s;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    status_of(st, marker).await.unwrap_or_default()
}

#[tokio::test]
async fn plugin_webhooks_are_signed_retried_and_logged() {
    let sink = Sink::default();
    *sink.fail_first.lock().unwrap() = 1;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", l.local_addr().unwrap());
    let r = axum::Router::new().route("/hook", axum::routing::post(hook)).with_state(sink.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });

    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let base = "/api/v1/admin/plugins/example-webhook";
    assert_eq!(call(&app, "GET", &format!("{base}/deliveries"), None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/plugins/event-logger/webhook", None, &admin.h()).await.status, StatusCode::NOT_FOUND, "built-ins have no webhook");

    let bad = call(&app, "PUT", &format!("{base}/webhook"), Some(json!({"url": "http://example.com/x"})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", &format!("{base}/webhook"), Some(json!({"url": url})), &admin.h()).await.status, StatusCode::NO_CONTENT);
    let rot = call(&app, "POST", &format!("{base}/webhook/secret"), None, &admin.h()).await;
    assert_eq!(rot.status, StatusCode::OK);
    let secret = rot.body["secret"].as_str().unwrap().to_string();
    assert_eq!(secret.len(), 64);
    let info = call(&app, "GET", &format!("{base}/webhook"), None, &admin.h()).await;
    assert_eq!(info.body["secret_set"], true);
    assert!(!info.body.to_string().contains(&secret), "secret never listed");

    // Disabled plugin: nothing queued.
    let m0 = uuid::Uuid::new_v4().to_string();
    st.plugins.set_enabled("example-webhook", false);
    st.emit(DomainEvent::new("invoice.paid", json!({"marker": m0})));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(status_of(&st, &m0).await.is_none());

    st.plugins.set_enabled("example-webhook", true);
    // Not subscribed (manifest hooks: invoice.paid, service.created).
    let m1 = uuid::Uuid::new_v4().to_string();
    st.emit(DomainEvent::new("ticket.created", json!({"marker": m1})));
    let marker = uuid::Uuid::new_v4().to_string();
    st.emit(DomainEvent::new("invoice.paid", json!({"marker": marker, "invoice_id": "x"})));
    let (status, attempts) = pump(&st, &marker, "delivered").await;
    assert_eq!((status.as_str(), attempts), ("delivered", 2), "first 500 retried, then delivered");
    assert!(status_of(&st, &m1).await.is_none());

    let hits = sink.hits.lock().unwrap().clone();
    let mine: Vec<_> = hits.iter().filter(|(_, b)| b.contains(&marker)).collect();
    assert_eq!(mine.len(), 2);
    for (h, body) in &mine {
        let g = |k: &str| h.get(k).unwrap().to_str().unwrap().to_string();
        let ts: i64 = g("x-pelty-timestamp").parse().unwrap();
        assert_eq!(g("x-pelty-signature"), pelty_api::plugin_hooks::signature(&secret, ts, body));
        assert_eq!(g("x-pelty-event"), "invoice.paid");
        let v: Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["data"]["marker"], marker.as_str());
        assert_eq!(g("x-pelty-delivery"), v["id"].as_str().unwrap());
    }

    // Log API shows it; redeliver sends again.
    let log = call(&app, "GET", &format!("{base}/deliveries?status=delivered&per_page=100"), None, &admin.h()).await;
    assert_eq!(log.status, StatusCode::OK);
    let row = log.body["items"].as_array().unwrap().iter().find(|d| d["payload"]["marker"] == marker.as_str()).cloned().expect("listed");
    assert_eq!(row["response_status"], 204);
    assert_eq!(call(&app, "GET", &format!("{base}/deliveries?status=nope"), None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let did = row["id"].as_str().unwrap();
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/plugins/event-logger/deliveries/{did}/redeliver"), None, &admin.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "POST", &format!("{base}/deliveries/{did}/redeliver"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let (status, attempts) = pump(&st, &marker, "delivered").await;
    assert_eq!((status.as_str(), attempts), ("delivered", 3));
    assert_eq!(sink.hits.lock().unwrap().iter().filter(|(_, b)| b.contains(&marker)).count(), 3);
    st.plugins.set_enabled("example-webhook", false);
}
