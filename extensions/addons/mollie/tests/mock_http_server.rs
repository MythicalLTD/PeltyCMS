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
        amount: Money::new(
            rust_decimal::Decimal::from_str("19.99").unwrap(),
            Currency::parse("EUR").unwrap(),
        ),
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
    r.register(
        pelty_addon_gateway_mollie::mollie::descriptor(),
        pelty_addon_gateway_mollie::mollie::factory,
    );
    r
}

#[tokio::test]
async fn non_loopback_http_base_is_refused() {
    let cfg = GatewayConfig {
        config: json!({"api_base": "http://example.com"}),
        secrets: [("api_key".into(), "k".into())].into(),
        ..Default::default()
    };
    assert!(reg().build("mollie", &cfg).is_err());
}
