//! Payment gateway abstraction. Gateways are drop-in modules implementing
//! [`PaymentGateway`] and registered in [`GatewayRegistry`]. All provider HTTP
//! goes through [`http::HttpTransport`] so it can be mocked.
pub mod http;
pub mod wire;

pub use http::{HttpRequestSpec, HttpTransport, MockTransport, NoTransport};
#[cfg(not(target_family = "wasm"))]
pub use http::ReqwestTransport;
use pelty_core::money::Money;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GatewayError {
    #[error("gateway misconfigured: {0}")]
    Config(String),
    #[error("invalid webhook signature")]
    InvalidSignature,
    #[error("malformed webhook payload")]
    MalformedPayload,
    #[error("operation not supported by this gateway")]
    Unsupported,
    #[error("provider error: {0}")]
    Provider(String),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub checkout: bool,
    pub webhooks: bool,
    pub refunds: bool,
    pub recurring: bool,
}

#[derive(Debug, Clone)]
pub struct PaymentRequest {
    pub payment_id: String,
    /// Human-readable reference shown to the payer (invoice number).
    pub reference: String,
    pub amount: Money,
    pub description: String,
    pub customer_email: Option<String>,
    pub success_url: String,
    pub cancel_url: String,
    /// Public URL of `POST /api/v1/webhooks/<gateway>` (for providers that
    /// take a per-payment notification URL).
    pub webhook_url: String,
    /// Passed to providers that support it; also our DB unique key.
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaymentInit {
    Redirect {
        url: String,
        provider_ref: String,
    },
    /// `text` is operator-configured (may be empty); the UI translates labels.
    Instructions {
        text: String,
        reference: String,
        amount: String,
        currency: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookKind {
    PaymentSucceeded,
    PaymentFailed,
    Refunded,
    Disputed,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// Provider's unique event id (deduplication key).
    pub event_id: String,
    pub event_type: String,
    pub kind: WebhookKind,
    /// Provider payment reference this event concerns, if any.
    pub provider_ref: Option<String>,
    /// Our payment id when the provider echoes it back (metadata), used as a
    /// fallback when `provider_ref` is not the one stored at checkout.
    pub payment_id: Option<String>,
}

impl WebhookEvent {
    pub fn new(event_id: impl Into<String>, event_type: impl Into<String>, kind: WebhookKind, provider_ref: Option<String>) -> Self {
        WebhookEvent {
            event_id: event_id.into(),
            event_type: event_type.into(),
            kind,
            provider_ref,
            payment_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefundResult {
    pub provider_ref: Option<String>,
    /// `succeeded`, `pending`, `failed`, `canceled` or `manual_pending`.
    pub status: String,
}

/// A reusable payment method stored at the provider (card on file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedMethod {
    /// Opaque provider token used for later off-session charges.
    pub token: String,
    pub brand: String,
    pub last4: String,
    pub exp_month: u8,
    pub exp_year: u16,
}

/// Outcome of an off-session charge of a saved method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChargeOutcome {
    Succeeded { provider_ref: String },
    /// Accepted; final result arrives by webhook.
    Pending { provider_ref: String },
    /// Declined (insufficient funds, expired card …); safe to retry later.
    Declined { reason: String },
}

/// Case-insensitive header lookup supplied by the HTTP layer.
/// Outcome of finishing a payment after the payer returns (e.g. PayPal capture).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompleteStatus {
    Succeeded,
    Pending,
    Failed,
}

/// Case-insensitive header lookup supplied by the HTTP layer (lowercase keys).
pub type Headers = HashMap<String, String>;

/// Resolved gateway configuration. `secrets` were read from env variables
/// referenced in the DB (`secret_refs`) and are never serialized.
#[derive(Clone)]
pub struct GatewayConfig {
    /// Row id (`stripe`, or `custom-foo` for custom HTTP gateways).
    pub id: String,
    pub config: serde_json::Value,
    pub secrets: HashMap<String, String>,
    pub transport: Arc<dyn HttpTransport>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        GatewayConfig {
            id: String::new(),
            config: serde_json::Value::Object(Default::default()),
            secrets: HashMap::new(),
            transport: Arc::new(NoTransport),
        }
    }
}

impl GatewayConfig {
    pub fn secret(&self, k: &str) -> Result<&str, GatewayError> {
        self.secrets
            .get(k)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| GatewayError::Config(format!("missing secret {k}")))
    }
    pub fn str(&self, k: &str) -> Option<&str> {
        self.config.get(k).and_then(|v| v.as_str())
    }
    pub fn bool(&self, k: &str) -> bool {
        self.config.get(k).and_then(|v| v.as_bool()) == Some(true)
    }
}

/// The stable gateway contract.
pub trait PaymentGateway: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn create_payment<'a>(
        &'a self,
        req: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>>;
    /// Verify the signature and parse the event. `now` is unix seconds.
    fn parse_webhook(
        &self,
        headers: &Headers,
        body: &[u8],
        now: i64,
    ) -> Result<WebhookEvent, GatewayError>;
    /// Async webhook entry point used by the HTTP layer. Defaults to
    /// [`parse_webhook`](Self::parse_webhook); gateways whose webhooks carry no
    /// signature (Mollie) override it to fetch the authoritative state.
    fn handle_webhook<'a>(
        &'a self,
        headers: &'a Headers,
        body: &'a [u8],
        now: i64,
    ) -> BoxFuture<'a, Result<WebhookEvent, GatewayError>> {
        Box::pin(async move { self.parse_webhook(headers, body, now) })
    }
    /// Finish a payment after the payer returned from the provider (PayPal
    /// capture, or a status poll). Default: not needed (webhook-driven).
    fn complete_payment<'a>(
        &'a self,
        _provider_ref: &'a str,
        _query: &'a HashMap<String, String>,
    ) -> BoxFuture<'a, Result<CompleteStatus, GatewayError>> {
        Box::pin(async { Err(GatewayError::Unsupported) })
    }
    fn refund<'a>(
        &'a self,
        provider_ref: &'a str,
        amount: Money,
        idempotency_key: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>>;
    /// Attach a client-side created method token (e.g. a Stripe `pm_…`) for
    /// future off-session use. Gateways without `recurring` keep the default.
    fn save_method<'a>(
        &'a self,
        _token: &'a str,
        _customer_email: &'a str,
        _idempotency_key: &'a str,
    ) -> BoxFuture<'a, Result<SavedMethod, GatewayError>> {
        Box::pin(async { Err(GatewayError::Unsupported) })
    }
    /// Charge a saved method off-session.
    fn charge_saved<'a>(
        &'a self,
        _token: &'a str,
        _req: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<ChargeOutcome, GatewayError>> {
        Box::pin(async { Err(GatewayError::Unsupported) })
    }
}

pub type GatewayFactory = fn(&GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError>;
/// Factory that may capture state (WASM addons).
pub type DynFactory = Arc<dyn Fn(&GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> + Send + Sync>;

/// One admin-editable setting; the admin UI renders these natively.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SettingField {
    pub key: &'static str,
    /// `text`, `url`, `bool`, `number`, `select`, `textarea` or `json`.
    pub kind: &'static str,
    pub required: bool,
    /// Options for `select`.
    pub options: &'static [&'static str],
}

impl SettingField {
    pub const fn text(key: &'static str, required: bool) -> Self {
        SettingField { key, kind: "text", required, options: &[] }
    }
    pub const fn of(key: &'static str, kind: &'static str) -> Self {
        SettingField { key, kind, required: false, options: &[] }
    }
}

/// Settings every gateway understands (handled by the HTTP layer).
pub const COMMON_SETTINGS: &[SettingField] = &[
    SettingField::text("display_name", false),
    SettingField::of("min_amount", "number"),
    SettingField::of("max_amount", "number"),
    SettingField::text("currencies", false),
];

#[derive(Debug, Clone, Serialize)]
pub struct GatewayDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub capabilities: Capabilities,
    /// Secret names this gateway needs (values come from env).
    pub secrets: &'static [&'static str],
    /// Gateway-specific (non-secret) settings schema.
    pub settings: &'static [SettingField],
    /// Supported ISO currencies; empty = any.
    pub currencies: &'static [&'static str],
}

/// Prefix of operator-defined custom HTTP gateway ids.
pub const CUSTOM_PREFIX: &str = "custom-";

pub struct GatewayRegistry {
    factories: std::sync::RwLock<BTreeMap<String, (GatewayDescriptor, DynFactory)>>,
    transport: Arc<dyn HttpTransport>,
}

impl Default for GatewayRegistry {
    /// An empty registry with an offline transport. Every gateway is a WASM
    /// addon (`extensions/addons/<id>`) registered at runtime with
    /// [`GatewayRegistry::register_dyn`].
    fn default() -> Self {
        GatewayRegistry { factories: Default::default(), transport: Arc::new(NoTransport) }
    }
}

impl GatewayRegistry {
    /// Registry with no gateways at all (addons only).
    pub fn empty(t: Arc<dyn HttpTransport>) -> Self {
        GatewayRegistry { factories: Default::default(), transport: t }
    }
    pub fn with_transport(t: Arc<dyn HttpTransport>) -> Self {
        GatewayRegistry { transport: t, ..Self::default() }
    }
    pub fn set_transport(&mut self, t: Arc<dyn HttpTransport>) {
        self.transport = t;
    }
    pub fn transport(&self) -> Arc<dyn HttpTransport> {
        self.transport.clone()
    }
    pub fn register(&mut self, d: GatewayDescriptor, f: GatewayFactory) {
        self.register_dyn(d, Arc::new(f));
    }
    /// Register (or replace) a gateway at runtime.
    pub fn register_dyn(&self, d: GatewayDescriptor, f: DynFactory) {
        if let Ok(mut m) = self.factories.write() {
            m.insert(d.id.to_string(), (d, f));
        }
    }
    pub fn unregister(&self, id: &str) {
        if let Ok(mut m) = self.factories.write() {
            m.remove(id);
        }
    }
    pub fn descriptors(&self) -> Vec<GatewayDescriptor> {
        self.factories.read().map(|m| m.values().map(|(d, _)| d.clone()).collect()).unwrap_or_default()
    }
    /// Descriptor for a row id; `custom-*` ids map to the custom gateway.
    pub fn descriptor(&self, id: &str) -> Option<GatewayDescriptor> {
        let key = if id.starts_with(CUSTOM_PREFIX) { CUSTOM_ID } else { id };
        self.factories.read().ok()?.get(key).map(|(d, _)| d.clone())
    }
    pub fn build(&self, id: &str, cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
        let key = if id.starts_with(CUSTOM_PREFIX) { CUSTOM_ID } else { id };
        let f = self.factories.read().ok().and_then(|m| m.get(key).map(|(_, f)| f.clone())).ok_or_else(|| GatewayError::Config("unknown gateway".into()))?;
        let mut c = cfg.clone();
        c.id = id.to_string();
        c.transport = self.transport.clone();
        f(&c)
    }
}

/// Engine id of the custom HTTP gateway (rows `custom-*`).
pub const CUSTOM_ID: &str = "custom_http";

/// Decimal amount string with exactly the currency's minor digits ("12.50").
pub fn decimal_str(m: Money) -> Result<String, GatewayError> {
    let r = m.rounded();
    let mut v = r.amount;
    v.rescale(r.currency.minor_units());
    if v.is_sign_negative() || v.is_zero() {
        return Err(GatewayError::Config("amount must be positive".into()));
    }
    Ok(v.to_string())
}

/// Integer minor units ("1250").
pub fn minor_units(m: Money) -> Result<i64, GatewayError> {
    m.to_minor_units()
        .filter(|v| *v > 0)
        .ok_or_else(|| GatewayError::Config("amount out of range".into()))
}

/// Lowercase header lookup.
pub fn header<'a>(h: &'a Headers, k: &str) -> Option<&'a str> {
    h.get(k).map(String::as_str)
}

#[doc(hidden)]
pub fn obj_str(v: &serde_json::Value, path: &str) -> Option<String> {
    let mut cur = v;
    for p in path.split('.').filter(|p| !p.is_empty()) {
        cur = match p.parse::<usize>() {
            Ok(i) if cur.is_array() => &cur[i],
            _ => &cur[p],
        };
    }
    match cur {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Test helpers shared by the gateway addons (feature `test-util`).
#[cfg(any(test, feature = "test-util"))]
pub mod test_util {
    use super::*;
    use pelty_core::money::Currency;
    use std::str::FromStr;
    pub fn money(v: &str, c: &str) -> Money {
        Money::new(rust_decimal::Decimal::from_str(v).unwrap(), Currency::parse(c).unwrap())
    }
    pub fn req(v: &str, c: &str) -> PaymentRequest {
        PaymentRequest {
            payment_id: "pay_1".into(),
            reference: "INV-7".into(),
            amount: money(v, c),
            description: "Invoice INV-7".into(),
            customer_email: Some("a@example.test".into()),
            success_url: "https://panel.example/ok".into(),
            cancel_url: "https://panel.example/cancel".into(),
            webhook_url: "https://panel.example/api/v1/webhooks/x".into(),
            idempotency_key: "idem-1".into(),
        }
    }
    pub fn cfg(id: &str, config: serde_json::Value, secrets: &[(&str, &str)], t: &MockTransport) -> GatewayConfig {
        GatewayConfig {
            id: id.into(),
            config,
            secrets: secrets.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            transport: Arc::new(t.clone()),
        }
    }
    pub fn hdrs(p: &[(&str, &str)]) -> Headers {
        p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
}
