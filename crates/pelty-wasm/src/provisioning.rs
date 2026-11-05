//! Host adapters: WASM addons of kind `provisioning` / `registrar` exposed as
//! [`ProvisioningModule`] / [`RegistrarModule`].
use crate::{CallCtx, WasmAddon, WasmError};
use pelty_provisioning::registrar::{Contact, RegistrarModule, RegistrarRegistry, RegistrarResult, RemoteDomain};
use pelty_provisioning::wire;
use pelty_provisioning::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[derive(Deserialize)]
struct OField {
    key: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: Option<Value>,
}
impl OField {
    fn leak(self) -> FieldSpec {
        FieldSpec { key: leak(self.key), label: leak(self.label), kind: leak(self.kind), required: self.required, default: self.default }
    }
}

#[derive(Deserialize)]
struct OCheckout {
    key: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    choices: Vec<String>,
    #[serde(default)]
    pattern: String,
    #[serde(default)]
    required: bool,
}

#[derive(Deserialize)]
struct OInfo {
    id: String,
    name: String,
    #[serde(default)]
    server_fields: Vec<OField>,
    #[serde(default)]
    product_fields: Vec<OField>,
    #[serde(default)]
    actions: Vec<String>,
}

#[derive(Deserialize)]
struct ODescribe {
    info: OInfo,
    #[serde(default)]
    checkout_fields: Vec<OCheckout>,
}

/// Static view of a provisioning addon's `describe()` (leaked once per load).
#[derive(Clone)]
struct ModDesc {
    id: &'static str,
    name: &'static str,
    server_fields: Vec<FieldSpec>,
    product_fields: Vec<FieldSpec>,
    actions: &'static [&'static str],
    checkout: Vec<CheckoutField>,
}

fn mod_desc(addon: &WasmAddon) -> Result<ModDesc, WasmError> {
    let d: ODescribe = serde_json::from_value(addon.descriptor.clone()).map_err(|e| WasmError::Manifest(format!("provisioning descriptor: {e}")))?;
    let actions: Vec<&'static str> = d.info.actions.into_iter().map(leak).collect();
    Ok(ModDesc {
        id: leak(d.info.id),
        name: leak(d.info.name),
        server_fields: d.info.server_fields.into_iter().map(OField::leak).collect(),
        product_fields: d.info.product_fields.into_iter().map(OField::leak).collect(),
        actions: Box::leak(actions.into_boxed_slice()),
        checkout: d
            .checkout_fields
            .into_iter()
            .map(|c| {
                let ch: Vec<&'static str> = c.choices.into_iter().map(leak).collect();
                CheckoutField { key: leak(c.key), name: leak(c.name), description: leak(c.description), kind: leak(c.kind), choices: Box::leak(ch.into_boxed_slice()), pattern: leak(c.pattern), required: c.required }
            })
            .collect(),
    })
}

pub struct WasmModule {
    addon: Arc<WasmAddon>,
    desc: ModDesc,
}

impl WasmModule {
    pub fn new(addon: Arc<WasmAddon>) -> Result<Self, WasmError> {
        Ok(WasmModule { desc: mod_desc(&addon)?, addon })
    }
    /// Panels are reached directly (SSRF-guarded), never through the
    /// gateway transport, matching the former native modules.
    async fn op(&self, server: &ServerConn, svc: Option<&ServiceCtx>, op: &str, extra: Value) -> ProvisionResult<Value> {
        let mut secrets = std::collections::HashMap::new();
        secrets.insert("api_key".to_string(), server.api_key.clone());
        let ctx = CallCtx { settings: server.config.clone(), secrets, server_url: Some(server.base_url.clone()), http: Some(Arc::new(crate::runtime::GuardedHttp)), event_depth: 0, ..Default::default() };
        let mut input = json!({ "server": wire::server_to(server), "svc": svc.map(wire::svc_to).unwrap_or(Value::Null) });
        if let (Some(o), Some(e)) = (input.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        self.addon.call_async(ctx, op.into(), input).await.map_err(|e| match e {
            WasmError::Addon(s) => { wire::perr_from(&s) }
            other => {
                tracing::warn!(addon = %self.desc.id, op, error = %other, "provisioning addon call failed");
                ProvisionError::Transient(format!("addon: {other}"))
            }
        })
    }
}

fn de<T: serde::de::DeserializeOwned>(v: Value) -> ProvisionResult<T> {
    serde_json::from_value(v).map_err(|e| ProvisionError::Transient(format!("addon output: {e}")))
}

impl ProvisioningModule for WasmModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo { id: self.desc.id, name: self.desc.name, server_fields: self.desc.server_fields.clone(), product_fields: self.desc.product_fields.clone(), actions: vec![] }
    }
    fn checkout_fields(&self) -> Vec<CheckoutField> {
        self.desc.checkout.clone()
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        self.desc.actions
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { self.op(server, None, "test_connection", json!({})).await.map(|_| ()) })
    }
    fn create<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            let v = self.op(server, Some(svc), "create", json!({})).await?;
            Ok(Created { remote_id: v["remote_id"].as_str().unwrap_or_default().into(), module_data: v["module_data"].clone() })
        })
    }
    fn suspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { self.op(server, Some(svc), "suspend", json!({})).await.map(|_| ()) })
    }
    fn unsuspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { self.op(server, Some(svc), "unsuspend", json!({})).await.map(|_| ()) })
    }
    fn terminate<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { self.op(server, Some(svc), "terminate", json!({})).await.map(|_| ()) })
    }
    fn change_package<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { self.op(server, Some(svc), "change_package", json!({})).await.map(|_| ()) })
    }
    fn sso_url<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, token: &'a str) -> BoxFuture<'a, ProvisionResult<Option<String>>> {
        Box::pin(async move { de(self.op(server, Some(svc), "sso_url", json!({ "token": token })).await?) })
    }
    fn custom<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, action: &'a str, params: &'a Value) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move { self.op(server, Some(svc), "custom", json!({ "action": action, "params": params })).await })
    }
    fn usage<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move { de(self.op(server, Some(svc), "usage", json!({})).await?) })
    }
    fn list_accounts<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<Option<Vec<RemoteAccount>>>> {
        Box::pin(async move {
            match self.op(server, None, "list_accounts", json!({})).await {
                Ok(v) => de(v),
                // Addons built before this op answer "unknown op": unsupported.
                Err(ProvisionError::Config(m)) if m.starts_with("unknown op") => Ok(None),
                Err(e) => Err(e),
            }
        })
    }
    fn catalog<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<Option<Vec<VendorProduct>>>> {
        Box::pin(async move {
            match self.op(server, None, "catalog", json!({})).await {
                Ok(v) => de(v),
                Err(ProvisionError::Config(m)) if m.starts_with("unknown op") => Ok(None),
                Err(e) => Err(e),
            }
        })
    }
}

/// Register a provisioning addon; returns its module id.
pub fn register(reg: &Registry, addon: Arc<WasmAddon>) -> Result<&'static str, WasmError> {
    let m = WasmModule::new(addon)?;
    let id = m.desc.id;
    reg.register_dyn(Arc::new(m));
    Ok(id)
}

// ----- registrar -----

pub struct WasmRegistrar {
    addon: Arc<WasmAddon>,
    id: &'static str,
    name: &'static str,
    settings: Value,
}

impl WasmRegistrar {
    pub fn new(addon: Arc<WasmAddon>) -> Result<Self, WasmError> {
        let d = &addon.descriptor;
        let id = d["id"].as_str().ok_or_else(|| WasmError::Manifest("registrar descriptor: missing id".into()))?;
        let name = d["name"].as_str().unwrap_or(id);
        Ok(WasmRegistrar { id: leak(id.into()), name: leak(name.into()), addon, settings: json!({}) })
    }
    /// Settings schema reported by the addon (`[{key,kind,required}]`).
    pub fn settings_schema(&self) -> Value {
        self.addon.descriptor["settings"].clone()
    }
    async fn op(&self, op: &str, mut input: Value) -> RegistrarResult<Value> {
        input["settings"] = self.settings.clone();
        let ctx = CallCtx { settings: self.settings.clone(), secrets: Default::default(), server_url: None, http: None, event_depth: 0, ..Default::default() };
        self.addon.call_async(ctx, op.into(), input).await.map_err(|e| match e {
            WasmError::Addon(s) => wire::rerr_from(&s),
            other => registrar::RegistrarError::Remote(format!("addon: {other}")),
        })
    }
}

impl RegistrarModule for WasmRegistrar {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(WasmRegistrar { addon: self.addon.clone(), id: self.id, name: self.name, settings: settings.clone() }))
    }
    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move { Ok(self.op("available", json!({ "domain": domain })).await?.as_bool().unwrap_or(false)) })
    }
    fn register<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("register", json!({ "domain": domain, "years": years, "expires_on": expires_on, "registrant": registrant, "nameservers": nameservers })).await.map(|_| ()) })
    }
    fn register_with_fields<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String], fields: &'a Value) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("register", json!({ "domain": domain, "years": years, "expires_on": expires_on, "registrant": registrant, "nameservers": nameservers, "fields": fields })).await.map(|_| ()) })
    }
    fn renew<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("renew", json!({ "domain": domain, "years": years, "expires_on": expires_on })).await.map(|_| ()) })
    }
    fn transfer<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("transfer", json!({ "domain": domain, "auth_code": auth_code, "expires_on": expires_on })).await.map(|_| ()) })
    }
    fn transfer_with_fields<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str, fields: &'a Value) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("transfer", json!({ "domain": domain, "auth_code": auth_code, "expires_on": expires_on, "fields": fields })).await.map(|_| ()) })
    }
    fn set_nameservers<'a>(&'a self, domain: &'a str, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("set_nameservers", json!({ "domain": domain, "nameservers": nameservers })).await.map(|_| ()) })
    }
    fn set_contact<'a>(&'a self, domain: &'a str, registrant: &'a Contact) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move { self.op("set_contact", json!({ "domain": domain, "registrant": registrant })).await.map(|_| ()) })
    }
    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move { Ok(wire::remote_from(&self.op("info", json!({ "domain": domain })).await?)) })
    }
    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move { Ok(self.op("epp_code", json!({ "domain": domain })).await?.as_str().map(str::to_string)) })
    }
    fn tld_prices<'a>(&'a self) -> BoxFuture<'a, RegistrarResult<Option<Vec<registrar::TldCost>>>> {
        Box::pin(async move { optional(self.op("tld_prices", json!({})).await) })
    }
    fn premium_price<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<registrar::PremiumQuote>>> {
        Box::pin(async move { optional(self.op("premium_price", json!({ "domain": domain })).await) })
    }
}

/// Optional registrar ops: older addons answer "unknown op" (= unsupported).
fn optional<T: serde::de::DeserializeOwned>(r: RegistrarResult<Value>) -> RegistrarResult<Option<T>> {
    match r {
        Ok(v) if v.is_null() => Ok(None),
        Ok(v) => serde_json::from_value(v).map(Some).map_err(|e| registrar::RegistrarError::Remote(format!("addon: bad reply {e}"))),
        Err(registrar::RegistrarError::Remote(m)) if m.starts_with("unknown op") => Ok(None),
        Err(e) => Err(e),
    }
}

/// Register a registrar addon; returns its id.
pub fn register_registrar(reg: &RegistrarRegistry, addon: Arc<WasmAddon>) -> Result<&'static str, WasmError> {
    let r = WasmRegistrar::new(addon)?;
    let id = r.id;
    reg.register_dyn(Arc::new(r));
    Ok(id)
}
