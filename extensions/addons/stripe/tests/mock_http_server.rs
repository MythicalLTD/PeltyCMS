#![allow(dead_code, unused_imports)]
//! Real HTTP (reqwest) against a local axum mock server on 127.0.0.1; no
//! provider is ever contacted.
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::money::{Currency, Money};
use pelty_payments::*;
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<(String, String, String)>>>;

async fn serve(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

fn req() -> PaymentRequest {
    PaymentRequest {
        payment_id: "p-1".into(),
        reference: "INV-1".into(),
        amount: Money::new(rust_decimal::Decimal::from_str("19.99").unwrap(), Currency::parse("EUR").unwrap()),
        description: "Hosting".into(),
        customer_email: None,
        success_url: "https://panel.example/ok".into(),
        cancel_url: "https://panel.example/no".into(),
        webhook_url: "https://panel.example/api/v1/webhooks/x".into(),
        idempotency_key: "idem-9".into(),
    }
}

fn reg() -> GatewayRegistry {
    let mut r = GatewayRegistry::with_transport(Arc::new(ReqwestTransport::new().unwrap()));
    r.register(pelty_addon_gateway_stripe::stripe::descriptor(), pelty_addon_gateway_stripe::stripe::factory);
    r
}

#[tokio::test]
async fn stripe_checkout_and_refund_over_http() {
    let log: Log = Default::default();
    let app = Router::new()
        .route("/v1/checkout/sessions", post(|State(l): State<Log>, h: HeaderMap, b: String| async move {
            l.lock().unwrap().push(("POST".into(), h["authorization"].to_str().unwrap().into(), b));
            Json(json!({"id": "cs_t1", "url": "https://checkout.example/cs_t1"}))
        }))
        .route("/v1/checkout/sessions/{id}", get(|Path(id): Path<String>| async move {
            Json(json!({"id": id, "payment_intent": "pi_t1", "status": "complete", "payment_status": "paid"}))
        }))
        .route("/v1/refunds", post(|State(l): State<Log>, h: HeaderMap, b: String| async move {
            l.lock().unwrap().push(("REFUND".into(), h["idempotency-key"].to_str().unwrap().into(), b));
            Json(json!({"id": "re_t1", "status": "succeeded"}))
        }))
        .with_state(log.clone());
    let base = serve(app).await;
    let cfg = GatewayConfig {
        config: json!({"api_base": format!("{base}/v1")}),
        secrets: [("secret_key".into(), "sk_test_local".into()), ("webhook_secret".into(), "whsec".into())].into(),
        ..Default::default()
    };
    let gw = reg().build("stripe", &cfg).unwrap();
    let init = gw.create_payment(&req()).await.unwrap();
    assert_eq!(init, PaymentInit::Redirect { url: "https://checkout.example/cs_t1".into(), provider_ref: "cs_t1".into() });
    assert_eq!(gw.complete_payment("cs_t1", &Default::default()).await.unwrap(), CompleteStatus::Succeeded);
    let r = gw.refund("cs_t1", Money::new(rust_decimal::Decimal::new(500, 2), Currency::parse("EUR").unwrap()), "rf-1").await.unwrap();
    assert_eq!(r.status, "succeeded");
    let l = log.lock().unwrap().clone();
    assert_eq!(l[0].1, "Bearer sk_test_local");
    assert!(l[0].2.contains("unit_amount%5D=1999"));
    assert_eq!((l[1].1.as_str(), l[1].2.as_str()), ("rf-1", "payment_intent=pi_t1&amount=500"));
}

