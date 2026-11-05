//! Stripe gateway: Checkout Sessions (redirect) with PaymentIntent
//! resolution for refunds and return-time status checks, signed webhooks for
//! payment, refund and dispute events.
//!
//! Verified here: `Stripe-Signature` verification against a vector computed
//! independently, event mapping, request building, and the full HTTP flow
//! against in-memory and local mock HTTP servers. NOT verified against Stripe.
use pelty_payments::http::{api_base, form_encode, send_json};
use pelty_payments::*;
#[allow(unused_imports)]
use {pelty_core::money::Money, serde::{Deserialize, Serialize}, std::collections::{BTreeMap, HashMap}, std::future::Future, std::pin::Pin, std::sync::Arc};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub use pelty_payments::http::HttpRequestSpec;
/// Kept for compatibility: Stripe's transport is the shared HTTP transport.
pub use pelty_payments::http::HttpTransport as StripeTransport;

pub const API_BASE: &str = "https://api.stripe.com/v1";
pub const DEFAULT_TOLERANCE_SECS: i64 = 300;

pub struct StripeGateway {
    secret_key: String,
    webhook_secret: String,
    tolerance: i64,
    base: String,
    transport: Arc<dyn HttpTransport>,
    /// Dynamic payment methods: a Dashboard Payment Method Configuration
    /// (`pmc_…`). Empty = Stripe's default configuration.
    pmc: Option<String>,
    /// Explicit legacy override (`card,sepa_debit`). When set, dynamic
    /// payment methods are disabled for this gateway.
    pm_types: Vec<String>,
}

const SETTINGS: &[SettingField] = &[
    SettingField::of("api_base", "url"),
    SettingField::text("payment_method_configuration", false),
    SettingField::text("payment_method_types", false),
];

/// Parse the dynamic payment method settings.
pub fn dynamic_settings(cfg: &serde_json::Value) -> Result<(Option<String>, Vec<String>), GatewayError> {
    let pmc = cfg.get("payment_method_configuration").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    if let Some(p) = pmc {
        if !p.starts_with("pmc_") || p.len() > 64 || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(GatewayError::Config("payment_method_configuration must be a pmc_… id".into()));
        }
    }
    let mut types = Vec::new();
    for t in cfg.get("payment_method_types").and_then(|v| v.as_str()).unwrap_or("").split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if t.len() > 40 || !t.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return Err(GatewayError::Config("invalid payment_method_types".into()));
        }
        types.push(t.to_string());
    }
    if pmc.is_some() && !types.is_empty() {
        return Err(GatewayError::Config("set either payment_method_configuration or payment_method_types".into()));
    }
    Ok((pmc.map(String::from), types))
}

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "stripe",
        name: "Stripe",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: true,
            recurring: true,
        },
        secrets: &["secret_key", "webhook_secret"],
        settings: SETTINGS,
        currencies: &[],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    let mut g = StripeGateway::with_transport(
        cfg.secret("secret_key")?,
        cfg.secret("webhook_secret")?,
        cfg.transport.clone(),
    );
    g.base = api_base(&cfg.config, API_BASE)?;
    (g.pmc, g.pm_types) = dynamic_settings(&cfg.config)?;
    Ok(Box::new(g))
}

impl StripeGateway {
    pub fn new(
        secret_key: &str,
        webhook_secret: &str,
        transport: Box<dyn StripeTransport>,
    ) -> Self {
        Self::with_transport(secret_key, webhook_secret, Arc::from(transport))
    }

    pub fn with_transport(secret_key: &str, webhook_secret: &str, transport: Arc<dyn HttpTransport>) -> Self {
        StripeGateway {
            secret_key: secret_key.into(),
            webhook_secret: webhook_secret.into(),
            tolerance: DEFAULT_TOLERANCE_SECS,
            base: API_BASE.into(),
            transport,
            pmc: None,
            pm_types: Vec::new(),
        }
    }

    fn auth_headers(&self, idempotency_key: Option<&str>) -> Vec<(String, String)> {
        let mut h = vec![
            ("Authorization".into(), format!("Bearer {}", self.secret_key)),
            ("Content-Type".into(), "application/x-www-form-urlencoded".into()),
        ];
        if let Some(k) = idempotency_key {
            h.push(("Idempotency-Key".into(), k.into()));
        }
        h
    }

    /// Dynamic payment methods: omit `payment_method_types` so Stripe picks
    /// eligible methods (cards, wallets, SEPA, iDEAL…) from the Dashboard,
    /// optionally pinned to a Payment Method Configuration.
    pub fn with_methods(mut self, pmc: Option<String>, types: Vec<String>) -> Self {
        self.pmc = pmc;
        self.pm_types = types;
        self
    }

    fn push_methods(&self, p: &mut Vec<(&'static str, String)>) {
        if let Some(c) = &self.pmc {
            p.push(("payment_method_configuration", c.clone()));
        }
        for t in &self.pm_types {
            p.push(("payment_method_types[]", t.clone()));
        }
    }

    pub fn checkout_request(&self, r: &PaymentRequest) -> Result<HttpRequestSpec, GatewayError> {
        let minor = minor_units(r.amount)?;
        let mut p = vec![
            ("mode", "payment".to_string()),
            ("success_url", r.success_url.clone()),
            ("cancel_url", r.cancel_url.clone()),
            ("client_reference_id", r.payment_id.clone()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", r.amount.currency.as_str().to_ascii_lowercase()),
            ("line_items[0][price_data][unit_amount]", minor.to_string()),
            ("line_items[0][price_data][product_data][name]", r.description.clone()),
            ("metadata[pelty_payment_id]", r.payment_id.clone()),
            ("payment_intent_data[metadata][pelty_payment_id]", r.payment_id.clone()),
        ];
        if let Some(e) = &r.customer_email {
            p.push(("customer_email", e.clone()));
        }
        self.push_methods(&mut p);
        Ok(HttpRequestSpec {
            method: "POST",
            url: format!("{}/checkout/sessions", self.base),
            headers: self.auth_headers(Some(&r.idempotency_key)),
            body: form_encode(&p),
        })
    }

    pub fn session_request(&self, id: &str) -> HttpRequestSpec {
        HttpRequestSpec {
            method: "GET",
            url: format!("{}/checkout/sessions/{}", self.base, pelty_payments::http::url_encode(id)),
            headers: self.auth_headers(None),
            body: String::new(),
        }
    }

    pub fn refund_request(
        &self,
        payment_intent: &str,
        amount: Money,
        idem: &str,
    ) -> Result<HttpRequestSpec, GatewayError> {
        let minor = minor_units(amount)?;
        Ok(HttpRequestSpec {
            method: "POST",
            url: format!("{}/refunds", self.base),
            headers: self.auth_headers(Some(idem)),
            body: form_encode(&[("payment_intent", payment_intent.into()), ("amount", minor.to_string())]),
        })
    }

    /// Checkout Session ids (`cs_…`) resolve to their PaymentIntent.
    async fn payment_intent_for(&self, provider_ref: &str) -> Result<String, GatewayError> {
        if !provider_ref.starts_with("cs_") {
            return Ok(provider_ref.to_string());
        }
        let v = send_json(self.transport.as_ref(), self.session_request(provider_ref)).await?;
        v["payment_intent"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| GatewayError::Provider("session has no payment_intent".into()))
    }

    /// Create a Stripe customer (step 1 of saving a method).
    pub fn customer_request(&self, email: &str, idem: &str) -> HttpRequestSpec {
        HttpRequestSpec {
            method: "POST",
            url: format!("{}/customers", self.base),
            headers: self.auth_headers(Some(&format!("{idem}:customer"))),
            body: form_encode(&[("email", email.into())]),
        }
    }

    /// Attach a payment method to a customer (step 2).
    pub fn attach_request(&self, pm: &str, customer: &str, idem: &str) -> Result<HttpRequestSpec, GatewayError> {
        if !pm.starts_with("pm_") || !pm[3..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || pm.len() > 255 {
            return Err(GatewayError::Config("invalid payment method token".into()));
        }
        Ok(HttpRequestSpec {
            method: "POST",
            url: format!("{}/payment_methods/{pm}/attach", self.base),
            headers: self.auth_headers(Some(&format!("{idem}:attach"))),
            body: form_encode(&[("customer", customer.into())]),
        })
    }

    /// Off-session PaymentIntent for a saved `cus_…|pm_…` token.
    pub fn charge_request(&self, token: &str, r: &PaymentRequest) -> Result<HttpRequestSpec, GatewayError> {
        let (customer, pm) = token.split_once('|').ok_or_else(|| GatewayError::Config("invalid saved token".into()))?;
        let minor = r.amount.to_minor_units().ok_or_else(|| GatewayError::Config("amount out of range".into()))?;
        Ok(HttpRequestSpec {
            method: "POST",
            url: format!("{}/payment_intents", self.base),
            headers: self.auth_headers(Some(&r.idempotency_key)),
            body: form_encode(&[
                ("amount", minor.to_string()),
                ("currency", r.amount.currency.as_str().to_ascii_lowercase()),
                ("customer", customer.into()),
                ("payment_method", pm.into()),
                ("off_session", "true".into()),
                ("confirm", "true".into()),
                ("description", r.description.clone()),
                ("metadata[pelty_payment_id]", r.payment_id.clone()),
            ]),
        })
    }

    async fn post_json(&self, spec: HttpRequestSpec) -> Result<(u16, serde_json::Value), GatewayError> {
        let (status, body) = self.transport.send(spec).await.map_err(GatewayError::Provider)?;
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| GatewayError::Provider("bad json".into()))?;
        Ok((status, v))
    }

    /// Verify a `Stripe-Signature` header.
    pub fn verify_signature(&self, header: &str, body: &[u8], now: i64) -> Result<(), GatewayError> {
        let mut ts: Option<i64> = None;
        let mut sigs = Vec::new();
        for part in header.split(',') {
            match part.trim().split_once('=') {
                Some(("t", v)) => ts = v.parse().ok(),
                Some(("v1", v)) => sigs.push(v.to_string()),
                _ => {}
            }
        }
        let ts = ts.ok_or(GatewayError::InvalidSignature)?;
        if (now - ts).abs() > self.tolerance || sigs.is_empty() {
            return Err(GatewayError::InvalidSignature);
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(self.webhook_secret.as_bytes())
            .map_err(|_| GatewayError::InvalidSignature)?;
        mac.update(ts.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        let expected = mac.finalize().into_bytes();
        if sigs.iter().any(|s| hex::decode(s).is_ok_and(|b| pelty_payments::http::ct_eq(&b, &expected))) {
            Ok(())
        } else {
            Err(GatewayError::InvalidSignature)
        }
    }
}

impl PaymentGateway for StripeGateway {
    fn id(&self) -> &'static str {
        "stripe"
    }
    fn display_name(&self) -> &'static str {
        "Stripe"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(&'a self, req: &'a PaymentRequest) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let v = send_json(self.transport.as_ref(), self.checkout_request(req)?).await?;
            match (v["url"].as_str(), v["id"].as_str()) {
                (Some(u), Some(id)) => Ok(PaymentInit::Redirect { url: u.into(), provider_ref: id.into() }),
                _ => Err(GatewayError::Provider("missing url/id".into())),
            }
        })
    }
    fn parse_webhook(&self, headers: &Headers, body: &[u8], now: i64) -> Result<WebhookEvent, GatewayError> {
        let sig = headers.get("stripe-signature").ok_or(GatewayError::InvalidSignature)?;
        self.verify_signature(sig, body, now)?;
        let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let event_id = v["id"].as_str().ok_or(GatewayError::MalformedPayload)?.to_string();
        let event_type = v["type"].as_str().ok_or(GatewayError::MalformedPayload)?.to_string();
        let obj = &v["data"]["object"];
        let s = |k: &str| obj[k].as_str().map(str::to_string);
        let (kind, provider_ref) = match event_type.as_str() {
            // Async methods complete later; only "paid" sessions are final.
            "checkout.session.completed" => (
                if obj["payment_status"].as_str().is_none_or(|p| p == "paid") {
                    WebhookKind::PaymentSucceeded
                } else {
                    WebhookKind::Ignored
                },
                s("id"),
            ),
            "checkout.session.async_payment_succeeded" => (WebhookKind::PaymentSucceeded, s("id")),
            "checkout.session.async_payment_failed" | "checkout.session.expired" => (WebhookKind::PaymentFailed, s("id")),
            "payment_intent.payment_failed" => (WebhookKind::PaymentFailed, s("id")),
            // Only a full refund flips the payment; partial refunds are
            // tracked by the refunds ledger.
            "charge.refunded" => (
                if obj["refunded"].as_bool() == Some(false) { WebhookKind::Ignored } else { WebhookKind::Refunded },
                s("payment_intent"),
            ),
            "charge.dispute.created" | "charge.dispute.funds_withdrawn" => (WebhookKind::Disputed, s("payment_intent")),
            _ => (WebhookKind::Ignored, s("id")),
        };
        let mut e = WebhookEvent::new(event_id, event_type, kind, provider_ref);
        e.payment_id = obj["metadata"]["pelty_payment_id"].as_str().map(str::to_string);
        Ok(e)
    }
    fn complete_payment<'a>(
        &'a self,
        provider_ref: &'a str,
        _q: &'a HashMap<String, String>,
    ) -> BoxFuture<'a, Result<CompleteStatus, GatewayError>> {
        Box::pin(async move {
            let v = send_json(self.transport.as_ref(), self.session_request(provider_ref)).await?;
            Ok(match (v["status"].as_str(), v["payment_status"].as_str()) {
                (_, Some("paid")) | (_, Some("no_payment_required")) => CompleteStatus::Succeeded,
                (Some("expired"), _) => CompleteStatus::Failed,
                _ => CompleteStatus::Pending,
            })
        })
    }
    fn save_method<'a>(&'a self, token: &'a str, email: &'a str, idem: &'a str) -> BoxFuture<'a, Result<SavedMethod, GatewayError>> {
        Box::pin(async move {
            let (st, c) = self.post_json(self.customer_request(email, idem)).await?;
            let customer = c["id"].as_str().filter(|_| (200..300).contains(&st)).ok_or_else(|| GatewayError::Provider(c["error"]["message"].as_str().unwrap_or("customer failed").into()))?.to_string();
            let (st, pm) = self.post_json(self.attach_request(token, &customer, idem)?).await?;
            if !(200..300).contains(&st) {
                return Err(GatewayError::Provider(pm["error"]["message"].as_str().unwrap_or("attach failed").into()));
            }
            let card = &pm["card"];
            Ok(SavedMethod {
                token: format!("{customer}|{token}"),
                brand: card["brand"].as_str().unwrap_or("card").chars().take(30).collect(),
                last4: card["last4"].as_str().unwrap_or("").chars().filter(char::is_ascii_digit).take(4).collect(),
                exp_month: card["exp_month"].as_u64().and_then(|m| u8::try_from(m).ok()).filter(|m| (1..=12).contains(m)).unwrap_or(1),
                exp_year: card["exp_year"].as_u64().and_then(|y| u16::try_from(y).ok()).unwrap_or(0),
            })
        })
    }
    fn charge_saved<'a>(&'a self, token: &'a str, req: &'a PaymentRequest) -> BoxFuture<'a, Result<ChargeOutcome, GatewayError>> {
        Box::pin(async move {
            let (st, v) = self.post_json(self.charge_request(token, req)?).await?;
            // Card errors (402) are declines; the PaymentIntent id is in the error object.
            if st == 402 || v["error"]["type"] == "card_error" {
                return Ok(ChargeOutcome::Declined {
                    reason: v["error"]["decline_code"].as_str().or(v["error"]["code"].as_str()).unwrap_or("declined").chars().take(100).collect(),
                });
            }
            if !(200..300).contains(&st) {
                return Err(GatewayError::Provider(v["error"]["message"].as_str().unwrap_or("error").into()));
            }
            let id = v["id"].as_str().unwrap_or_default().to_string();
            Ok(match v["status"].as_str() {
                Some("succeeded") => ChargeOutcome::Succeeded { provider_ref: id },
                Some("processing") => ChargeOutcome::Pending { provider_ref: id },
                Some(other) => ChargeOutcome::Declined { reason: other.into() },
                None => return Err(GatewayError::Provider("missing status".into())),
            })
        })
    }
    fn refund<'a>(&'a self, provider_ref: &'a str, amount: Money, idem: &'a str) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            let pi = self.payment_intent_for(provider_ref).await?;
            let v = send_json(self.transport.as_ref(), self.refund_request(&pi, amount, idem)?).await?;
            Ok(RefundResult {
                provider_ref: v["id"].as_str().map(str::to_string),
                status: v["status"].as_str().unwrap_or("pending").into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_core::money::{Currency, Money};
    use rust_decimal::Decimal;
    use std::sync::Mutex;

    // Vector computed independently with Python's hmac module:
    // hmac.new(b"whsec_test_secret_123", b"1700000000." + BODY, sha256).hexdigest()
    const SECRET: &str = "whsec_test_secret_123";
    const BODY: &str = r#"{"id":"evt_1","type":"checkout.session.completed","data":{"object":{"id":"cs_test_1"}}}"#;
    const SIG: &str = "727bc6389e18c07af27cce48a404505a0c51fac6fab3bf805808a022ac556d1a";
    const T: i64 = 1_700_000_000;

    fn gw() -> StripeGateway {
        StripeGateway::new("sk_test_x", SECRET, Box::new(NoTransport))
    }

    #[test]
    fn known_vector_passes() {
        assert_eq!(
            gw().verify_signature(&format!("t={T},v1={SIG}"), BODY.as_bytes(), T + 10),
            Ok(())
        );
        // Multiple v1 (secret rotation) and v0 entries are tolerated.
        assert_eq!(
            gw().verify_signature(
                &format!("t={T},v1=deadbeef,v0=abc,v1={SIG}"),
                BODY.as_bytes(),
                T
            ),
            Ok(())
        );
    }

    #[test]
    fn tampering_and_replay_rejected() {
        let g = gw();
        let h = format!("t={T},v1={SIG}");
        assert!(g
            .verify_signature(&h, BODY.replace("cs_test_1", "cs_x").as_bytes(), T)
            .is_err());
        assert!(
            g.verify_signature(&h, BODY.as_bytes(), T + DEFAULT_TOLERANCE_SECS + 1)
                .is_err(),
            "too old"
        );
        assert!(g
            .verify_signature(&format!("t={},v1={SIG}", T + 1), BODY.as_bytes(), T)
            .is_err());
        assert!(g.verify_signature("v1=abc", BODY.as_bytes(), T).is_err());
        assert!(
            StripeGateway::new("sk", "whsec_other", Box::new(NoTransport))
                .verify_signature(&h, BODY.as_bytes(), T)
                .is_err()
        );
    }

    #[test]
    fn parses_event() {
        let mut h = Headers::new();
        h.insert("stripe-signature".into(), format!("t={T},v1={SIG}"));
        let e = gw().parse_webhook(&h, BODY.as_bytes(), T).unwrap();
        assert_eq!(e.event_id, "evt_1");
        assert_eq!(e.kind, WebhookKind::PaymentSucceeded);
        assert_eq!(e.provider_ref.as_deref(), Some("cs_test_1"));
    }

    fn req() -> PaymentRequest {
        PaymentRequest {
            payment_id: "pay_123".into(),
            reference: "INV-000001".into(),
            amount: Money::new(Decimal::new(1999, 2), Currency::parse("EUR").unwrap()),
            description: "VPS S & more".into(),
            customer_email: Some("a+b@x.io".into()),
            success_url: "https://panel.example/ok".into(),
            cancel_url: "https://panel.example/cancel".into(),
            webhook_url: String::new(),
            idempotency_key: "idem-1".into(),
        }
    }

    #[test]
    fn builds_checkout_request() {
        let s = gw().checkout_request(&req()).unwrap();
        assert_eq!(s.url, "https://api.stripe.com/v1/checkout/sessions");
        assert!(s
            .headers
            .contains(&("Idempotency-Key".into(), "idem-1".into())));
        assert!(s
            .headers
            .contains(&("Authorization".into(), "Bearer sk_test_x".into())));
        assert!(s
            .body
            .contains("line_items%5B0%5D%5Bprice_data%5D%5Bunit_amount%5D=1999"));
        assert!(s
            .body
            .contains("line_items%5B0%5D%5Bprice_data%5D%5Bcurrency%5D=eur"));
        assert!(s.body.contains("product_data%5D%5Bname%5D=VPS+S+%26+more"));
        assert!(s.body.contains("customer_email=a%2Bb%40x.io"));
    }

    #[test]
    fn dynamic_payment_methods_by_default() {
        let s = gw().checkout_request(&req()).unwrap();
        assert!(!s.body.contains("payment_method_types"));
        assert!(!s.body.contains("payment_method_configuration"));
    }

    #[test]
    fn pinned_configuration_and_override() {
        let g = gw().with_methods(Some("pmc_123".into()), vec![]);
        assert!(g.checkout_request(&req()).unwrap().body.contains("payment_method_configuration=pmc_123"));
        let g = gw().with_methods(None, vec!["card".into(), "sepa_debit".into()]);
        let b = g.checkout_request(&req()).unwrap().body;
        assert!(b.contains("payment_method_types%5B%5D=card") && b.contains("payment_method_types%5B%5D=sepa_debit"));
    }

    #[test]
    fn dynamic_settings_validation() {
        use serde_json::json;
        assert_eq!(dynamic_settings(&json!({})).unwrap(), (None, vec![]));
        assert_eq!(dynamic_settings(&json!({"payment_method_configuration":" pmc_Ab1 "})).unwrap().0.as_deref(), Some("pmc_Ab1"));
        assert_eq!(dynamic_settings(&json!({"payment_method_types":"card, ideal"})).unwrap().1, vec!["card", "ideal"]);
        assert!(dynamic_settings(&json!({"payment_method_configuration":"x&y"})).is_err());
        assert!(dynamic_settings(&json!({"payment_method_types":"Card"})).is_err());
        assert!(dynamic_settings(&json!({"payment_method_configuration":"pmc_1","payment_method_types":"card"})).is_err());
    }

    struct Mock(Mutex<Vec<HttpRequestSpec>>);
    impl StripeTransport for Mock {
        fn send(&self, r: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
            if let Ok(mut v) = self.0.lock() {
                v.push(r);
            }
            Box::pin(async {
                Ok((
                    200,
                    r#"{"id":"cs_1","url":"https://checkout.stripe.com/c/cs_1"}"#.to_string(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn create_payment_with_transport() {
        let g = StripeGateway::new("sk", SECRET, Box::new(Mock(Mutex::new(vec![]))));
        let r = g.create_payment(&req()).await.unwrap();
        assert_eq!(
            r,
            PaymentInit::Redirect {
                url: "https://checkout.stripe.com/c/cs_1".into(),
                provider_ref: "cs_1".into()
            }
        );
        assert!(matches!(
            gw().create_payment(&req()).await,
            Err(GatewayError::Provider(_))
        ));
    }

    struct Saved;
    impl StripeTransport for Saved {
        fn send(&self, r: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
            let out = if r.url.ends_with("/customers") {
                (200, r#"{"id":"cus_9"}"#.to_string())
            } else if r.url.contains("/attach") {
                (200, r#"{"id":"pm_1","card":{"brand":"visa","last4":"4242","exp_month":12,"exp_year":2030}}"#.to_string())
            } else if r.body.contains("amount=777") {
                (402, r#"{"error":{"type":"card_error","code":"card_declined","decline_code":"insufficient_funds"}}"#.to_string())
            } else {
                (200, r#"{"id":"pi_5","status":"succeeded"}"#.to_string())
            };
            Box::pin(async move { Ok(out) })
        }
    }

    #[tokio::test]
    async fn save_and_charge_off_session() {
        let g = StripeGateway::new("sk", SECRET, Box::new(Saved));
        let m = g.save_method("pm_1", "a@x.io", "k").await.unwrap();
        assert_eq!((m.token.as_str(), m.brand.as_str(), m.last4.as_str(), m.exp_month, m.exp_year), ("cus_9|pm_1", "visa", "4242", 12, 2030));
        assert!(g.save_method("pm_../x", "a@x.io", "k").await.is_err());
        let spec = g.charge_request("cus_9|pm_1", &req()).unwrap();
        assert!(spec.body.contains("off_session=true") && spec.body.contains("customer=cus_9") && spec.body.contains("payment_method=pm_1"));
        assert_eq!(g.charge_saved("cus_9|pm_1", &req()).await.unwrap(), ChargeOutcome::Succeeded { provider_ref: "pi_5".into() });
        let mut r = req();
        r.amount = Money::new(Decimal::new(777, 2), Currency::parse("EUR").unwrap());
        assert_eq!(g.charge_saved("cus_9|pm_1", &r).await.unwrap(), ChargeOutcome::Declined { reason: "insufficient_funds".into() });
        assert!(g.charge_saved("bad", &req()).await.is_err());
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use pelty_payments::test_util::*;

    fn g(t: &MockTransport) -> Box<dyn PaymentGateway> {
        factory(&cfg("stripe", serde_json::json!({}), &[("secret_key", "sk_test"), ("webhook_secret", "whsec")], t)).unwrap()
    }

    #[tokio::test]
    async fn refund_resolves_checkout_session_to_payment_intent() {
        let t = MockTransport::new();
        t.push(200, r#"{"id":"cs_1","payment_intent":"pi_9"}"#).push(200, r#"{"id":"re_1","status":"succeeded"}"#);
        let r = g(&t).refund("cs_1", money("5.00", "EUR"), "k1").await.unwrap();
        assert_eq!(r, RefundResult { provider_ref: Some("re_1".into()), status: "succeeded".into() });
        let sent = t.requests();
        assert_eq!(sent[0].method, "GET");
        assert!(sent[0].url.ends_with("/checkout/sessions/cs_1"));
        assert_eq!(sent[1].body, "payment_intent=pi_9&amount=500");
        assert_eq!(sent[1].header("idempotency-key"), Some("k1"));
    }

    #[tokio::test]
    async fn provider_error_surfaces_message() {
        let t = MockTransport::new();
        t.push(402, r#"{"error":{"message":"card_declined"}}"#);
        let e = g(&t).refund("pi_1", money("1.00", "EUR"), "k").await.unwrap_err();
        assert_eq!(e, GatewayError::Provider("HTTP 402: card_declined".into()));
    }

    #[tokio::test]
    async fn complete_payment_reads_session_status() {
        let t = MockTransport::new();
        t.push(200, r#"{"status":"complete","payment_status":"paid"}"#)
            .push(200, r#"{"status":"open","payment_status":"unpaid"}"#)
            .push(200, r#"{"status":"expired","payment_status":"unpaid"}"#);
        let gw = g(&t);
        let q = HashMap::new();
        assert_eq!(gw.complete_payment("cs_1", &q).await.unwrap(), CompleteStatus::Succeeded);
        assert_eq!(gw.complete_payment("cs_1", &q).await.unwrap(), CompleteStatus::Pending);
        assert_eq!(gw.complete_payment("cs_1", &q).await.unwrap(), CompleteStatus::Failed);
    }

    fn signed(body: &str, t: i64) -> Headers {
        let sig = hex::encode(pelty_payments::http::hmac_sha256(b"whsec", format!("{t}.{body}").as_bytes()));
        hdrs(&[("stripe-signature", &format!("t={t},v1={sig}"))])
    }

    #[test]
    fn maps_refund_dispute_and_async_events() {
        let t = MockTransport::new();
        let gw = g(&t);
        let cases = [
            (r#"{"id":"e1","type":"charge.refunded","data":{"object":{"refunded":true,"payment_intent":"pi_1"}}}"#, WebhookKind::Refunded, Some("pi_1")),
            (r#"{"id":"e2","type":"charge.refunded","data":{"object":{"refunded":false,"payment_intent":"pi_1"}}}"#, WebhookKind::Ignored, Some("pi_1")),
            (r#"{"id":"e3","type":"charge.dispute.created","data":{"object":{"payment_intent":"pi_2"}}}"#, WebhookKind::Disputed, Some("pi_2")),
            (r#"{"id":"e4","type":"checkout.session.completed","data":{"object":{"id":"cs_1","payment_status":"unpaid"}}}"#, WebhookKind::Ignored, Some("cs_1")),
            (r#"{"id":"e5","type":"payment_intent.payment_failed","data":{"object":{"id":"pi_3","metadata":{"pelty_payment_id":"p-1"}}}}"#, WebhookKind::PaymentFailed, Some("pi_3")),
        ];
        for (b, k, r) in cases.clone() {
            let e = gw.parse_webhook(&signed(b, 100), b.as_bytes(), 100).unwrap();
            assert_eq!(e.kind, k, "{b}");
            assert_eq!(e.provider_ref.as_deref(), r);
        }
        let b = cases[4].0;
        assert_eq!(gw.parse_webhook(&signed(b, 100), b.as_bytes(), 100).unwrap().payment_id.as_deref(), Some("p-1"));
    }
}
