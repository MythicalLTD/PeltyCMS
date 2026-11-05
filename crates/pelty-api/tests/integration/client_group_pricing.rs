use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn group_discount_applies_to_recurring_not_setup() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let cust = sess(&app, &st, "customer").await;
    let other = sess(&app, &st, "customer").await;
    for s in [&cust, &other] {
        assert_eq!(call(&app, "GET", "/api/v1/client/profile", None, &s.h()).await.status, StatusCode::OK);
    }
    let name = format!("VIP {}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let g = call(&app, "POST", "/api/v1/admin/client-groups", Some(json!({"name": name, "discount_percent": "12.50"})), &admin.h()).await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let gid = g.body["group"]["id"].as_str().unwrap();
    let r = call(&app, "POST", &format!("/api/v1/admin/client-groups/{gid}/members"), Some(json!({"email": cust.email})), &admin.h()).await;
    assert!(r.status.is_success(), "{:?}", r.body);

    let body = json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.as_str(): ch[0]}}]});
    // 10.00 monthly + 5.00 setup; 12.5% of 10.00 = 1.25 → 13.75
    let r = call(&app, "POST", "/api/v1/client/orders", Some(body.clone()), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "13.7500", "{:?}", r.body);
    let d: rust_decimal::Decimal = sqlx::query_scalar("SELECT discount FROM orders WHERE id = $1::uuid")
        .bind(r.body["order"]["id"].as_str().unwrap()).fetch_one(&st.pool).await.unwrap();
    assert_eq!(d, rust_decimal::Decimal::new(125, 2));
    let line: String = sqlx::query_scalar("SELECT description FROM invoice_items WHERE invoice_id = $1::uuid AND unit_price < 0")
        .bind(r.body["invoice"]["id"].as_str().unwrap()).fetch_one(&st.pool).await.unwrap();
    assert!(line.contains(&name) && line.contains("12.5%"), "{line}");
    // Non-members pay full price.
    let r = call(&app, "POST", "/api/v1/client/orders", Some(body), &other.h()).await;
    assert_eq!(r.body["invoice"]["total"], "15.0000", "{:?}", r.body);
}

#[tokio::test]
async fn late_fee_exempt_group_skips_late_fee() {
    use rust_decimal::Decimal;
    let (app, st) = app().await;
    let pool = &st.pool;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    assert_eq!(call(&app, "GET", "/api/v1/client/profile", None, &cust.h()).await.status, StatusCode::OK);
    let r = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({
        "enabled": true, "reminder_days": [1], "late_fee_days": 1, "late_fee_fixed": "5.00", "late_fee_percent": "0", "suspend_days": 0
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cid: uuid::Uuid = sqlx::query_scalar("SELECT c.id FROM customers c JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1").bind(&cust.email).fetch_one(pool).await.unwrap();
    let gid: uuid::Uuid = sqlx::query_scalar("INSERT INTO client_groups (id, name, late_fee_exempt) VALUES (gen_random_uuid(), $1, true) RETURNING id")
        .bind(format!("exempt-{}", uuid::Uuid::new_v4().simple())).fetch_one(pool).await.unwrap();
    sqlx::query("UPDATE customers SET client_group_id = $2 WHERE id = $1").bind(cid).bind(gid).execute(pool).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    let inv = pelty_database::invoices::create_draft(&mut tx, cid, "EUR", None).await.unwrap();
    let line = pelty_database::invoices::NewItem { description: "x".into(), quantity: Decimal::ONE, unit_price: "10".parse().unwrap(), tax_rate: Decimal::ZERO, service_id: None, period: None, auto_tax: false };
    pelty_database::invoices::add_items(&mut tx, inv, "EUR", &[line]).await.unwrap();
    pelty_database::invoices::finalize(&mut tx, inv, 7).await.unwrap();
    tx.commit().await.unwrap();
    let due: time::Date = sqlx::query_scalar("SELECT due_at::date FROM invoices WHERE id = $1").bind(inv).fetch_one(pool).await.unwrap();

    let r = pelty_database::overdue::run_overdue(pool, due + time::Duration::days(2)).await.unwrap();
    assert!(r.reminders.iter().any(|m| m.invoice_id == inv), "still reminded");
    assert!(!r.late_fees.iter().any(|f| f.invoice_id == inv), "exempt group: no late fee");

    // Leaving the group makes the fee apply.
    sqlx::query("UPDATE customers SET client_group_id = NULL WHERE id = $1").bind(cid).execute(pool).await.unwrap();
    let r = pelty_database::overdue::run_overdue(pool, due + time::Duration::days(2)).await.unwrap();
    assert!(r.late_fees.iter().any(|f| f.invoice_id == inv && f.amount == Decimal::from(5)));
    let off = call(&app, "PUT", "/api/v1/admin/settings/overdue", Some(json!({"enabled": false})), &admin.h()).await;
    assert_eq!(off.status, StatusCode::OK, "{:?}", off.body);
}
