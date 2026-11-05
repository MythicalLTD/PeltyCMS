use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_addon_gateway_stripe::stripe::{HttpRequestSpec, StripeGateway, StripeTransport};
use pelty_payments::{BoxFuture, GatewayConfig, GatewayError, GatewayRegistry, PaymentGateway};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Offline Stripe: customers / attach / off-session PaymentIntents. Never live.
static DECLINE: AtomicBool = AtomicBool::new(false);
static CHARGES: Mutex<Vec<HttpRequestSpec>> = Mutex::new(Vec::new());
struct MockStripe;
impl StripeTransport for MockStripe {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            assert!(req.url.starts_with("https://api.stripe.com/v1/"));
            let id = uuid::Uuid::new_v4().simple();
            Ok(if req.url.ends_with("/customers") {
                (200, format!(r#"{{"id":"cus_{id}"}}"#))
            } else if req.url.contains("/attach") {
                (200, r#"{"id":"pm_x","card":{"brand":"visa","last4":"4242","exp_month":12,"exp_year":2031}}"#.into())
            } else {
                CHARGES.lock().unwrap().push(req);
                if DECLINE.load(Ordering::SeqCst) {
                    (402, r#"{"error":{"type":"card_error","code":"card_declined","decline_code":"insufficient_funds"}}"#.into())
                } else {
                    (200, format!(r#"{{"id":"pi_{id}","status":"succeeded"}}"#))
                }
            })
        })
    }
}
fn mock_factory(_: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    Ok(Box::new(StripeGateway::new("sk_test_mock", "whsec_mock", Box::new(MockStripe))))
}

async fn invoice_due(app: &axum::Router, st: &pelty_api::AppState, admin: &Sess, cid: &str, price: &str) -> String {
    let r = call(app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true,
        "items": [{"description": "Hosting", "quantity": "1", "unit_price": price, "tax_rate": "0"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["invoice"]["id"].as_str().unwrap().to_string();
    pelty_database::raw_execute(&st.pool, &format!("UPDATE invoices SET due_at = '2026-01-10T12:00:00Z' WHERE id = '{id}'")).await.unwrap();
    id
}

async fn attempts(st: &pelty_api::AppState, inv: &str) -> Vec<(i32, String, Option<String>)> {
    sqlx::query_as("SELECT attempt, status, error FROM autopay_attempts WHERE invoice_id = $1::uuid ORDER BY attempt")
        .bind(inv).fetch_all(&st.pool).await.unwrap()
}

async fn mails(st: &pelty_api::AppState, to: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'payment_failed' AND payload->>'to' = $1 ORDER BY id")
        .bind(to).fetch_all(&st.pool).await.unwrap()
}

#[tokio::test]
async fn saved_methods_autopay_retries_and_dunning() {
    let (_, mut st) = app().await;
    let mut reg = GatewayRegistry::default();
    reg.register(pelty_addon_gateway_stripe::stripe::descriptor(), mock_factory);
    reg.register(pelty_addon_gateway_manual::manual::descriptor(), pelty_addon_gateway_manual::manual::factory);
    st.gateways = Arc::new(reg);
    pelty_database::raw_execute(&st.pool, "INSERT INTO payment_gateways (id, enabled) VALUES ('stripe', TRUE) ON CONFLICT (id) DO UPDATE SET enabled = TRUE").await.unwrap();
    let app = pelty_api::router(st.clone());
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let aid = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE", "first_name": "Alice"})), &alice.h()).await.body["id"].as_str().unwrap().to_string();
    let email: String = sqlx::query_scalar("SELECT email::text FROM customers WHERE id = $1::uuid").bind(&aid).fetch_one(&st.pool).await.unwrap();
    let base = "/api/v1/client/billing/payment-methods";

    // Nothing saved yet: auto-pay cannot be enabled; Stripe is offered (not manual).
    let r = call(&app, "GET", base, None, &alice.h()).await;
    assert_eq!((r.status, r.body["methods"].as_array().unwrap().len(), r.body["auto_pay"].clone()), (StatusCode::OK, 0, json!(false)));
    assert_eq!(r.body["gateways"], json!(["stripe"]));
    let r = call(&app, "PUT", "/api/v1/client/billing/auto-pay", Some(json!({"enabled": true})), &alice.h()).await;
    assert_eq!((r.status, r.body["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("error.no_payment_method")));

    // Save methods: token is never returned; first is default; bad tokens rejected.
    assert_eq!(call(&app, "POST", base, Some(json!({"gateway": "manual", "token": "pm_1"})), &alice.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", base, Some(json!({"gateway": "stripe", "token": "tok_../x"})), &alice.h()).await.status, StatusCode::BAD_REQUEST);
    let m1 = call(&app, "POST", base, Some(json!({"gateway": "stripe", "token": "pm_card_visa"})), &alice.h()).await;
    assert_eq!(m1.status, StatusCode::OK, "{:?}", m1.body);
    assert_eq!((m1.body["last4"].as_str(), m1.body["is_default"].as_bool(), m1.body["exp_year"].as_i64()), (Some("4242"), Some(true), Some(2031)));
    assert!(m1.body.get("token").is_none());
    let m1 = m1.body["id"].as_str().unwrap().to_string();
    let m2 = call(&app, "POST", base, Some(json!({"gateway": "stripe", "token": "pm_card_mc"})), &alice.h()).await.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", &format!("{base}/{m2}/default"), None, &alice.h()).await;
    assert_eq!(r.body["methods"][0]["id"].as_str(), Some(m2.as_str()));
    assert_eq!(call(&app, "DELETE", &format!("{base}/{m2}"), None, &bob.h()).await.status, StatusCode::NOT_FOUND);
    let r = call(&app, "DELETE", &format!("{base}/{m2}"), None, &alice.h()).await;
    assert_eq!((r.body["methods"].as_array().unwrap().len(), r.body["methods"][0]["id"].as_str(), r.body["methods"][0]["is_default"].as_bool()), (1, Some(m1.as_str()), Some(true)));
    let r = call(&app, "PUT", "/api/v1/client/billing/auto-pay", Some(json!({"enabled": true})), &alice.h()).await;
    assert_eq!((r.status, r.body["auto_pay"].clone()), (StatusCode::OK, json!(true)));

    // Settings: staff only, validated, normalized.
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/autopay", Some(json!({"retry_days": [0]})), &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/autopay", Some(json!({"retry_days": [400]})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "PUT", "/api/v1/admin/settings/autopay", Some(json!({"enabled": true, "retry_days": [5, 0, 2]})), &admin.h()).await;
    assert_eq!((r.status, r.body["retry_days"].clone()), (StatusCode::OK, json!([0, 2, 5])));

    let run = |d: &'static str| { let app = app.clone(); let h = admin.h(); async move {
        let r = call(&app, "POST", "/api/v1/admin/autopay/run", Some(json!({"date": d})), &h).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    } };

    // Invoice A (12.34): declined on the due date, retried on day 2 → paid.
    let a = invoice_due(&app, &st, &admin, &aid, "12.34").await;
    DECLINE.store(true, Ordering::SeqCst);
    run("2026-01-09").await;
    assert!(attempts(&st, &a).await.is_empty());
    run("2026-01-10").await;
    assert_eq!(attempts(&st, &a).await, vec![(0, "declined".into(), Some("insufficient_funds".into()))]);
    let m = mails(&st, &email).await;
    assert_eq!(m.len(), 1);
    assert_eq!((m[0]["vars"]["next_retry"].as_str(), m[0]["vars"]["total"].as_str(), m[0]["vars"]["name"].as_str()), (Some("2026-01-12"), Some("12.34 EUR"), Some("Alice")));
    let charge = CHARGES.lock().unwrap().last().cloned().unwrap();
    assert!(charge.body.contains("amount=1234") && charge.body.contains("off_session=true") && charge.body.contains("payment_method=pm_card_visa"));
    assert!(charge.headers.iter().any(|(k, v)| k == "Idempotency-Key" && v == &format!("pelty-autopay:{a}:0")));
    run("2026-01-10").await;
    run("2026-01-11").await;
    assert_eq!(attempts(&st, &a).await.len(), 1);
    DECLINE.store(false, Ordering::SeqCst);
    run("2026-01-12").await;
    assert_eq!(attempts(&st, &a).await[1], (1, "succeeded".into(), None));
    let r = call(&app, "GET", &format!("/api/v1/admin/invoices/{a}"), None, &admin.h()).await;
    assert_eq!(r.body["invoice"]["status"], "paid");
    let st_pay: Vec<String> = r.body["payments"].as_array().unwrap().iter().map(|p| p["status"].as_str().unwrap().to_string()).collect();
    assert_eq!(st_pay.iter().filter(|s| *s == "failed").count(), 1);
    assert_eq!(st_pay.iter().filter(|s| *s == "succeeded").count(), 1);
    run("2026-01-15").await;
    assert_eq!(attempts(&st, &a).await.len(), 2);

    // Invoice B: every attempt declined → 3 attempts, 3 dunning mails, then stops.
    let b = invoice_due(&app, &st, &admin, &aid, "5.00").await;
    DECLINE.store(true, Ordering::SeqCst);
    for d in ["2026-01-10", "2026-01-12", "2026-01-15", "2026-01-20"] {
        run(d).await;
    }
    assert_eq!(attempts(&st, &b).await.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), vec!["declined"; 3]);
    let m = mails(&st, &email).await;
    assert_eq!(m.len(), 4);
    assert_eq!(m[3]["vars"]["next_retry"].as_str(), Some("No further automatic attempts will be made."));
    let r = call(&app, "GET", &format!("/api/v1/admin/invoices/{b}"), None, &admin.h()).await;
    assert_eq!(r.body["invoice"]["status"], "unpaid");

    // Admin view + client history; turning auto-pay off stops new attempts.
    let r = call(&app, "GET", &format!("/api/v1/admin/customers/{aid}/payment-methods"), None, &admin.h()).await;
    assert_eq!((r.status, r.body["attempts"].as_array().unwrap().len()), (StatusCode::OK, 5));
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/customers/{aid}/payment-methods"), None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "PUT", &format!("/api/v1/admin/customers/{aid}/auto-pay"), Some(json!({"enabled": false})), &admin.h()).await;
    assert_eq!(r.body["auto_pay"], json!(false));
    let c = invoice_due(&app, &st, &admin, &aid, "1.00").await;
    run("2026-01-10").await;
    assert!(attempts(&st, &c).await.is_empty());
    DECLINE.store(false, Ordering::SeqCst);
}
