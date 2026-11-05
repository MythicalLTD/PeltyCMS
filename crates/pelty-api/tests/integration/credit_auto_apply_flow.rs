use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn credit_auto_applies_to_new_invoices() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let r = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &c.h()).await;
    let cid = r.body["id"].as_str().unwrap().to_string();
    let cfg = |on: bool| json!({"enabled": false, "min": "10", "max": "500", "max_balance": "1000", "require_active_service": false, "auto_apply_credit": on});
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/add-funds", Some(cfg(true)), &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/customers/{cid}/credit"), Some(json!({"currency": "EUR", "amount": "30"})), &admin.h()).await.status, StatusCode::OK);
    let mk = |price: &str| json!({"customer_id": cid, "currency": "EUR", "finalize": true, "items": [{"description": "Hosting", "quantity": "1", "unit_price": price, "tax_rate": "0"}]});
    let a = call(&app, "POST", "/api/v1/admin/invoices", Some(mk("20")), &admin.h()).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let a = a.body["invoice"]["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap();
    let status = |id: uuid::Uuid| { let p = st.pool.clone(); async move { sqlx::query_scalar::<_, String>("SELECT status::text FROM invoices WHERE id = $1").bind(id).fetch_one(&p).await.unwrap() } };
    let mut ok = false;
    for _ in 0..50 { if status(a).await == "paid" { ok = true; break } tokio::time::sleep(std::time::Duration::from_millis(100)).await; }
    assert!(ok, "first invoice paid from credit");
    // Second invoice: 10 left of 30, partially covered.
    let b = call(&app, "POST", "/api/v1/admin/invoices", Some(mk("25")), &admin.h()).await;
    let b = b.body["invoice"]["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap();
    let paid = || { let p = st.pool.clone(); async move { sqlx::query_scalar::<_, rust_decimal::Decimal>("SELECT amount_paid FROM invoices WHERE id = $1").bind(b).fetch_one(&p).await.unwrap() } };
    for _ in 0..50 { if paid().await > rust_decimal::Decimal::ZERO { break } tokio::time::sleep(std::time::Duration::from_millis(100)).await; }
    assert_eq!(paid().await, rust_decimal::Decimal::from(10));
    assert_eq!(status(b).await, "unpaid");
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/add-funds", Some(cfg(false)), &admin.h()).await.status, StatusCode::OK);
}
