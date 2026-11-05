use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_database::invoices;
use rust_decimal::Decimal;
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

// One test: it drives the global daily automation with a synthetic "today".
#[tokio::test]
async fn overdue_reminders_late_fee_suspend_and_unsuspend() {
    let (app, st) = app().await;
    let pool = &st.pool;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE", "first_name": "Alice"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Settings: staff only, validated.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/settings/overdue",
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/overdue",
        Some(json!({"enabled": true, "late_fee_percent": "150"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({
        "enabled": true, "reminder_days": [7, 1], "late_fee_days": 3, "late_fee_fixed": "5.00", "late_fee_percent": "10", "suspend_days": 10
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["reminder_days"], json!([1, 7]));

    // An order: unpaid invoice linked to an (activated) service.
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let oid_order = r.body["order"]["id"].as_str().unwrap().to_string();
    let order_inv: Uuid = r.body["invoice"]["id"].as_str().unwrap().parse().unwrap();
    let ot = r.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{order_inv}/payments"),
        Some(json!({"amount": ot, "idempotency_key": format!("od-{order_inv}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let a = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{}/approve", oid_order),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let (sid, cid, st0): (Uuid, Uuid, String) = sqlx::query_as("SELECT s.id, s.customer_id, s.status FROM services s JOIN order_items oi ON oi.id = s.order_item_id JOIN orders o ON o.id = oi.order_id WHERE o.invoice_id = $1")
        .bind(order_inv).fetch_one(pool).await.unwrap();
    if st0 != "active" {
        let a = call(
            &app,
            "POST",
            &format!("/api/v1/admin/services/{sid}/action"),
            Some(json!({"action": "activate"})),
            &admin.h(),
        )
        .await;
        assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    }
    // A renewal invoice for the service, left unpaid.
    let mut tx = pool.begin().await.unwrap();
    let inv = invoices::create_draft(&mut tx, cid, "EUR", None)
        .await
        .unwrap();
    let line = invoices::NewItem {
        description: "VPS S renewal".into(),
        quantity: Decimal::ONE,
        unit_price: "12.34".parse().unwrap(),
        tax_rate: Decimal::ZERO,
        service_id: Some(sid),
        period: None,
        auto_tax: false,
    };
    invoices::add_items(&mut tx, inv, "EUR", &[line])
        .await
        .unwrap();
    let total = invoices::finalize(&mut tx, inv, 7).await.unwrap().total;
    tx.commit().await.unwrap();
    let due: time::Date = sqlx::query_scalar("SELECT due_at::date FROM invoices WHERE id = $1")
        .bind(inv)
        .fetch_one(pool)
        .await
        .unwrap();
    let run = |d: i64| pelty_database::overdue::run_overdue(pool, due + Duration::days(d));
    let status = || async {
        sqlx::query_as::<_, (String, String)>(
            "SELECT status, suspend_reason FROM services WHERE id = $1",
        )
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
    };

    // Not overdue on the due date itself.
    let r = run(0).await.unwrap();
    assert!(!r.reminders.iter().any(|m| m.invoice_id == inv));

    // Day 1: one reminder, exactly once.
    let r = run(1).await.unwrap();
    let m: Vec<_> = r.reminders.iter().filter(|m| m.invoice_id == inv).collect();
    assert_eq!((m.len(), m[0].days_overdue, m[0].balance), (1, 1, total));
    assert!(!run(1)
        .await
        .unwrap()
        .reminders
        .iter()
        .any(|m| m.invoice_id == inv));
    assert!(!run(2)
        .await
        .unwrap()
        .reminders
        .iter()
        .any(|m| m.invoice_id == inv));

    // Day 3: a separate late-fee invoice of 5.00 + 10%, charged once.
    let r = run(3).await.unwrap();
    let f: Vec<_> = r.late_fees.iter().filter(|f| f.invoice_id == inv).collect();
    assert_eq!(f.len(), 1);
    let expect = pelty_billing::math::round(Decimal::from(5) + total / Decimal::from(10), 2);
    assert_eq!(f[0].amount, expect);
    let fee = f[0].fee_invoice_id;
    let (ft, fs, fk): (Decimal, String, String) =
        sqlx::query_as("SELECT total, status, kind FROM invoices WHERE id = $1")
            .bind(fee)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        (ft, fs.as_str(), fk.as_str()),
        (expect, "unpaid", "invoice")
    );
    let orig: Decimal = sqlx::query_scalar("SELECT total FROM invoices WHERE id = $1")
        .bind(inv)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(orig, total, "original invoice untouched");
    assert!(!run(4)
        .await
        .unwrap()
        .late_fees
        .iter()
        .any(|f| f.invoice_id == inv));
    assert_eq!(status().await.0, "active");

    // Day 10 (via the job + side effects): 7-day reminder and suspension.
    let res = pelty_api::overdue::run(&st, due + Duration::days(10))
        .await
        .unwrap();
    assert!(res["suspended"].as_u64().unwrap() >= 1);
    assert_eq!(status().await, ("suspended".into(), "overdue".into()));
    let mails = mails_to(&st, &alice.email).await;
    let reminders = mails
        .iter()
        .filter(|m| m.subject.contains("overdue"))
        .count();
    assert_eq!(reminders, 1, "only the job run mails (the day-7 reminder)");
    assert!(
        mails
            .iter()
            .any(|m| m.subject.to_lowercase().contains("suspend")),
        "{:?}",
        mails.iter().map(|m| &m.subject).collect::<Vec<_>>()
    );
    // The fee invoice itself is never reminded/fined/suspended for.
    let again = pelty_database::overdue::run_overdue(pool, due + Duration::days(40))
        .await
        .unwrap();
    assert!(!again
        .reminders
        .iter()
        .any(|m| m.invoice_id == fee || m.invoice_id == inv));
    assert!(!again.late_fees.iter().any(|f| f.invoice_id == fee));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM overdue_actions WHERE invoice_id = $1")
        .bind(inv)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(n, 4, "reminder:1, reminder:7, late_fee, suspend");

    // Paying the overdue invoice reactivates the service.
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{inv}/payments"),
        Some(json!({"amount": total.to_string(), "idempotency_key": format!("od-{inv}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert_eq!(status().await, ("active".into(), "".into()));

    // Manual suspension stays even if an invoice is paid.
    let a = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/action"),
        Some(json!({"action": "suspend", "reason": "abuse"})),
        &admin.h(),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{fee}/payments"),
        Some(json!({"amount": expect.to_string(), "idempotency_key": format!("od-{fee}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert_eq!(status().await.0, "suspended");

    // Disabled: nothing happens.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/overdue",
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = pelty_database::overdue::run_overdue(
        pool,
        OffsetDateTime::now_utc().date() + Duration::days(400),
    )
    .await
    .unwrap();
    assert!(r.reminders.is_empty() && r.late_fees.is_empty() && r.suspended.is_empty());

    // Daily job is keyed per date.
    assert!(
        pelty_api::overdue::schedule(&st).await.unwrap().is_none()
            || pelty_api::overdue::schedule(&st).await.unwrap().is_none()
    );
}
