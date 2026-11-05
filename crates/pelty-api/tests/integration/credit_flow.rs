use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

fn n(v: &Value) -> String {
    v.as_str().unwrap_or_default().parse::<rust_decimal::Decimal>().map(|d| d.normalize().to_string()).unwrap_or_default()
}

async fn customer(app: &axum::Router, st: &pelty_api::AppState) -> (Sess, String) {
    let c = sess(app, st, "customer").await;
    let r = call(app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &c.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    (c, id)
}

async fn invoice(app: &axum::Router, admin: &Sess, cid: &str, price: &str) -> String {
    let r = call(app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true,
        "items": [{"description": "Hosting", "quantity": "1", "unit_price": price, "tax_rate": "0"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(n(&r.body["invoice"]["total"]), price.parse::<rust_decimal::Decimal>().unwrap().normalize().to_string());
    r.body["invoice"]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn credit_ledger_adjust_apply_partial_and_concurrency() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (alice, aid) = customer(&app, &st).await;
    let (bob, _) = customer(&app, &st).await;
    let credit = |cid: &str| format!("/api/v1/admin/customers/{cid}/credit");

    // Staff only; validation; can't go negative.
    assert_eq!(call(&app, "POST", &credit(&aid), Some(json!({"currency": "EUR", "amount": "5"})), &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", &credit(&aid), Some(json!({"currency": "EUR", "amount": "0"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", &credit(&aid), Some(json!({"currency": "EUR", "amount": "1.001"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", &credit(&aid), Some(json!({"currency": "EUR", "amount": "-1"})), &admin.h()).await.status, StatusCode::CONFLICT);
    let r = call(&app, "POST", &credit(&aid), Some(json!({"currency": "eur", "amount": "50.00", "note": "goodwill"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(n(&r.body["balance"]), "50");

    let r = call(&app, "GET", "/api/v1/client/credit", None, &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!((r.body["balances"][0]["currency"].as_str(), n(&r.body["balances"][0]["amount"])), (Some("EUR"), "50".into()));
    assert_eq!(r.body["entries"][0]["note"], "goodwill");
    let r = call(&app, "GET", "/api/v1/client/credit", None, &bob.h()).await;
    assert_eq!(r.body["balances"], json!([]));

    // Full payment from credit.
    let i1 = invoice(&app, &admin, &aid, "30.00").await;
    assert_eq!(call(&app, "POST", &format!("/api/v1/client/invoices/{i1}/apply-credit"), None, &bob.h()).await.status, StatusCode::NOT_FOUND);
    let r = call(&app, "POST", &format!("/api/v1/client/invoices/{i1}/apply-credit"), None, &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!((n(&r.body["amount"]), n(&r.body["balance"]), r.body["invoice"]["status"].as_str()), ("30".into(), "20".into(), Some("paid")));
    assert_eq!(call(&app, "POST", &format!("/api/v1/client/invoices/{i1}/apply-credit"), None, &alice.h()).await.status, StatusCode::CONFLICT);

    // Partial: explicit amount, too much, then the rest.
    let i2 = invoice(&app, &admin, &aid, "100.00").await;
    let ap = format!("/api/v1/client/invoices/{i2}/apply-credit");
    assert_eq!(call(&app, "POST", &ap, Some(json!({"amount": "25"})), &alice.h()).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "POST", &ap, Some(json!({"amount": "-1"})), &alice.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", &ap, Some(json!({"amount": "7.50"})), &alice.h()).await;
    assert_eq!((r.status, n(&r.body["balance"])), (StatusCode::OK, "12.5".into()));
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{i2}/apply-credit"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!((n(&r.body["amount"]), n(&r.body["balance"])), ("12.5".into(), "0".into()));
    assert_eq!((r.body["invoice"]["status"].as_str(), n(&r.body["invoice"]["amount_paid"])), (Some("unpaid"), "20".into()));
    assert_eq!(call(&app, "POST", &ap, None, &alice.h()).await.status, StatusCode::CONFLICT);
    let pays: i64 = sqlx::query_scalar("SELECT count(*) FROM payments WHERE invoice_id = $1::uuid AND gateway = 'credit' AND status = 'succeeded'")
        .bind(&i2).fetch_one(&st.pool).await.unwrap();
    assert_eq!(pays, 2);

    // Ledger is append-only.
    assert!(pelty_database::raw_execute(&st.pool, &format!("UPDATE credit_ledger SET amount = 1 WHERE customer_id = '{aid}'")).await.is_err());
    assert!(pelty_database::raw_execute(&st.pool, &format!("DELETE FROM credit_ledger WHERE customer_id = '{aid}'")).await.is_err());

    // Concurrent applications never overspend.
    let r = call(&app, "POST", &credit(&aid), Some(json!({"currency": "EUR", "amount": "10"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let a = invoice(&app, &admin, &aid, "10.00").await;
    let b = invoice(&app, &admin, &aid, "10.00").await;
    let (pa, pb) = (format!("/api/v1/client/invoices/{a}/apply-credit"), format!("/api/v1/client/invoices/{b}/apply-credit"));
    let h = alice.h();
    let (ra, rb) = tokio::join!(call(&app, "POST", &pa, None, &h), call(&app, "POST", &pb, None, &h));
    let ok = [ra.status, rb.status].iter().filter(|s| **s == StatusCode::OK).count();
    assert_eq!(ok, 1, "{:?} {:?}", ra.body, rb.body);
    let bal: rust_decimal::Decimal = sqlx::query_scalar("SELECT sum(amount) FROM credit_ledger WHERE customer_id = $1::uuid").bind(&aid).fetch_one(&st.pool).await.unwrap();
    assert!(bal.is_zero());
    let r = call(&app, "GET", &credit(&aid), None, &admin.h()).await;
    assert_eq!(r.body["balances"], json!([]));
    assert_eq!(r.body["entries"].as_array().unwrap().len(), 6);
}
