use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::json;
use time::{Duration, OffsetDateTime};
use pelty_database::invoices;
use uuid::Uuid;

// One test: it drives the global daily automation with a synthetic "today".
#[tokio::test]
async fn overdue_terminate_and_auto_cancel() {
    let (app, st) = app().await;
    let pool = &st.pool;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let r = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE", "first_name": "Alice"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Settings: staff only, validated.
    assert_eq!(call(&app, "GET", "/api/v1/admin/settings/overdue", None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    let bad = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({"enabled": true, "late_fee_percent": "150"})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({
        "enabled": true, "reminder_days": [1], "suspend_days": 5, "terminate_days": 10
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["terminate_days"], 10);
    // Termination must come after suspension.
    let bad = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({"enabled": true, "suspend_days": 10, "terminate_days": 5})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);

    // An order: unpaid invoice linked to an (activated) service.
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let oid_order = r.body["order"]["id"].as_str().unwrap().to_string();
    let order_inv: Uuid = r.body["invoice"]["id"].as_str().unwrap().parse().unwrap();
    let ot = r.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(&app, "POST", &format!("/api/v1/admin/invoices/{order_inv}/payments"), Some(json!({"amount": ot, "idempotency_key": format!("od-{order_inv}")})), &admin.h()).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let a = call(&app, "POST", &format!("/api/v1/admin/orders/{}/approve", oid_order), None, &admin.h()).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let (sid, cid, st0): (Uuid, Uuid, String) = sqlx::query_as("SELECT s.id, s.customer_id, s.status FROM services s JOIN order_items oi ON oi.id = s.order_item_id JOIN orders o ON o.id = oi.order_id WHERE o.invoice_id = $1")
        .bind(order_inv).fetch_one(pool).await.unwrap();
    if st0 != "active" {
        let a = call(&app, "POST", &format!("/api/v1/admin/services/{sid}/action"), Some(json!({"action": "activate"})), &admin.h()).await;
        assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    }
    // A renewal invoice for the service, left unpaid.
    let mut tx = pool.begin().await.unwrap();
    let inv = invoices::create_draft(&mut tx, cid, "EUR", None).await.unwrap();
    let line = invoices::NewItem { description: "VPS S renewal".into(), quantity: Decimal::ONE, unit_price: "12.34".parse().unwrap(), tax_rate: Decimal::ZERO, service_id: Some(sid), period: None, auto_tax: false };
    invoices::add_items(&mut tx, inv, "EUR", &[line]).await.unwrap();
    let total = invoices::finalize(&mut tx, inv, 7).await.unwrap().total;
    tx.commit().await.unwrap();
    let due: time::Date = sqlx::query_scalar("SELECT due_at::date FROM invoices WHERE id = $1").bind(inv).fetch_one(pool).await.unwrap();
    let run = |d: i64| pelty_database::overdue::run_overdue(pool, due + Duration::days(d));
    let status = || async { sqlx::query_scalar::<_, String>("SELECT status FROM services WHERE id = $1").bind(sid).fetch_one(pool).await.unwrap() };

    run(5).await.unwrap();
    assert_eq!(status().await, "suspended");
    let r = run(9).await.unwrap();
    assert!(!r.terminated.iter().any(|x| x.service_id == sid));
    assert_eq!(status().await, "suspended");
    let res = pelty_api::overdue::run(&st, due + Duration::days(10)).await.unwrap();
    assert!(res["terminated"].as_u64().unwrap() >= 1, "{res}");
    assert_eq!(status().await, "terminated");
    assert!(!run(11).await.unwrap().terminated.iter().any(|x| x.service_id == sid), "once only");
    let _ = total;

    // Invoice auto-cancellation: unpaid, no money received → void after N days.
    let r = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({"enabled": true, "suspend_days": 0, "cancel_unpaid_days": 3})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mut tx = pool.begin().await.unwrap();
    let inv2 = invoices::create_draft(&mut tx, cid, "EUR", None).await.unwrap();
    let line = invoices::NewItem { description: "Setup".into(), quantity: Decimal::ONE, unit_price: "9.99".parse().unwrap(), tax_rate: Decimal::ZERO, service_id: None, period: None, auto_tax: false };
    invoices::add_items(&mut tx, inv2, "EUR", &[line]).await.unwrap();
    invoices::finalize(&mut tx, inv2, 7).await.unwrap();
    tx.commit().await.unwrap();
    let due2: time::Date = sqlx::query_scalar("SELECT due_at::date FROM invoices WHERE id = $1").bind(inv2).fetch_one(pool).await.unwrap();
    let st2 = || async { sqlx::query_scalar::<_, String>("SELECT status FROM invoices WHERE id = $1").bind(inv2).fetch_one(pool).await.unwrap() };
    pelty_database::overdue::run_overdue(pool, due2 + Duration::days(2)).await.unwrap();
    assert_eq!(st2().await, "unpaid");
    let r = pelty_database::overdue::run_overdue(pool, due2 + Duration::days(3)).await.unwrap();
    assert!(r.voided.contains(&inv2));
    assert_eq!(st2().await, "void");

    let r = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({"enabled": false})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let _ = OffsetDateTime::now_utc();
}
