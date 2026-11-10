//! Payout gateways for creator payouts: Stripe Connect (Express accounts,
//! onboarding links, transfers from the platform balance, transfer
//! reversals, signed webhooks) and PayPal Payouts (OAuth, batch payouts,
//! batch status, webhook verification).
//!
//! Every call goes through the app's outbound [`HttpTransport`] so tests
//! script responses with `MockTransport`. Credentials come from the
//! environment (see [`env`]); live keys are refused unless
//! `PELTY_CREATOR_PAYOUTS_LIVE=true`, so a fresh install can only reach the
//! providers' test modes.
use base64::Engine as _;
use pelty_payments::http::{
    ct_eq, form_encode, hmac_sha256, url_encode, HttpRequestSpec, HttpTransport,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};

pub const STRIPE_API: &str = "https://api.stripe.com";
pub const PAYPAL_LIVE_API: &str = "https://api-m.paypal.com";
pub const PAYPAL_SANDBOX_API: &str = "https://api-m.sandbox.paypal.com";
/// Stripe-Signature timestamps older than this are refused (seconds).
pub const STRIPE_TOLERANCE: i64 = 300;

/// Environment variable names (values are never logged or returned).
pub mod env {
    /// Platform secret key with Connect enabled (`sk_test_…`; `sk_live_…` needs [`LIVE`]).
    pub const STRIPE_KEY: &str = "PELTY_STRIPE_CONNECT_SECRET_KEY";
    /// Older name of [`STRIPE_KEY`], still read.
    pub const STRIPE_KEY_OLD: &str = "PELTY_STRIPE_CONNECT_KEY";
    /// Signing secret of the Connect webhook endpoint (`whsec_…`).
    pub const STRIPE_WEBHOOK: &str = "PELTY_STRIPE_CONNECT_WEBHOOK_SECRET";
    /// Optional API base override (https, or http on loopback for mocks).
    pub const STRIPE_BASE: &str = "PELTY_STRIPE_CONNECT_API_BASE";
    /// Client id of the PayPal REST app with Payouts enabled (sandbox app by default).
    pub const PAYPAL_ID: &str = "PELTY_PAYPAL_PAYOUTS_CLIENT_ID";
    /// Client secret of that PayPal REST app.
    pub const PAYPAL_SECRET: &str = "PELTY_PAYPAL_PAYOUTS_CLIENT_SECRET";
    /// `sandbox` (default) or `live` (needs [`LIVE`]).
    pub const PAYPAL_MODE: &str = "PELTY_PAYPAL_PAYOUTS_MODE";
    /// Webhook id from the PayPal app, used to verify webhook signatures.
    pub const PAYPAL_WEBHOOK: &str = "PELTY_PAYPAL_PAYOUTS_WEBHOOK_ID";
    /// Optional PayPal API base override (https, or http on loopback for mocks).
    pub const PAYPAL_BASE: &str = "PELTY_PAYPAL_PAYOUTS_API_BASE";
    /// `true` allows live Stripe keys and PayPal live mode.
    pub const LIVE: &str = "PELTY_CREATOR_PAYOUTS_LIVE";
}

fn var(k: &str) -> Option<String> {
    std::env::var(k)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn live_allowed() -> bool {
    var(env::LIVE).is_some_and(|v| v == "true" || v == "1")
}

fn base(k: &str, default: &str) -> Result<String, ProvErr> {
    pelty_payments::http::api_base(&json!({ "api_base": var(k).unwrap_or_default() }), default)
        .map_err(|_| ProvErr::Unconfigured)
}

/// Why a provider call did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvErr {
    /// No (usable) credentials, or the creator has not connected the method.
    Unconfigured,
    /// Network or provider-side (5xx/429) error: safe to retry with the same
    /// idempotency key.
    Retry(String),
    /// The provider refused the request (4xx): it will not succeed as is.
    Rejected(String),
}

impl ProvErr {
    pub fn code(&self) -> &'static str {
        match self {
            ProvErr::Unconfigured => "error.payout_provider_unconfigured",
            ProvErr::Retry(_) => "error.payout_provider_unavailable",
            ProvErr::Rejected(_) => "error.payout_provider_rejected",
        }
    }
    pub fn message(&self) -> String {
        match self {
            ProvErr::Unconfigured => "not configured".into(),
            ProvErr::Retry(m) | ProvErr::Rejected(m) => m.chars().take(300).collect(),
        }
    }
}

async fn call(t: &dyn HttpTransport, spec: HttpRequestSpec) -> Result<Value, ProvErr> {
    let (status, body) = t.send(spec).await.map_err(ProvErr::Retry)?;
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(v);
    }
    let msg = [
        &v["error"]["message"],
        &v["message"],
        &v["error_description"],
        &v["name"],
        &v["error"],
    ]
    .iter()
    .find_map(|x| x.as_str())
    .unwrap_or("error")
    .to_string();
    let msg = format!("HTTP {status}: {msg}");
    if status == 429 || status >= 500 {
        Err(ProvErr::Retry(msg))
    } else {
        Err(ProvErr::Rejected(msg))
    }
}

/// Amount in minor units (`None` when it has more decimals than the currency).
pub fn to_minor(amount: Decimal, exponent: u32) -> Option<i64> {
    let scaled = amount * Decimal::from(10i64.pow(exponent));
    if scaled.fract() != Decimal::ZERO {
        return None;
    }
    rust_decimal::prelude::ToPrimitive::to_i64(&scaled)
}

pub fn from_minor(minor: i64, exponent: u32) -> Decimal {
    Decimal::new(minor, exponent)
}

/// `12.5` → `"12.50"` for a 2-decimal currency.
pub fn decimal_string(amount: Decimal, exponent: u32) -> String {
    let mut a = amount.round_dp(exponent);
    a.rescale(exponent);
    a.to_string()
}

// ------------------------------------------------------------------ Stripe

pub struct Stripe {
    key: String,
    webhook_secret: Option<String>,
    base: String,
}

impl Stripe {
    /// `Ok(None)` without a key; `Err` for a live key while live is off.
    pub fn from_env() -> Result<Option<Self>, ProvErr> {
        let Some(key) = var(env::STRIPE_KEY).or_else(|| var(env::STRIPE_KEY_OLD)) else {
            return Ok(None);
        };
        Self::new(
            key,
            var(env::STRIPE_WEBHOOK),
            base(env::STRIPE_BASE, STRIPE_API)?,
        )
        .map(Some)
    }

    pub fn new(key: String, webhook_secret: Option<String>, base: String) -> Result<Self, ProvErr> {
        let test = key.starts_with("sk_test_") || key.starts_with("rk_test_");
        let live = key.starts_with("sk_live_") || key.starts_with("rk_live_");
        if !test && !live {
            return Err(ProvErr::Unconfigured);
        }
        if live && !live_allowed() {
            return Err(ProvErr::Unconfigured);
        }
        Ok(Self {
            key,
            webhook_secret,
            base,
        })
    }

    pub fn test_mode(&self) -> bool {
        self.key.contains("_test_")
    }

    pub fn webhook_ready(&self) -> bool {
        self.webhook_secret.is_some()
    }

    fn spec(
        &self,
        method: &'static str,
        path: &str,
        form: &[(&str, String)],
        idem: Option<&str>,
    ) -> HttpRequestSpec {
        let mut headers = vec![
            ("Authorization".to_string(), format!("Bearer {}", self.key)),
            ("Stripe-Version".to_string(), "2024-06-20".to_string()),
        ];
        if method == "POST" {
            headers.push((
                "Content-Type".into(),
                "application/x-www-form-urlencoded".into(),
            ));
        }
        if let Some(k) = idem {
            headers.push(("Idempotency-Key".into(), k.to_string()));
        }
        HttpRequestSpec {
            method,
            url: format!("{}{path}", self.base),
            headers,
            body: form_encode(form),
        }
    }

    /// Creates an Express account able to receive transfers.
    pub async fn create_account(
        &self,
        t: &dyn HttpTransport,
        creator: uuid::Uuid,
        email: &str,
        country: Option<&str>,
    ) -> Result<String, ProvErr> {
        let mut f = vec![
            ("type", "express".to_string()),
            ("capabilities[transfers][requested]", "true".into()),
            ("metadata[creator_id]", creator.to_string()),
        ];
        if !email.is_empty() {
            f.push(("email", email.to_string()));
        }
        if let Some(c) =
            country.filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            f.push(("country", c.to_ascii_uppercase()));
            // Cross-border Express accounts only take transfers under the recipient agreement.
            f.push(("tos_acceptance[service_agreement]", "recipient".into()));
        }
        let v = call(
            t,
            self.spec(
                "POST",
                "/v1/accounts",
                &f,
                Some(&format!("pelty-acct-{creator}")),
            ),
        )
        .await?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProvErr::Retry("no account id".into()))
    }

    /// Hosted onboarding link (single use, short lived).
    pub async fn account_link(
        &self,
        t: &dyn HttpTransport,
        account: &str,
        refresh: &str,
        ret: &str,
    ) -> Result<String, ProvErr> {
        let f = [
            ("account", account.to_string()),
            ("refresh_url", refresh.to_string()),
            ("return_url", ret.to_string()),
            ("type", "account_onboarding".into()),
        ];
        let v = call(t, self.spec("POST", "/v1/account_links", &f, None)).await?;
        v["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProvErr::Retry("no link".into()))
    }

    /// Express dashboard login link.
    pub async fn login_link(
        &self,
        t: &dyn HttpTransport,
        account: &str,
    ) -> Result<String, ProvErr> {
        let path = format!("/v1/accounts/{}/login_links", url_encode(account));
        let v = call(t, self.spec("POST", &path, &[], None)).await?;
        v["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProvErr::Retry("no link".into()))
    }

    pub async fn account(&self, t: &dyn HttpTransport, account: &str) -> Result<Value, ProvErr> {
        call(
            t,
            self.spec(
                "GET",
                &format!("/v1/accounts/{}", url_encode(account)),
                &[],
                None,
            ),
        )
        .await
    }

    /// Transfer from the platform balance; the payout id is the idempotency key.
    pub async fn transfer(
        &self,
        t: &dyn HttpTransport,
        account: &str,
        minor: i64,
        currency: &str,
        payout: uuid::Uuid,
    ) -> Result<String, ProvErr> {
        let f = [
            ("amount", minor.to_string()),
            ("currency", currency.to_ascii_lowercase()),
            ("destination", account.to_string()),
            ("transfer_group", format!("payout_{payout}")),
            ("description", "Creator payout".into()),
            ("metadata[payout_id]", payout.to_string()),
        ];
        let v = call(
            t,
            self.spec(
                "POST",
                "/v1/transfers",
                &f,
                Some(&format!("pelty-payout-{payout}")),
            ),
        )
        .await?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProvErr::Retry("no transfer id".into()))
    }

    /// Reverse part of a transfer: `(reversal id, amount reversed in minor units)`.
    pub async fn reverse(
        &self,
        t: &dyn HttpTransport,
        transfer: &str,
        minor: i64,
        idem: &str,
    ) -> Result<(String, i64), ProvErr> {
        let path = format!("/v1/transfers/{}/reversals", url_encode(transfer));
        let f = [
            ("amount", minor.to_string()),
            ("description", "Refund after creator payout".into()),
        ];
        let v = call(t, self.spec("POST", &path, &f, Some(idem))).await?;
        match (v["id"].as_str(), v["amount"].as_i64()) {
            (Some(id), Some(a)) => Ok((id.to_string(), a)),
            _ => Err(ProvErr::Retry("no reversal id".into())),
        }
    }

    /// Checks `Stripe-Signature` (`t=…,v1=…`) over `t.body` within the tolerance.
    pub fn verify(&self, header: &str, body: &[u8], now: i64) -> bool {
        let Some(secret) = &self.webhook_secret else {
            return false;
        };
        verify_stripe(secret, header, body, now)
    }
}

pub fn verify_stripe(secret: &str, header: &str, body: &[u8], now: i64) -> bool {
    let mut ts = None;
    let mut sigs = vec![];
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => ts = v.parse::<i64>().ok(),
            Some(("v1", v)) => sigs.push(v.to_string()),
            _ => {}
        }
    }
    let Some(ts) = ts else { return false };
    if (now - ts).abs() > STRIPE_TOLERANCE {
        return false;
    }
    let mut msg = format!("{ts}.").into_bytes();
    msg.extend_from_slice(body);
    let want = hex::encode(hmac_sha256(secret.as_bytes(), &msg));
    sigs.iter().any(|s| ct_eq(s.as_bytes(), want.as_bytes()))
}

/// Signature header for a payload (tests and docs).
pub fn sign_stripe(secret: &str, body: &[u8], ts: i64) -> String {
    let mut msg = format!("{ts}.").into_bytes();
    msg.extend_from_slice(body);
    format!(
        "t={ts},v1={}",
        hex::encode(hmac_sha256(secret.as_bytes(), &msg))
    )
}

/// Our status for a Stripe account object, with a short reason.
pub fn account_status(a: &Value) -> (&'static str, String) {
    let transfers = a["capabilities"]["transfers"].as_str().unwrap_or("");
    let payouts = a["payouts_enabled"].as_bool().unwrap_or(false);
    let submitted = a["details_submitted"].as_bool().unwrap_or(false);
    let reason = a["requirements"]["disabled_reason"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let due: Vec<&str> = a["requirements"]["currently_due"]
        .as_array()
        .map(|x| x.iter().filter_map(Value::as_str).take(5).collect())
        .unwrap_or_default();
    let detail = match (reason.is_empty(), due.is_empty()) {
        (true, true) => String::new(),
        (false, true) => reason,
        (true, false) => format!("due: {}", due.join(", ")),
        (false, false) => format!("{reason}; due: {}", due.join(", ")),
    };
    if transfers == "active" && payouts {
        ("enabled", String::new())
    } else if submitted {
        ("restricted", detail)
    } else {
        ("onboarding", detail)
    }
}

// ------------------------------------------------------------------ PayPal

pub struct PayPal {
    id: String,
    secret: String,
    webhook_id: Option<String>,
    base: String,
    live: bool,
}

/// One PayPal Payouts item to send.
pub struct PayPalItem {
    pub payout: uuid::Uuid,
    pub email: String,
    pub value: String,
    pub currency: String,
}

/// Status of one item in a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStatus {
    pub payout: uuid::Uuid,
    pub status: String,
    pub item_id: String,
    pub error: String,
}

/// What a PayPal item status means for the payout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Paid,
    Failed,
    Pending,
}

pub fn paypal_outcome(status: &str) -> Outcome {
    match status.to_ascii_uppercase().as_str() {
        "SUCCESS" => Outcome::Paid,
        "FAILED" | "RETURNED" | "BLOCKED" | "REFUNDED" | "REVERSED" | "DENIED" | "CANCELED" => {
            Outcome::Failed
        }
        // PENDING, UNCLAIMED (returned after 30 days), ONHOLD, NEW…
        _ => Outcome::Pending,
    }
}

impl PayPal {
    pub fn from_env() -> Result<Option<Self>, ProvErr> {
        let (Some(id), Some(secret)) = (var(env::PAYPAL_ID), var(env::PAYPAL_SECRET)) else {
            return Ok(None);
        };
        let live = var(env::PAYPAL_MODE).is_some_and(|m| m.eq_ignore_ascii_case("live"));
        if live && !live_allowed() {
            return Err(ProvErr::Unconfigured);
        }
        let default = if live {
            PAYPAL_LIVE_API
        } else {
            PAYPAL_SANDBOX_API
        };
        Ok(Some(Self {
            id,
            secret,
            webhook_id: var(env::PAYPAL_WEBHOOK),
            base: base(env::PAYPAL_BASE, default)?,
            live,
        }))
    }

    pub fn test_mode(&self) -> bool {
        !self.live
    }

    pub fn webhook_ready(&self) -> bool {
        self.webhook_id.is_some()
    }

    fn spec(
        &self,
        method: &'static str,
        path: &str,
        token: &str,
        body: Option<&Value>,
        request_id: Option<&str>,
    ) -> HttpRequestSpec {
        let mut headers = vec![
            ("Authorization".to_string(), format!("Bearer {token}")),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        if let Some(r) = request_id {
            headers.push(("PayPal-Request-Id".into(), r.to_string()));
        }
        HttpRequestSpec {
            method,
            url: format!("{}{path}", self.base),
            headers,
            body: body.map(Value::to_string).unwrap_or_default(),
        }
    }

    pub async fn token(&self, t: &dyn HttpTransport) -> Result<String, ProvErr> {
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.id, self.secret));
        let spec = HttpRequestSpec {
            method: "POST",
            url: format!("{}/v1/oauth2/token", self.base),
            headers: vec![
                ("Authorization".into(), format!("Basic {basic}")),
                (
                    "Content-Type".into(),
                    "application/x-www-form-urlencoded".into(),
                ),
            ],
            body: "grant_type=client_credentials".into(),
        };
        let v = call(t, spec).await.map_err(|e| match e {
            // Wrong credentials are a configuration problem, not a payout rejection.
            ProvErr::Rejected(_) => ProvErr::Unconfigured,
            e => e,
        })?;
        v["access_token"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProvErr::Retry("no token".into()))
    }

    /// Creates a batch; `sender_batch_id` doubles as `PayPal-Request-Id`, so a
    /// retry after a lost answer returns the same batch.
    pub async fn create_batch(
        &self,
        t: &dyn HttpTransport,
        token: &str,
        sender_batch_id: &str,
        subject: &str,
        items: &[PayPalItem],
    ) -> Result<String, ProvErr> {
        let body = json!({
            "sender_batch_header": { "sender_batch_id": sender_batch_id, "email_subject": subject, "recipient_type": "EMAIL" },
            "items": items.iter().map(|i| json!({
                "recipient_type": "EMAIL",
                "amount": { "value": i.value, "currency": i.currency },
                "receiver": i.email,
                "note": "Creator payout",
                "sender_item_id": i.payout.to_string(),
            })).collect::<Vec<_>>(),
        });
        let v = call(
            t,
            self.spec(
                "POST",
                "/v1/payments/payouts",
                token,
                Some(&body),
                Some(sender_batch_id),
            ),
        )
        .await?;
        v["batch_header"]["payout_batch_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProvErr::Retry("no batch id".into()))
    }

    pub async fn batch(
        &self,
        t: &dyn HttpTransport,
        token: &str,
        batch: &str,
    ) -> Result<Vec<ItemStatus>, ProvErr> {
        let path = format!("/v1/payments/payouts/{}?page_size=1000", url_encode(batch));
        let v = call(t, self.spec("GET", &path, token, None, None)).await?;
        Ok(v["items"]
            .as_array()
            .map(|a| a.iter().filter_map(item_status).collect())
            .unwrap_or_default())
    }

    /// Asks PayPal whether a webhook delivery is genuine.
    pub async fn verify(
        &self,
        t: &dyn HttpTransport,
        token: &str,
        headers: &std::collections::HashMap<String, String>,
        event: &Value,
    ) -> Result<bool, ProvErr> {
        let h = |k: &str| headers.get(k).cloned();
        let Some(webhook_id) = &self.webhook_id else {
            return Err(ProvErr::Unconfigured);
        };
        let (Some(algo), Some(cert), Some(tid), Some(sig), Some(time)) = (
            h("paypal-auth-algo"),
            h("paypal-cert-url"),
            h("paypal-transmission-id"),
            h("paypal-transmission-sig"),
            h("paypal-transmission-time"),
        ) else {
            return Ok(false);
        };
        let body = json!({
            "auth_algo": algo, "cert_url": cert, "transmission_id": tid, "transmission_sig": sig,
            "transmission_time": time, "webhook_id": webhook_id, "webhook_event": event,
        });
        let v = call(
            t,
            self.spec(
                "POST",
                "/v1/notifications/verify-webhook-signature",
                token,
                Some(&body),
                None,
            ),
        )
        .await?;
        Ok(v["verification_status"] == "SUCCESS")
    }
}

/// Item status from a batch item or a `PAYMENT.PAYOUTS-ITEM.*` webhook resource.
pub fn item_status(i: &Value) -> Option<ItemStatus> {
    let payout = i["payout_item"]["sender_item_id"].as_str()?.parse().ok()?;
    Some(ItemStatus {
        payout,
        status: i["transaction_status"].as_str().unwrap_or("").to_string(),
        item_id: i["payout_item_id"].as_str().unwrap_or("").to_string(),
        error: [&i["errors"]["message"], &i["errors"]["name"]]
            .iter()
            .find_map(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minor_units() {
        assert_eq!(to_minor(Decimal::new(1250, 2), 2), Some(1250));
        assert_eq!(to_minor(Decimal::new(40, 0), 2), Some(4000));
        assert_eq!(to_minor(Decimal::new(1001, 3), 2), None);
        assert_eq!(to_minor(Decimal::new(500, 0), 0), Some(500));
        assert_eq!(from_minor(1250, 2), Decimal::new(1250, 2));
        assert_eq!(decimal_string(Decimal::new(125, 1), 2), "12.50");
        assert_eq!(decimal_string(Decimal::new(500, 0), 0), "500");
    }

    #[test]
    fn stripe_signatures() {
        let body = br#"{"id":"evt_1"}"#;
        let h = sign_stripe("whsec_x", body, 1_000);
        assert!(verify_stripe("whsec_x", &h, body, 1_100));
        assert!(
            !verify_stripe("whsec_x", &h, body, 1_000 + STRIPE_TOLERANCE + 1),
            "too old"
        );
        assert!(!verify_stripe("whsec_y", &h, body, 1_100), "other secret");
        assert!(
            !verify_stripe("whsec_x", &h, br#"{"id":"evt_2"}"#, 1_100),
            "tampered"
        );
        assert!(
            !verify_stripe("whsec_x", "v1=abc", body, 1_100),
            "no timestamp"
        );
    }

    #[test]
    fn statuses() {
        let a = json!({"capabilities": {"transfers": "active"}, "payouts_enabled": true, "details_submitted": true});
        assert_eq!(account_status(&a).0, "enabled");
        let a = json!({"capabilities": {"transfers": "inactive"}, "details_submitted": true,
            "requirements": {"disabled_reason": "requirements.past_due", "currently_due": ["external_account"]}});
        assert_eq!(
            account_status(&a),
            (
                "restricted",
                "requirements.past_due; due: external_account".into()
            )
        );
        assert_eq!(account_status(&json!({})).0, "onboarding");
        assert_eq!(paypal_outcome("SUCCESS"), Outcome::Paid);
        assert_eq!(paypal_outcome("returned"), Outcome::Failed);
        assert_eq!(paypal_outcome("UNCLAIMED"), Outcome::Pending);
    }

    #[test]
    fn live_keys_need_opt_in() {
        std::env::remove_var(env::LIVE);
        let b = STRIPE_API.to_string();
        assert!(Stripe::new("sk_test_x".into(), None, b.clone()).is_ok());
        assert!(Stripe::new("sk_live_x".into(), None, b.clone()).is_err());
        assert!(
            Stripe::new("pk_test_x".into(), None, b).is_err(),
            "publishable keys are not secret keys"
        );
    }
}
