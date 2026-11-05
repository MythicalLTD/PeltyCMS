//! Mollie (Payments API v2). Mollie webhooks are unsigned and carry only
//! `id=tr_…`; [`handle_webhook`](PaymentGateway::handle_webhook) therefore
//! fetches the payment from the API and trusts only that response.
//! Verified with mocked HTTP only, never against Mollie.
use pelty_payments::http::{api_base, form_decode, send_json, url_encode};
use pelty_payments::*;
#[allow(unused_imports)]
use {pelty_core::money::Money, serde::{Deserialize, Serialize}, std::collections::{BTreeMap, HashMap}, std::future::Future, std::pin::Pin, std::sync::Arc};

pub const API_BASE: &str = "https://api.mollie.com/v2";

pub struct MollieGateway {
    key: String,
    base: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[SettingField::of("api_base", "url")];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "mollie",
        name: "Mollie",
        capabilities: Capabilities { checkout: true, webhooks: true, refunds: true, recurring: false },
        secrets: &["api_key"],
        settings: SETTINGS,
        currencies: &["AUD", "BGN", "BRL", "CAD", "CHF", "CZK", "DKK", "EUR", "GBP", "HKD", "HUF", "ILS", "ISK", "JPY", "MXN", "MYR", "NOK", "NZD", "PHP", "PLN", "RON", "SEK", "SGD", "THB", "TWD", "USD", "ZAR"],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    Ok(Box::new(MollieGateway {
        key: cfg.secret("api_key")?.into(),
        base: api_base(&cfg.config, API_BASE)?,
        t: cfg.transport.clone(),
    }))
}

impl MollieGateway {
    fn spec(&self, method: &'static str, path: &str, body: Option<serde_json::Value>, idem: Option<&str>) -> HttpRequestSpec {
        let mut headers = vec![("Authorization".into(), format!("Bearer {}", self.key))];
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        if let Some(k) = idem {
            headers.push(("Idempotency-Key".into(), k.into()));
        }
        HttpRequestSpec { method, url: format!("{}{path}", self.base), headers, body: body.map(|b| b.to_string()).unwrap_or_default() }
    }
    async fn fetch(&self, id: &str) -> Result<serde_json::Value, GatewayError> {
        send_json(self.t.as_ref(), self.spec("GET", &format!("/payments/{}", url_encode(id)), None, None)).await
    }
}

fn status_kind(v: &serde_json::Value) -> WebhookKind {
    let refunded = v["amountRefunded"]["value"].as_str().zip(v["amount"]["value"].as_str()).is_some_and(|(r, a)| r == a);
    match v["status"].as_str() {
        Some("paid") if refunded => WebhookKind::Refunded,
        Some("paid") => {
            if v["amountChargedBack"]["value"].as_str().is_some_and(|c| c != "0.00") {
                WebhookKind::Disputed
            } else {
                WebhookKind::PaymentSucceeded
            }
        }
        Some("failed") | Some("canceled") | Some("expired") => WebhookKind::PaymentFailed,
        _ => WebhookKind::Ignored,
    }
}

impl PaymentGateway for MollieGateway {
    fn id(&self) -> &'static str {
        "mollie"
    }
    fn display_name(&self) -> &'static str {
        "Mollie"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(&'a self, r: &'a PaymentRequest) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let body = serde_json::json!({
                "amount": {"currency": r.amount.currency.as_str(), "value": decimal_str(r.amount)?},
                "description": r.description,
                "redirectUrl": r.success_url,
                "cancelUrl": r.cancel_url,
                "webhookUrl": r.webhook_url,
                "metadata": {"pelty_payment_id": r.payment_id, "reference": r.reference},
            });
            let v = send_json(self.t.as_ref(), self.spec("POST", "/payments", Some(body), Some(&r.idempotency_key))).await?;
            match (v["id"].as_str(), v["_links"]["checkout"]["href"].as_str()) {
                (Some(id), Some(url)) => Ok(PaymentInit::Redirect { url: url.into(), provider_ref: id.into() }),
                _ => Err(GatewayError::Provider("missing id/checkout link".into())),
            }
        })
    }
    fn parse_webhook(&self, _: &Headers, _: &[u8], _: i64) -> Result<WebhookEvent, GatewayError> {
        // Unsigned by design; only the async path (API fetch) is trusted.
        Err(GatewayError::Unsupported)
    }
    fn handle_webhook<'a>(&'a self, _h: &'a Headers, body: &'a [u8], _now: i64) -> BoxFuture<'a, Result<WebhookEvent, GatewayError>> {
        Box::pin(async move {
            let id = form_decode(body)
                .into_iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v)
                .filter(|v| v.starts_with("tr_"))
                .ok_or(GatewayError::MalformedPayload)?;
            let v = self.fetch(&id).await.map_err(|e| match e {
                GatewayError::Provider(m) if m.starts_with("HTTP 404") => GatewayError::InvalidSignature,
                e => e,
            })?;
            let status = v["status"].as_str().unwrap_or("unknown");
            let kind = status_kind(&v);
            let refunded = v["amountRefunded"]["value"].as_str().unwrap_or("0");
            let mut e = WebhookEvent::new(format!("{id}:{status}:{refunded}"), format!("payment.{status}"), kind, Some(id));
            e.payment_id = v["metadata"]["pelty_payment_id"].as_str().map(str::to_string);
            Ok(e)
        })
    }
    fn complete_payment<'a>(&'a self, id: &'a str, _q: &'a HashMap<String, String>) -> BoxFuture<'a, Result<CompleteStatus, GatewayError>> {
        Box::pin(async move {
            let v = self.fetch(id).await?;
            Ok(match status_kind(&v) {
                WebhookKind::PaymentSucceeded => CompleteStatus::Succeeded,
                WebhookKind::PaymentFailed => CompleteStatus::Failed,
                _ => CompleteStatus::Pending,
            })
        })
    }
    fn refund<'a>(&'a self, id: &'a str, amount: Money, idem: &'a str) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            let body = serde_json::json!({"amount": {"currency": amount.currency.as_str(), "value": decimal_str(amount)?}});
            let v = send_json(self.t.as_ref(), self.spec("POST", &format!("/payments/{}/refunds", url_encode(id)), Some(body), Some(idem))).await?;
            let status = match v["status"].as_str() {
                Some("refunded") => "succeeded",
                Some("failed") => "failed",
                Some("canceled") => "canceled",
                _ => "pending",
            };
            Ok(RefundResult { provider_ref: v["id"].as_str().map(str::to_string), status: status.into() })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_payments::test_util::*;

    fn g(t: &MockTransport) -> Box<dyn PaymentGateway> {
        factory(&cfg("mollie", serde_json::json!({}), &[("api_key", "test_abc")], t)).unwrap()
    }

    #[tokio::test]
    async fn create_webhook_refund() {
        let t = MockTransport::new();
        t.push(201, r#"{"id":"tr_1","status":"open","_links":{"checkout":{"href":"https://www.mollie.com/checkout/tr_1"}}}"#);
        let gw = g(&t);
        let init = gw.create_payment(&req("10", "EUR")).await.unwrap();
        assert_eq!(init, PaymentInit::Redirect { url: "https://www.mollie.com/checkout/tr_1".into(), provider_ref: "tr_1".into() });
        let s = &t.requests()[0];
        let v: serde_json::Value = serde_json::from_str(&s.body).unwrap();
        assert_eq!(v["amount"]["value"], "10.00");
        assert_eq!(v["webhookUrl"], "https://panel.example/api/v1/webhooks/x");
        assert_eq!(s.header("authorization"), Some("Bearer test_abc"));

        t.push(200, r#"{"id":"tr_1","status":"paid","amount":{"value":"10.00"},"amountRefunded":{"value":"0.00"},"metadata":{"pelty_payment_id":"p1"}}"#);
        let e = gw.handle_webhook(&Headers::new(), b"id=tr_1", 0).await.unwrap();
        assert_eq!((e.kind, e.provider_ref.as_deref(), e.payment_id.as_deref()), (WebhookKind::PaymentSucceeded, Some("tr_1"), Some("p1")));
        assert_eq!(e.event_id, "tr_1:paid:0.00");

        // Unknown id (forged webhook) → treated as a bad signature.
        t.push(404, r#"{"status":404,"detail":"No payment exists"}"#);
        assert_eq!(gw.handle_webhook(&Headers::new(), b"id=tr_forged", 0).await.unwrap_err(), GatewayError::InvalidSignature);
        assert_eq!(gw.handle_webhook(&Headers::new(), b"foo=bar", 0).await.unwrap_err(), GatewayError::MalformedPayload);

        t.push(201, r#"{"id":"re_1","status":"pending"}"#);
        let r = gw.refund("tr_1", money("4.00", "EUR"), "rk").await.unwrap();
        assert_eq!(r.status, "pending");
        assert!(t.requests().last().unwrap().url.ends_with("/payments/tr_1/refunds"));

        t.push(200, r#"{"id":"tr_1","status":"paid","amount":{"value":"10.00"},"amountRefunded":{"value":"10.00"}}"#);
        assert_eq!(gw.handle_webhook(&Headers::new(), b"id=tr_1", 0).await.unwrap().kind, WebhookKind::Refunded);
    }
}
