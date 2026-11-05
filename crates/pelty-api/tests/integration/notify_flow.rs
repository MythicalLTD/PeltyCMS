use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_api::notify::MemoryNotify;
use pelty_extensions::events::DomainEvent;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn rules_fan_out_events_to_mock_transport() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let tag = uuid::Uuid::new_v4();
    let slack = format!("https://hooks.slack.example/{tag}");
    let teams = format!("https://teams.example/{tag}");
    for (p, u, ev) in [
        ("slack", &slack, "invoice.paid"),
        ("teams", &teams, "order.created"),
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/notification-rules",
            Some(json!({"name": p, "provider": p, "url": u, "events": [ev]})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    let mem = Arc::new(MemoryNotify::default());
    let mut s2 = st.clone();
    s2.notify_transport = mem.clone();
    pelty_api::notify::on_event(
        &s2,
        &DomainEvent::new("invoice.paid", json!({"invoice_id": "INV-1"})),
    )
    .await;
    drain_jobs(&s2, "notify.deliver", &tag.to_string()).await;
    let sent: Vec<_> = mem
        .sent
        .lock()
        .unwrap()
        .iter()
        .filter(|(u, _)| u.contains(&tag.to_string()))
        .cloned()
        .collect();
    assert_eq!(sent.len(), 1, "only the invoice.paid rule fires");
    assert_eq!(sent[0].0, slack);
    assert_eq!(sent[0].1["text"], "*invoice.paid*: invoice_id=INV-1");

    // A 4xx answer is permanent (dead, no retries).
    let bad = Arc::new(MemoryNotify {
        status: 404,
        ..Default::default()
    });
    s2.notify_transport = bad.clone();
    pelty_api::notify::on_event(
        &s2,
        &DomainEvent::new("order.created", json!({"order_id": "O-9"})),
    )
    .await;
    drain_jobs(&s2, "notify.deliver", &tag.to_string()).await;
    let dead: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind='notify.deliver' AND status='dead' AND payload->>'url' = $1").bind(&teams).fetch_one(&st.pool).await.unwrap();
    assert_eq!(dead, 1);
    assert_eq!(
        bad.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(u, b)| u == &teams && b["title"] == "order.created")
            .count(),
        1
    );
}
