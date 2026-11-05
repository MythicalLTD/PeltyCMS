//! JSON wire format between the host and WASM gateway addons
//! (`pelty:addon@0.2` world, kind `gateway`). Money travels as exact decimal
//! strings, never floats.
use crate::*;
use pelty_core::money::{Currency, Money};
use serde_json::{json, Value};
use std::str::FromStr;

pub fn money_to(m: Money) -> Value {
    json!({ "amount": m.amount.to_string(), "currency": m.currency.as_str() })
}

pub fn money_from(v: &Value) -> Result<Money, GatewayError> {
    let bad = || GatewayError::Config("bad money".into());
    let a = v["amount"].as_str().ok_or_else(bad)?;
    let c = Currency::parse(v["currency"].as_str().ok_or_else(bad)?).ok_or_else(bad)?;
    Ok(Money::new(rust_decimal::Decimal::from_str(a).map_err(|_| bad())?, c))
}

pub fn req_to(r: &PaymentRequest) -> Value {
    json!({
        "payment_id": r.payment_id, "reference": r.reference, "amount": money_to(r.amount),
        "description": r.description, "customer_email": r.customer_email, "success_url": r.success_url,
        "cancel_url": r.cancel_url, "webhook_url": r.webhook_url, "idempotency_key": r.idempotency_key,
    })
}

pub fn req_from(v: &Value) -> Result<PaymentRequest, GatewayError> {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Ok(PaymentRequest {
        payment_id: s("payment_id"),
        reference: s("reference"),
        amount: money_from(&v["amount"])?,
        description: s("description"),
        customer_email: v["customer_email"].as_str().map(str::to_string),
        success_url: s("success_url"),
        cancel_url: s("cancel_url"),
        webhook_url: s("webhook_url"),
        idempotency_key: s("idempotency_key"),
    })
}

/// `code:message` so the host can rebuild the exact [`GatewayError`].
pub fn err_to(e: &GatewayError) -> String {
    match e {
        GatewayError::Config(m) => format!("config:{m}"),
        GatewayError::InvalidSignature => "invalid_signature:".into(),
        GatewayError::MalformedPayload => "malformed_payload:".into(),
        GatewayError::Unsupported => "unsupported:".into(),
        GatewayError::Provider(m) => format!("provider:{m}"),
    }
}

pub fn err_from(s: &str) -> GatewayError {
    let (code, msg) = s.split_once(':').unwrap_or(("provider", s));
    match code {
        "config" => GatewayError::Config(msg.into()),
        "invalid_signature" => GatewayError::InvalidSignature,
        "malformed_payload" => GatewayError::MalformedPayload,
        "unsupported" => GatewayError::Unsupported,
        "provider" => GatewayError::Provider(msg.into()),
        _ => GatewayError::Provider(s.into()),
    }
}

/// Owned descriptor as sent by `describe()`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OwnedField {
    pub key: String,
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OwnedDescriptor {
    pub id: String,
    pub name: String,
    pub capabilities: Capabilities,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub settings: Vec<OwnedField>,
    #[serde(default)]
    pub currencies: Vec<String>,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
fn leak_list(v: Vec<String>) -> &'static [&'static str] {
    Box::leak(v.into_iter().map(leak).collect::<Vec<_>>().into_boxed_slice())
}

impl OwnedDescriptor {
    /// Turn into the registry's `'static` descriptor. Leaks a few bytes once
    /// per addon load (bounded by the number of installed addons).
    pub fn into_static(self) -> GatewayDescriptor {
        let settings: Vec<SettingField> = self
            .settings
            .into_iter()
            .map(|f| SettingField { key: leak(f.key), kind: leak(f.kind), required: f.required, options: leak_list(f.options) })
            .collect();
        GatewayDescriptor {
            id: leak(self.id),
            name: leak(self.name),
            capabilities: self.capabilities,
            secrets: leak_list(self.secrets),
            settings: Box::leak(settings.into_boxed_slice()),
            currencies: leak_list(self.currencies),
        }
    }
}

/// Drives one gateway future to completion inside the guest.
pub type BlockOn = dyn Fn(BoxFuture<'_, Result<Value, GatewayError>>) -> Result<Value, GatewayError>;

/// Guest-side dispatcher: run `op` against a gateway built by `factory`.
/// `secret` reads declared secrets from the host. Async gateway futures are
/// driven by `block_on` (host HTTP is synchronous inside the sandbox).
pub fn dispatch(
    desc: &GatewayDescriptor,
    factory: GatewayFactory,
    transport: Arc<dyn HttpTransport>,
    secret: &dyn Fn(&str) -> Option<String>,
    block_on: &BlockOn,
    op: &str,
    input: &Value,
) -> Result<Value, String> {
    let mut secrets = HashMap::new();
    let extra: Vec<String> = input["secret_names"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    for k in desc.secrets.iter().map(|s| s.to_string()).chain(extra) {
        if secrets.contains_key(&k) {
            continue;
        }
        if let Some(v) = secret(&k) {
            secrets.insert(k, v);
        }
    }
    let cfg = GatewayConfig { id: input["id"].as_str().unwrap_or(desc.id).to_string(), config: input["config"].clone(), secrets, transport };
    let gw = factory(&cfg).map_err(|e| err_to(&e))?;
    let gw = gw.as_ref();
    let headers = || -> Headers { input["headers"].as_object().map(|m| m.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default() };
    let body = || -> Result<Vec<u8>, GatewayError> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(input["body_b64"].as_str().unwrap_or_default()).map_err(|_| GatewayError::MalformedPayload)
    };
    let now = input["now"].as_i64().unwrap_or(0);
    let to = |r: Result<Value, GatewayError>| r.map_err(|e| err_to(&e));
    let ser = |v: Result<Value, serde_json::Error>| v.map_err(|e| GatewayError::Provider(e.to_string()));
    match op {
        // Factory ran above: config and secrets are valid.
        "validate" => Ok(Value::Null),
        "create_payment" => {
            let req = req_from(&input["req"]).map_err(|e| err_to(&e))?;
            to(block_on(Box::pin(async move { ser(serde_json::to_value(gw.create_payment(&req).await?)) })))
        }
        "parse_webhook" => {
            let (h, b) = (headers(), body().map_err(|e| err_to(&e))?);
            to(ser(serde_json::to_value(gw.parse_webhook(&h, &b, now).map_err(|e| err_to(&e))?)))
        }
        "handle_webhook" => {
            let (h, b) = (headers(), body().map_err(|e| err_to(&e))?);
            to(block_on(Box::pin(async move { ser(serde_json::to_value(gw.handle_webhook(&h, &b, now).await?)) })))
        }
        "complete_payment" => {
            let pr = input["provider_ref"].as_str().unwrap_or_default().to_string();
            let q: HashMap<String, String> = input["query"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
            to(block_on(Box::pin(async move { ser(serde_json::to_value(gw.complete_payment(&pr, &q).await?)) })))
        }
        "refund" => {
            let pr = input["provider_ref"].as_str().unwrap_or_default().to_string();
            let amount = money_from(&input["amount"]).map_err(|e| err_to(&e))?;
            let key = input["idempotency_key"].as_str().unwrap_or_default().to_string();
            to(block_on(Box::pin(async move { ser(serde_json::to_value(gw.refund(&pr, amount, &key).await?)) })))
        }
        "save_method" => {
            let s = |k: &str| input[k].as_str().unwrap_or_default().to_string();
            let (t, e, k) = (s("token"), s("customer_email"), s("idempotency_key"));
            to(block_on(Box::pin(async move { ser(serde_json::to_value(gw.save_method(&t, &e, &k).await?)) })))
        }
        "charge_saved" => {
            let t = input["token"].as_str().unwrap_or_default().to_string();
            let req = req_from(&input["req"]).map_err(|e| err_to(&e))?;
            to(block_on(Box::pin(async move { ser(serde_json::to_value(gw.charge_saved(&t, &req).await?)) })))
        }
        other => Err(format!("unsupported:unknown op {other}")),
    }
}
