//! Provisioning jobs triggered indirectly: overdue-style lifecycle events
//! on the bus (no request handler) and applied plan changes.
use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_extensions::events::DomainEvent;
use serde_json::json;

async fn jobs(st: &pelty_api::AppState, sid: &str, action: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs WHERE kind = 'provision' AND payload->>'service_id' = $1 AND payload->>'action' = $2")
        .bind(sid).bind(action).fetch_one(&st.pool).await.unwrap()
}

async fn wait(st: &pelty_api::AppState, sid: &str, action: &str, n: i64) -> i64 {
    for _ in 0..40 {
        let c = jobs(st, sid, action).await;
        if c >= n {
            return c;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    jobs(st, sid, action).await
}

#[tokio::test]
async fn bus_events_and_plan_changes_enqueue_provisioning() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let group: String =
        sqlx::query_scalar("SELECT group_id::text FROM products WHERE id = $1::uuid")
            .bind(&pid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    let slug = format!("p{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let big = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": group, "name": "VPS L", "slug": slug})),
        &admin.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{big}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "monthly", "amount": "20.00"}]})),
        &admin.h(),
    )
    .await;
    sqlx::query("UPDATE products SET module = 'pterodactyl' WHERE id IN ($1::uuid, $2::uuid)")
        .bind(&pid)
        .bind(&big)
        .execute(&st.pool)
        .await
        .unwrap();

    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    let order_id = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let sid = ap.body["services"][0].as_str().unwrap().to_string();
    assert_eq!(wait(&st, &sid, "create", 1).await, 1);

    // Overdue automation publishes events only → the bus enqueues jobs.
    sqlx::query(
        "UPDATE services SET status = 'suspended', suspend_reason = 'overdue' WHERE id = $1::uuid",
    )
    .bind(&sid)
    .execute(&st.pool)
    .await
    .unwrap();
    st.emit(DomainEvent::new(
        "service.suspended",
        json!({ "service_id": sid, "from": "active", "reason": "overdue" }),
    ));
    assert_eq!(wait(&st, &sid, "suspend", 1).await, 1);
    sqlx::query("UPDATE services SET status = 'active', suspend_reason = '' WHERE id = $1::uuid")
        .bind(&sid)
        .execute(&st.pool)
        .await
        .unwrap();
    st.emit(DomainEvent::new(
        "service.unsuspended",
        json!({ "service_id": sid, "from": "suspended", "reason": "paid" }),
    ));
    assert_eq!(wait(&st, &sid, "unsuspend", 1).await, 1);
    // Handlers that already enqueued mark the event; no duplicate job.
    let s = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/action"),
        Some(json!({"action": "suspend", "reason": "manual"})),
        &admin.h(),
    )
    .await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(jobs(&st, &sid, "suspend").await, 2);
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/action"),
        Some(json!({"action": "unsuspend"})),
        &admin.h(),
    )
    .await;

    // Paid upgrade → change_package job.
    let next = time::OffsetDateTime::now_utc().date() + time::Duration::days(10);
    sqlx::query("UPDATE services SET next_due_date = $2 WHERE id = $1::uuid")
        .bind(&sid)
        .bind(next)
        .execute(&st.pool)
        .await
        .unwrap();
    let c = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/change"),
        Some(json!({"product_id": big})),
        &admin.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    assert_eq!(
        jobs(&st, &sid, "change_package").await,
        0,
        "not before payment"
    );
    let iid = c.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = c.body["invoice"]["total"].as_str().unwrap().to_string();
    let pay = call(&app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": uuid::Uuid::new_v4().to_string(), "reference": format!("pe-{iid}")})), &admin.h()).await;
    assert_eq!(pay.status, StatusCode::OK, "{:?}", pay.body);
    assert_eq!(wait(&st, &sid, "change_package", 1).await, 1);
}
