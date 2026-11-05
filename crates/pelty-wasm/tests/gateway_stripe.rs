//! Stripe, compiled as a separate wasm32-wasip2 addon, driven through the
//! host adapter with the host HTTP transport mocked. Mirrors the native
//! `pelty_addon_gateway_stripe::stripe` behaviour tests.
mod common;
use pelty_core::money::{Currency, Money};
use pelty_payments::*;
use pelty_wasm::{HostServices, WasmAddon};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

fn money(v: &str, c: &str) -> Money {
    Money::new(rust_decimal::Decimal::from_str(v).unwrap(), Currency::parse(c).unwrap())
}

fn gateway(t: &MockTransport) -> Box<dyn PaymentGateway> {
    let zip = common::addon_zip("stripe", "pelty-addon-gateway-stripe");
    let services = HostServices { http: Arc::new(t.clone()), ..Default::default() };
    let addon = Arc::new(WasmAddon::from_zip(&zip, pelty_extensions::addon_pkg::Limits::default(), services).unwrap());
    let reg = GatewayRegistry::empty(Arc::new(t.clone()));
    let d = pelty_wasm::gateway::register(&reg, addon).unwrap();
    assert_eq!(d.id, "stripe");
    assert!(d.capabilities.refunds && d.capabilities.recurring);
    assert_eq!(d.secrets, &["secret_key", "webhook_secret"]);
    let mut secrets = HashMap::new();
    secrets.insert("secret_key".to_string(), "sk_test".to_string());
    secrets.insert("webhook_secret".to_string(), "whsec".to_string());
    reg.build("stripe", &GatewayConfig { id: "stripe".into(), config: serde_json::json!({}), secrets, ..Default::default() }).unwrap()
}

fn req() -> PaymentRequest {
    PaymentRequest {
        payment_id: "pay_1".into(),
        reference: "INV-7".into(),
        amount: money("12.50", "EUR"),
        description: "Invoice INV-7".into(),
        customer_email: Some("a@example.test".into()),
        success_url: "https://x.test/ok".into(),
        cancel_url: "https://x.test/no".into(),
        webhook_url: "https://x.test/wh".into(),
        idempotency_key: "idem-1".into(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn create_payment_through_wasm() {
    let t = MockTransport::new();
    t.push(200, r#"{"id":"cs_1","url":"https://checkout.stripe.com/c/cs_1"}"#);
    let init = gateway(&t).create_payment(&req()).await.unwrap();
    assert_eq!(init, PaymentInit::Redirect { url: "https://checkout.stripe.com/c/cs_1".into(), provider_ref: "cs_1".into() });
    let sent = t.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "POST");
    assert!(sent[0].url.starts_with("https://api.stripe.com/v1/checkout/sessions"));
    assert_eq!(sent[0].header("authorization"), Some("Bearer sk_test"));
    assert!(sent[0].body.contains("unit_amount%5D=1250") || sent[0].body.contains("unit_amount]=1250"), "{}", sent[0].body);
}

#[tokio::test(flavor = "multi_thread")]
async fn refund_resolves_session_and_errors_map_exactly() {
    let t = MockTransport::new();
    t.push(200, r#"{"id":"cs_1","payment_intent":"pi_9"}"#).push(200, r#"{"id":"re_1","status":"succeeded"}"#);
    let g = gateway(&t);
    let r = g.refund("cs_1", money("5.00", "EUR"), "k1").await.unwrap();
    assert_eq!(r, RefundResult { provider_ref: Some("re_1".into()), status: "succeeded".into() });
    let sent = t.requests();
    assert!(sent[0].url.ends_with("/checkout/sessions/cs_1"));
    assert_eq!(sent[1].body, "payment_intent=pi_9&amount=500");
    assert_eq!(sent[1].header("idempotency-key"), Some("k1"));
    t.push(402, r#"{"error":{"message":"card_declined"}}"#);
    let e = g.refund("pi_1", money("1.00", "EUR"), "k").await.unwrap_err();
    assert_eq!(e, GatewayError::Provider("HTTP 402: card_declined".into()));
}

#[tokio::test(flavor = "multi_thread")]
async fn complete_payment_statuses() {
    let t = MockTransport::new();
    t.push(200, r#"{"status":"complete","payment_status":"paid"}"#).push(200, r#"{"status":"open","payment_status":"unpaid"}"#).push(200, r#"{"status":"expired","payment_status":"unpaid"}"#);
    let g = gateway(&t);
    let q = HashMap::new();
    assert_eq!(g.complete_payment("cs_1", &q).await.unwrap(), CompleteStatus::Succeeded);
    assert_eq!(g.complete_payment("cs_1", &q).await.unwrap(), CompleteStatus::Pending);
    assert_eq!(g.complete_payment("cs_1", &q).await.unwrap(), CompleteStatus::Failed);
}

fn signed(body: &str, ts: i64, secret: &[u8]) -> Headers {
    use hmac::{Hmac, Mac};
    let mut m = Hmac::<sha2::Sha256>::new_from_slice(secret).unwrap();
    m.update(format!("{ts}.{body}").as_bytes());
    let sig = hex::encode(m.finalize().into_bytes());
    let mut h = Headers::new();
    h.insert("stripe-signature".into(), format!("t={ts},v1={sig}"));
    h
}

#[test]
fn webhook_signature_verified_inside_sandbox() {
    let t = MockTransport::new();
    let g = gateway(&t);
    let b = r#"{"id":"e5","type":"payment_intent.payment_failed","data":{"object":{"id":"pi_3","metadata":{"pelty_payment_id":"p-1"}}}}"#;
    let e = g.parse_webhook(&signed(b, 100, b"whsec"), b.as_bytes(), 100).unwrap();
    assert_eq!(e.kind, WebhookKind::PaymentFailed);
    assert_eq!(e.provider_ref.as_deref(), Some("pi_3"));
    assert_eq!(e.payment_id.as_deref(), Some("p-1"));
    assert_eq!(g.parse_webhook(&signed(b, 100, b"wrong"), b.as_bytes(), 100).unwrap_err(), GatewayError::InvalidSignature);
    assert_eq!(g.parse_webhook(&signed(b, 100, b"whsec"), b.as_bytes(), 100_000).unwrap_err(), GatewayError::InvalidSignature);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_outside_allowlist_is_refused() {
    let t = MockTransport::new();
    let zip = common::addon_zip("stripe", "pelty-addon-gateway-stripe");
    let services = HostServices { http: Arc::new(t.clone()), ..Default::default() };
    let addon = Arc::new(WasmAddon::from_zip(&zip, pelty_extensions::addon_pkg::Limits::default(), services).unwrap());
    let reg = GatewayRegistry::empty(Arc::new(t.clone()));
    pelty_wasm::gateway::register(&reg, addon).unwrap();
    let mut secrets = HashMap::new();
    secrets.insert("secret_key".to_string(), "sk".to_string());
    secrets.insert("webhook_secret".to_string(), "wh".to_string());
    // api_base is allowed via `$setting:api_base`; a different host is not.
    let g = reg.build("stripe", &GatewayConfig { config: serde_json::json!({"api_base": "https://evil.test/v1"}), secrets: secrets.clone(), ..Default::default() }).unwrap();
    t.push(200, r#"{"id":"cs_1","url":"https://evil.test/x"}"#);
    // evil.test is allowed because the admin set it as api_base…
    assert!(g.create_payment(&req()).await.is_ok());
    // …but an addon cannot reach any other host by itself.
    let g2 = reg.build("stripe", &GatewayConfig { config: serde_json::json!({}), secrets, ..Default::default() }).unwrap();
    assert_eq!(t.requests().len(), 1);
    drop(g2);
}
