//! GoCardless Direct Debit (API version 2015-07-06).
//!
//! Checkout creates a Billing Request with a payment request *and* a mandate
//! request, then a Billing Request Flow whose `authorisation_url` the payer
//! is redirected to. The payment is collected days later, so the invoice is
//! settled by the `payments.confirmed` webhook, not on return.
//!
//! Webhooks: `Webhook-Signature` = hex(HMAC-SHA256(body, webhook_secret)).
//! One delivery may carry several events; the most significant payment
//! event is reported (failure/chargeback > confirmation > others), and every
//! event id is folded into the dedupe key so a redelivery is idempotent.
//!
//! Recurring: `save_method(mandate_id)` checks the mandate is usable and
//! `charge_saved` creates a payment against it (always `Pending`).
//! Verified with mocked HTTP only, never against GoCardless.
use pelty_payments::http::{api_base, ct_eq, hmac_sha256, send_json, url_encode};
use pelty_payments::*;
#[allow(unused_imports)]
use {pelty_core::money::Money, serde::{Deserialize, Serialize}, std::collections::{BTreeMap, HashMap}, std::future::Future, std::pin::Pin, std::sync::Arc};

pub const API_BASE: &str = "https://api.gocardless.com";
pub const SANDBOX_BASE: &str = "https://api-sandbox.gocardless.com";
const VERSION: &str = "2015-07-06";

pub struct GoCardlessGateway {
    token: String,
    webhook_secret: String,
    base: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[SettingField::of("sandbox", "bool"), SettingField::of("api_base", "url")];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "gocardless",
        name: "GoCardless",
        capabilities: Capabilities { checkout: true, webhooks: true, refunds: true, recurring: true },
        secrets: &["access_token", "webhook_secret"],
        settings: SETTINGS,
        currencies: &["AUD", "CAD", "DKK", "EUR", "GBP", "NZD", "SEK", "USD"],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    let default = if cfg.bool("sandbox") { SANDBOX_BASE } else { API_BASE };
    Ok(Box::new(GoCardlessGateway {
        token: cfg.secret("access_token")?.into(),
        webhook_secret: cfg.secret("webhook_secret")?.into(),
        base: api_base(&cfg.config, default)?,
        t: cfg.transport.clone(),
    }))
}

/// Webhook event → (kind, precedence). Higher precedence wins in a batch.
pub fn classify(resource: &str, action: &str) -> (WebhookKind, u8) {
    match (resource, action) {
        ("payments", "charged_back" | "late_failure_settled") => (WebhookKind::Disputed, 4),
        ("payments", "failed" | "cancelled") => (WebhookKind::PaymentFailed, 3),
        ("refunds", "paid" | "refund_settled") => (WebhookKind::Refunded, 3),
        ("payments", "confirmed" | "paid_out") => (WebhookKind::PaymentSucceeded, 2),
        _ => (WebhookKind::Ignored, 0),
    }
}

impl GoCardlessGateway {
    fn spec(&self, method: &'static str, path: &str, body: Option<serde_json::Value>, idem: Option<&str>) -> HttpRequestSpec {
        let mut headers = vec![("Authorization".into(), format!("Bearer {}", self.token)), ("GoCardless-Version".into(), VERSION.into()), ("Accept".into(), "application/json".into())];
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        if let Some(k) = idem {
            headers.push(("Idempotency-Key".into(), k.into()));
        }
        HttpRequestSpec { method, url: format!("{}{path}", self.base), headers, body: body.map(|b| b.to_string()).unwrap_or_default() }
    }
    pub fn verify(&self, h: &Headers, body: &[u8]) -> Result<(), GatewayError> {
        let sig = header(h, "webhook-signature").ok_or(GatewayError::InvalidSignature)?;
        let want = hmac_sha256(self.webhook_secret.as_bytes(), body);
        match hex::decode(sig.trim()) {
            Ok(b) if ct_eq(&b, &want) => Ok(()),
            _ => Err(GatewayError::InvalidSignature),
        }
    }
    /// Billing request id → the payment it created (once fulfilled).
    async fn payment_of(&self, r: &str) -> Result<String, GatewayError> {
        if r.starts_with("PM") {
            return Ok(r.to_string());
        }
        let v = send_json(self.t.as_ref(), self.spec("GET", &format!("/billing_requests/{}", url_encode(r)), None, None)).await?;
        obj_str(&v, "billing_requests.links.payment_request_payment").ok_or_else(|| GatewayError::Provider("billing request has no payment yet".into()))
    }
}

impl PaymentGateway for GoCardlessGateway {
    fn id(&self) -> &'static str {
        "gocardless"
    }
    fn display_name(&self) -> &'static str {
        "GoCardless"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(&'a self, r: &'a PaymentRequest) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let meta = serde_json::json!({"pelty_payment_id": r.payment_id});
            let br = serde_json::json!({"billing_requests": {
                "payment_request": {"amount": minor_units(r.amount)?, "currency": r.amount.currency.as_str(), "description": r.description, "metadata": meta},
                "mandate_request": {"metadata": meta},
                "metadata": {"pelty_payment_id": r.payment_id, "reference": r.reference},
            }});
            let v = send_json(self.t.as_ref(), self.spec("POST", "/billing_requests", Some(br), Some(&r.idempotency_key))).await?;
            let id = obj_str(&v, "billing_requests.id").ok_or_else(|| GatewayError::Provider("missing billing request id".into()))?;
            let mut flow = serde_json::json!({"billing_request_flows": {"redirect_uri": r.success_url, "exit_uri": r.cancel_url, "links": {"billing_request": id}}});
            if let Some(e) = &r.customer_email {
                flow["billing_request_flows"]["prefilled_customer"] = serde_json::json!({"email": e});
            }
            let f = send_json(self.t.as_ref(), self.spec("POST", "/billing_request_flows", Some(flow), Some(&format!("{}-flow", r.idempotency_key)))).await?;
            let url = obj_str(&f, "billing_request_flows.authorisation_url").ok_or_else(|| GatewayError::Provider("missing authorisation_url".into()))?;
            Ok(PaymentInit::Redirect { url, provider_ref: id })
        })
    }
    fn parse_webhook(&self, h: &Headers, body: &[u8], _now: i64) -> Result<WebhookEvent, GatewayError> {
        self.verify(h, body)?;
        let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let events = v["events"].as_array().filter(|a| !a.is_empty()).ok_or(GatewayError::MalformedPayload)?;
        let mut ids: Vec<&str> = events.iter().filter_map(|e| e["id"].as_str()).collect();
        if ids.len() != events.len() {
            return Err(GatewayError::MalformedPayload);
        }
        ids.sort_unstable();
        let best = events
            .iter()
            .map(|e| (e, classify(e["resource_type"].as_str().unwrap_or(""), e["action"].as_str().unwrap_or(""))))
            .max_by_key(|(_, (_, p))| *p)
            .ok_or(GatewayError::MalformedPayload)?;
        let (e, (kind, _)) = best;
        let ev_type = format!("{}.{}", e["resource_type"].as_str().unwrap_or(""), e["action"].as_str().unwrap_or(""));
        let r = obj_str(e, "links.payment").or_else(|| obj_str(e, "links.refund"));
        let mut out = WebhookEvent::new(ids.join(","), ev_type, kind, r);
        out.payment_id = obj_str(e, "resource_metadata.pelty_payment_id");
        Ok(out)
    }
    fn complete_payment<'a>(&'a self, provider_ref: &'a str, _q: &'a HashMap<String, String>) -> BoxFuture<'a, Result<CompleteStatus, GatewayError>> {
        Box::pin(async move {
            let v = send_json(self.t.as_ref(), self.spec("GET", &format!("/billing_requests/{}", url_encode(provider_ref)), None, None)).await?;
            // Direct Debit settles days later; a fulfilled request is still pending.
            Ok(match v["billing_requests"]["status"].as_str() {
                Some("cancelled") => CompleteStatus::Failed,
                _ => CompleteStatus::Pending,
            })
        })
    }
    fn refund<'a>(&'a self, r: &'a str, amount: Money, idem: &'a str) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            let pm = self.payment_of(r).await?;
            let body = serde_json::json!({"refunds": {"amount": minor_units(amount)?, "total_amount_confirmation": minor_units(amount)?, "reference": idem.chars().take(18).collect::<String>(), "links": {"payment": pm}, "metadata": {"idempotency_key": idem.chars().take(50).collect::<String>()}}});
            let v = send_json(self.t.as_ref(), self.spec("POST", "/refunds", Some(body), Some(idem))).await?;
            let status = match v["refunds"]["status"].as_str() {
                Some("paid") | Some("refund_settled") => "succeeded",
                Some("failed") | Some("cancelled") => "failed",
                _ => "pending",
            };
            Ok(RefundResult { provider_ref: obj_str(&v, "refunds.id"), status: status.into() })
        })
    }
    fn save_method<'a>(&'a self, token: &'a str, _email: &'a str, _idem: &'a str) -> BoxFuture<'a, Result<SavedMethod, GatewayError>> {
        Box::pin(async move {
            if !token.starts_with("MD") {
                return Err(GatewayError::Provider("expected a mandate id (MD…)".into()));
            }
            let v = send_json(self.t.as_ref(), self.spec("GET", &format!("/mandates/{}", url_encode(token)), None, None)).await?;
            let m = &v["mandates"];
            match m["status"].as_str() {
                Some("pending_customer_approval" | "pending_submission" | "submitted" | "active") => {}
                _ => return Err(GatewayError::Provider("mandate is not usable".into())),
            }
            Ok(SavedMethod { token: token.into(), brand: m["scheme"].as_str().unwrap_or("direct_debit").into(), last4: String::new(), exp_month: 0, exp_year: 0 })
        })
    }
    fn charge_saved<'a>(&'a self, token: &'a str, r: &'a PaymentRequest) -> BoxFuture<'a, Result<ChargeOutcome, GatewayError>> {
        Box::pin(async move {
            let body = serde_json::json!({"payments": {"amount": minor_units(r.amount)?, "currency": r.amount.currency.as_str(), "description": r.description, "metadata": {"pelty_payment_id": r.payment_id}, "links": {"mandate": token}}});
            let (status, raw) = self.t.send(self.spec("POST", "/payments", Some(body), Some(&r.idempotency_key))).await.map_err(GatewayError::Provider)?;
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
            if status == 422 || status == 409 {
                let reason = v["error"]["errors"][0]["reason"].as_str().or(v["error"]["message"].as_str()).unwrap_or("declined");
                return Ok(ChargeOutcome::Declined { reason: reason.chars().take(120).collect() });
            }
            if !(200..300).contains(&status) {
                return Err(GatewayError::Provider(format!("HTTP {status}")));
            }
            let id = obj_str(&v, "payments.id").ok_or_else(|| GatewayError::Provider("missing payment id".into()))?;
            Ok(ChargeOutcome::Pending { provider_ref: id })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_payments::test_util::*;

    fn g(t: &MockTransport, sandbox: bool) -> Box<dyn PaymentGateway> {
        factory(&cfg("gocardless", serde_json::json!({"sandbox": sandbox}), &[("access_token", "sandbox_tok"), ("webhook_secret", "whs")], t)).unwrap()
    }

    #[tokio::test]
    async fn billing_request_flow() {
        let t = MockTransport::new();
        t.push(201, r#"{"billing_requests":{"id":"BRQ1","status":"pending"}}"#).push(201, r#"{"billing_request_flows":{"id":"BRF1","authorisation_url":"https://pay-sandbox.gocardless.com/flow/BRF1"}}"#);
        let gw = g(&t, true);
        assert_eq!(gw.create_payment(&req("12.34", "EUR")).await.unwrap(), PaymentInit::Redirect { url: "https://pay-sandbox.gocardless.com/flow/BRF1".into(), provider_ref: "BRQ1".into() });
        let rq = t.requests();
        assert_eq!(rq[0].url, "https://api-sandbox.gocardless.com/billing_requests");
        assert_eq!(rq[0].header("GoCardless-Version"), Some("2015-07-06"));
        assert_eq!(rq[0].header("Idempotency-Key"), Some("idem-1"));
        let b: serde_json::Value = serde_json::from_str(&rq[0].body).unwrap();
        assert_eq!(b["billing_requests"]["payment_request"]["amount"], 1234);
        assert_eq!(b["billing_requests"]["payment_request"]["currency"], "EUR");
        assert!(b["billing_requests"]["mandate_request"].is_object());
        let f: serde_json::Value = serde_json::from_str(&rq[1].body).unwrap();
        assert_eq!(f["billing_request_flows"]["links"]["billing_request"], "BRQ1");
        assert_eq!(f["billing_request_flows"]["prefilled_customer"]["email"], "a@example.test");
        // Live base when sandbox is off.
        let t2 = MockTransport::new();
        t2.push(201, r#"{"billing_requests":{"id":"BRQ2"}}"#).push(201, r#"{"billing_request_flows":{"authorisation_url":"https://pay.gocardless.com/x"}}"#);
        g(&t2, false).create_payment(&req("1", "GBP")).await.unwrap();
        assert!(t2.requests()[0].url.starts_with("https://api.gocardless.com/"));
        // Return: fulfilled is still pending (DD clears later); cancelled fails.
        t.push(200, r#"{"billing_requests":{"status":"fulfilled"}}"#).push(200, r#"{"billing_requests":{"status":"cancelled"}}"#);
        assert_eq!(gw.complete_payment("BRQ1", &HashMap::new()).await.unwrap(), CompleteStatus::Pending);
        assert_eq!(gw.complete_payment("BRQ1", &HashMap::new()).await.unwrap(), CompleteStatus::Failed);
    }

    // Signatures computed independently:
    // python3 -c 'import hmac,hashlib,sys;print(hmac.new(b"whs",open(f).read().encode(),hashlib.sha256).hexdigest())'
    pub const BATCH: &str = r#"{"events":[{"id":"EV2","resource_type":"mandates","action":"active","links":{"mandate":"MD1"}},{"id":"EV1","resource_type":"payments","action":"confirmed","links":{"payment":"PM1"},"resource_metadata":{"pelty_payment_id":"pay_1"}}]}"#;
    pub const BATCH_SIG: &str = "65c85a327fae318460b942de378c16b0ff55ac3f271250c6a803cad4abb0f538";
    pub const FAIL: &str = r#"{"events":[{"id":"EV3","resource_type":"payments","action":"confirmed","links":{"payment":"PM1"}},{"id":"EV4","resource_type":"payments","action":"charged_back","links":{"payment":"PM1"}}]}"#;
    pub const FAIL_SIG: &str = "d47a8708b3dcda5aa7d733b685dcebf094776e242da59a78421cce5dfc3915b4";

    #[test]
    fn webhooks() {
        let t = MockTransport::new();
        let gw = g(&t, true);
        let e = gw.parse_webhook(&hdrs(&[("webhook-signature", BATCH_SIG)]), BATCH.as_bytes(), 0).unwrap();
        assert_eq!((e.kind.clone(), e.provider_ref.as_deref(), e.payment_id.as_deref(), e.event_id.as_str(), e.event_type.as_str()), (WebhookKind::PaymentSucceeded, Some("PM1"), Some("pay_1"), "EV1,EV2", "payments.confirmed"));
        let e = gw.parse_webhook(&hdrs(&[("webhook-signature", FAIL_SIG)]), FAIL.as_bytes(), 0).unwrap();
        assert_eq!(e.kind, WebhookKind::Disputed);
        assert_eq!(gw.parse_webhook(&hdrs(&[("webhook-signature", BATCH_SIG)]), BATCH.replace("PM1", "PM9").as_bytes(), 0).unwrap_err(), GatewayError::InvalidSignature);
        assert_eq!(gw.parse_webhook(&hdrs(&[]), BATCH.as_bytes(), 0).unwrap_err(), GatewayError::InvalidSignature);
        assert_eq!(classify("mandates", "cancelled").0, WebhookKind::Ignored);
        assert_eq!(classify("payments", "failed").0, WebhookKind::PaymentFailed);
    }

    #[tokio::test]
    async fn refund_and_mandate_charges() {
        let t = MockTransport::new();
        let gw = g(&t, true);
        t.push(200, r#"{"billing_requests":{"links":{"payment_request_payment":"PM7"}}}"#).push(201, r#"{"refunds":{"id":"RF1","status":"created"}}"#);
        let r = gw.refund("BRQ1", money("5.00", "EUR"), "refund-key").await.unwrap();
        assert_eq!((r.status.as_str(), r.provider_ref.as_deref()), ("pending", Some("RF1")));
        let rq = t.requests();
        let b: serde_json::Value = serde_json::from_str(&rq[1].body).unwrap();
        assert_eq!((b["refunds"]["amount"].as_i64(), b["refunds"]["links"]["payment"].as_str()), (Some(500), Some("PM7")));

        assert!(gw.save_method("pm_card", "a@x", "k").await.is_err());
        t.push(200, r#"{"mandates":{"id":"MD1","status":"active","scheme":"sepa_core"}}"#);
        assert_eq!(gw.save_method("MD1", "a@x", "k").await.unwrap().brand, "sepa_core");
        t.push(200, r#"{"mandates":{"id":"MD2","status":"cancelled"}}"#);
        assert!(gw.save_method("MD2", "a@x", "k").await.is_err());

        t.push(201, r#"{"payments":{"id":"PM9","status":"pending_submission"}}"#);
        assert_eq!(gw.charge_saved("MD1", &req("9.99", "EUR")).await.unwrap(), ChargeOutcome::Pending { provider_ref: "PM9".into() });
        let b: serde_json::Value = serde_json::from_str(&t.requests().last().unwrap().body).unwrap();
        assert_eq!((b["payments"]["amount"].as_i64(), b["payments"]["links"]["mandate"].as_str()), (Some(999), Some("MD1")));
        t.push(422, r#"{"error":{"message":"Mandate is cancelled","errors":[{"reason":"mandate_is_inactive"}]}}"#);
        assert_eq!(gw.charge_saved("MD1", &req("9.99", "EUR")).await.unwrap(), ChargeOutcome::Declined { reason: "mandate_is_inactive".into() });
        t.push(500, "{}");
        assert!(gw.charge_saved("MD1", &req("9.99", "EUR")).await.is_err());
    }
}
