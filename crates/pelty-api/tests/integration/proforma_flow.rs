use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_database::invoices;
use rust_decimal::Decimal;
use serde_json::json;
use uuid::Uuid;

async fn issue(pool: &sqlx::PgPool, cid: Uuid, price: &str) -> invoices::Invoice {
    let mut tx = pool.begin().await.unwrap();
    let inv = invoices::create_draft(&mut tx, cid, "EUR", None).await.unwrap();
    let line = invoices::NewItem { description: "Hosting".into(), quantity: Decimal::ONE, unit_price: price.parse().unwrap(), tax_rate: Decimal::ZERO, service_id: None, period: None, auto_tax: false };
    invoices::add_items(&mut tx, inv, "EUR", &[line]).await.unwrap();
    let i = invoices::finalize(&mut tx, inv, 7).await.unwrap();
    tx.commit().await.unwrap();
    i
}

#[tokio::test]
async fn proforma_invoices_get_number_on_payment() {
    let (app, st) = app().await;
    let pool = &st.pool;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let r = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE", "first_name": "Alice"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&alice.email).fetch_one(pool).await.unwrap();
    let cid = pelty_database::customers::id_for_user(pool, uid).await.unwrap();

    pelty_database::settings::set(pool, invoices::PROFORMA_KEY, json!(false)).await.unwrap();
    assert_eq!(call(&app, "GET", "/api/v1/admin/settings/proforma", None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/settings/proforma", None, &admin.h()).await.body["enabled"], false);
    // Off: numbered on issue.
    assert!(issue(pool, cid, "10.00").await.number.is_some());

    let r = call(&app, "PUT", "/api/v1/admin/settings/proforma", Some(json!({"enabled": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/proforma", Some(json!({"enabled": "yes"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    let inv = issue(pool, cid, "20.00").await;
    assert!(inv.number.is_none(), "proforma has no number");
    assert_eq!(inv.status, "unpaid");
    assert!(inv.finalized_at.is_some());
    // The client can still see and download it.
    let d = call(&app, "GET", &format!("/api/v1/client/invoices/{}", inv.id), None, &alice.h()).await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    let pdf = call(&app, "GET", &format!("/api/v1/client/invoices/{}/pdf", inv.id), None, &alice.h()).await;
    assert_eq!(pdf.status, StatusCode::OK);

    // Cron jobs read unnumbered invoices without failing.
    let later = time::OffsetDateTime::now_utc().date() + time::Duration::days(60);
    pelty_database::overdue::run_overdue(pool, later).await.unwrap();
    pelty_database::invoice_tools::run_due_reminders(pool, time::OffsetDateTime::now_utc().date()).await.unwrap();

    // Partial payment: still proforma. Full payment: numbered.
    invoices::record_payment(pool, inv.id, "manual", None, "5.00".parse().unwrap(), &format!("pf-1-{}", inv.id), None).await.unwrap();
    assert!(invoices::get(pool, inv.id).await.unwrap().unwrap().number.is_none());
    invoices::record_payment(pool, inv.id, "manual", None, "15.00".parse().unwrap(), &format!("pf-2-{}", inv.id), None).await.unwrap();
    let paid = invoices::get(pool, inv.id).await.unwrap().unwrap();
    assert_eq!(paid.status, "paid");
    let n = paid.number.expect("numbered on payment");
    assert!(n.starts_with("INV-"), "{n}");
    // Replaying a payment does not renumber.
    invoices::record_payment(pool, inv.id, "manual", None, "15.00".parse().unwrap(), &format!("pf-2-{}", inv.id), None).await.unwrap();
    assert_eq!(invoices::get(pool, inv.id).await.unwrap().unwrap().number.as_deref(), Some(n.as_str()));
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/proforma", Some(json!({"enabled": false})), &admin.h()).await.status, StatusCode::OK);
}
