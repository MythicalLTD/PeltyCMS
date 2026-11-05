//! Host adapter: a WASM addon of kind `gateway` exposed as a [`PaymentGateway`].
use crate::{CallCtx, WasmAddon, WasmError};
use pelty_payments::wire::{self, OwnedDescriptor};
use pelty_payments::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub struct WasmGateway {
    addon: Arc<WasmAddon>,
    desc: GatewayDescriptor,
    cfg: GatewayConfig,
}

fn ctx(cfg: &GatewayConfig) -> CallCtx {
    CallCtx { settings: cfg.config.clone(), secrets: cfg.secrets.clone(), server_url: None, http: Some(cfg.transport.clone()), event_depth: 0, ..Default::default() }
}

fn map_err(e: WasmError) -> GatewayError {
    match e {
        WasmError::Addon(s) => wire::err_from(&s),
        other => GatewayError::Provider(format!("addon: {other}")),
    }
}

impl WasmGateway {
    fn input(&self, extra: Value) -> Value {
        let names: Vec<&String> = self.cfg.secrets.keys().collect();
        let mut v = json!({ "id": self.cfg.id, "config": self.cfg.config, "secret_names": names });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    }
    async fn op<T: serde::de::DeserializeOwned>(&self, op: &str, extra: Value) -> Result<T, GatewayError> {
        let out = self.addon.call_async(ctx(&self.cfg), op.into(), self.input(extra)).await.map_err(map_err)?;
        serde_json::from_value(out).map_err(|e| GatewayError::Provider(format!("addon output: {e}")))
    }
}

fn b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

impl PaymentGateway for WasmGateway {
    fn id(&self) -> &'static str {
        self.desc.id
    }
    fn display_name(&self) -> &'static str {
        self.desc.name
    }
    fn capabilities(&self) -> Capabilities {
        self.desc.capabilities
    }
    fn create_payment<'a>(&'a self, req: &'a PaymentRequest) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(self.op("create_payment", json!({ "req": wire::req_to(req) })))
    }
    fn parse_webhook(&self, headers: &Headers, body: &[u8], now: i64) -> Result<WebhookEvent, GatewayError> {
        let out = self.addon.call_json(&ctx(&self.cfg), "parse_webhook", &self.input(json!({ "headers": headers, "body_b64": b64(body), "now": now }))).map_err(map_err)?;
        serde_json::from_value(out).map_err(|e| GatewayError::Provider(format!("addon output: {e}")))
    }
    fn handle_webhook<'a>(&'a self, headers: &'a Headers, body: &'a [u8], now: i64) -> BoxFuture<'a, Result<WebhookEvent, GatewayError>> {
        Box::pin(self.op("handle_webhook", json!({ "headers": headers, "body_b64": b64(body), "now": now })))
    }
    fn complete_payment<'a>(&'a self, provider_ref: &'a str, query: &'a HashMap<String, String>) -> BoxFuture<'a, Result<CompleteStatus, GatewayError>> {
        Box::pin(self.op("complete_payment", json!({ "provider_ref": provider_ref, "query": query })))
    }
    fn refund<'a>(&'a self, provider_ref: &'a str, amount: pelty_core::money::Money, key: &'a str) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(self.op("refund", json!({ "provider_ref": provider_ref, "amount": wire::money_to(amount), "idempotency_key": key })))
    }
    fn save_method<'a>(&'a self, token: &'a str, email: &'a str, key: &'a str) -> BoxFuture<'a, Result<SavedMethod, GatewayError>> {
        Box::pin(self.op("save_method", json!({ "token": token, "customer_email": email, "idempotency_key": key })))
    }
    fn charge_saved<'a>(&'a self, token: &'a str, req: &'a PaymentRequest) -> BoxFuture<'a, Result<ChargeOutcome, GatewayError>> {
        Box::pin(self.op("charge_saved", json!({ "token": token, "req": wire::req_to(req) })))
    }
}

/// Descriptor reported by the addon's `describe()`.
pub fn descriptor(addon: &WasmAddon) -> Result<GatewayDescriptor, WasmError> {
    let d: OwnedDescriptor = serde_json::from_value(addon.descriptor.clone()).map_err(|e| WasmError::Manifest(format!("gateway descriptor: {e}")))?;
    Ok(d.into_static())
}

/// Register a gateway addon in the registry (replaces a previous version).
pub fn register(reg: &GatewayRegistry, addon: Arc<WasmAddon>) -> Result<GatewayDescriptor, WasmError> {
    let desc = descriptor(&addon)?;
    let d2 = desc.clone();
    reg.register_dyn(
        desc.clone(),
        Arc::new(move |cfg: &GatewayConfig| -> Result<Box<dyn PaymentGateway>, GatewayError> {
            let g = WasmGateway { addon: addon.clone(), desc: d2.clone(), cfg: cfg.clone() };
            // Same eager config/secret validation as a native factory (no I/O).
            g.addon.call_json(&ctx(&g.cfg), "validate", &g.input(json!({}))).map_err(map_err)?;
            Ok(Box::new(g))
        }),
    );
    Ok(desc)
}
