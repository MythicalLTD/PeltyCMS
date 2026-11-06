//! Provisioning and registrar contracts: the module traits, registries,
//! HTTP transports and shared helpers. No panel code lives here: every
//! module (Pterodactyl, cPanel, Proxmox, ...) is its own addon crate under
//! `extensions/addons/prov-*`, loaded as WASM. Scheduling, retries and
//! persistence live in the job queue (`pelty-api::worker`).
//! Every module describes its server and product settings with a declarative
//! field schema, so the admin UI renders them natively without module code.
pub mod http;
pub mod licensing;
pub mod registrar;
pub mod registrars;
pub mod sso;
pub mod template;
pub mod util;
pub mod wire;

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    /// Bad module/product/server configuration (not retryable).
    #[error("configuration: {0}")]
    Config(String),
    /// The panel rejected the request (4xx; usually not retryable).
    #[error("remote rejected request ({status}): {message}")]
    Rejected { status: u16, message: String },
    /// Network/5xx; safe to retry with backoff.
    #[error("transient: {0}")]
    Transient(String),
}

impl ProvisionError {
    pub fn retryable(&self) -> bool {
        matches!(self, ProvisionError::Transient(_))
    }
}

pub type ProvisionResult<T> = Result<T, ProvisionError>;

/// Connection to one panel instance (a row in `servers`).
#[derive(Debug, Clone)]
pub struct ServerConn {
    pub base_url: String,
    /// Decrypted API key; never logged or serialized.
    pub api_key: String,
    pub config: Value,
}

/// Everything a module needs to act on one service.
#[derive(Debug, Clone, Default)]
pub struct ServiceCtx {
    pub service_id: String,
    pub customer_email: String,
    pub customer_first_name: String,
    pub customer_last_name: String,
    /// Product `module_config` (package definition: limits, egg, image…).
    pub product_config: Value,
    pub custom_fields: Value,
    /// Remote id from a previous `create` (None before creation).
    pub remote_id: Option<String>,
    /// Module-owned state persisted between calls.
    pub module_data: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Created {
    pub remote_id: String,
    pub module_data: Value,
}

/// A declarative settings field (rendered natively by the admin UI).
#[derive(Debug, Clone, Serialize)]
pub struct FieldSpec {
    pub key: &'static str,
    /// Translation key for the label.
    pub label: &'static str,
    /// text | number | password | textarea | json
    pub kind: &'static str,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub server_fields: Vec<FieldSpec>,
    pub product_fields: Vec<FieldSpec>,
    /// Extra admin/client buttons (`reboot`, `console`, `reset_password`…),
    /// filled from [`ProvisioningModule::custom_actions`] by the registry.
    #[serde(default)]
    pub actions: Vec<&'static str>,
}

/// A field the client fills in at checkout for products using this module
/// (e.g. a hostname or a CSR). Synced into the product's custom fields; the
/// value reaches the module as `ServiceCtx::custom_fields[key]`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckoutField {
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// text | textarea | number | select | checkbox
    pub kind: &'static str,
    pub choices: &'static [&'static str],
    /// Optional POSIX regex the value must match.
    pub pattern: &'static str,
    pub required: bool,
}

/// Owned checkout field returned by [`ProvisioningModule::checkout_config`].
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DynamicCheckoutField {
    pub key: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub choices: Vec<String>,
    pub pattern: String,
    pub required: bool,
}

impl From<&CheckoutField> for DynamicCheckoutField {
    fn from(f: &CheckoutField) -> Self {
        Self {
            key: f.key.into(),
            name: f.name.into(),
            description: f.description.into(),
            kind: f.kind.into(),
            choices: f.choices.iter().map(|c| (*c).to_string()).collect(),
            pattern: f.pattern.into(),
            required: f.required,
        }
    }
}

pub trait ProvisioningModule: Send + Sync {
    fn info(&self) -> ModuleInfo;
    /// Fields asked at checkout for this module's products.
    fn checkout_fields(&self) -> Vec<CheckoutField> {
        Vec::new()
    }
    /// Dynamic checkout form for the current selection (WHMCS-style
    /// `getCheckoutConfig`). Default returns [`checkout_fields`]; modules
    /// may override to filter/reshape from `product_config` / `selected`.
    fn checkout_config(
        &self,
        _product_config: &Value,
        _selected: &Value,
    ) -> Vec<DynamicCheckoutField> {
        self.checkout_fields()
            .into_iter()
            .map(|f| DynamicCheckoutField::from(&f))
            .collect()
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>>;
    fn create<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Created>>;
    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>>;
    fn unsuspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>>;
    fn terminate<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>>;
    /// Apply the (new) product config in `svc.product_config` to the remote.
    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>>;
    /// Client single sign-on: the URL to send the client to, carrying the
    /// signed one-time `token` (see [`sso`]). `None` means unsupported. The
    /// default uses the server's `sso_url` template; modules may override it
    /// (e.g. to exchange the token for a panel-native login link).
    fn sso_url<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        token: &'a str,
    ) -> BoxFuture<'a, ProvisionResult<Option<String>>> {
        Box::pin(async move { Ok(sso::template_url(server, svc, token)) })
    }
    /// Names of extra actions this module supports (custom buttons).
    fn custom_actions(&self) -> &'static [&'static str] {
        &[]
    }
    /// Run one extra action. `params` carries user input (e.g. a new
    /// password or an image id). Returns module-specific output (a console
    /// URL, a new password…) to show once.
    fn custom<'a>(
        &'a self,
        _server: &'a ServerConn,
        _svc: &'a ServiceCtx,
        action: &'a str,
        _params: &'a Value,
    ) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            Err(ProvisionError::Config(format!(
                "action {action} is not supported by this module"
            )))
        })
    }
    /// Live resource usage / status for the client area (`None` = unsupported).
    fn usage<'a>(
        &'a self,
        _server: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move { Ok(None) })
    }
    /// Accounts that exist on the server, for import/sync (`None` = unsupported).
    fn list_accounts<'a>(
        &'a self,
        _server: &'a ServerConn,
    ) -> BoxFuture<'a, ProvisionResult<Option<Vec<RemoteAccount>>>> {
        Box::pin(async move { Ok(None) })
    }
    /// Vendor resale (MarketConnect-style): products this vendor sells,
    /// with the reseller's cost. `None` = not a vendor module.
    fn catalog<'a>(
        &'a self,
        _server: &'a ServerConn,
    ) -> BoxFuture<'a, ProvisionResult<Option<Vec<VendorProduct>>>> {
        Box::pin(async move { Ok(None) })
    }
}

/// One product in a vendor catalog (see [`ProvisioningModule::catalog`]).
/// `cost` is a decimal string in `currency` per `cycle`; `product_config`
/// is copied into the imported product's module config.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct VendorProduct {
    pub sku: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub cost: String,
    pub currency: String,
    pub cycle: String,
    #[serde(default)]
    pub product_config: serde_json::Value,
}

/// An account found on a provisioning server (see [`ProvisioningModule::list_accounts`]).
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RemoteAccount {
    pub remote_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub package: String,
    /// `active` or `suspended`.
    #[serde(default)]
    pub status: String,
}

/// Run a custom action after checking the module declares it.
pub async fn run_custom(
    m: &dyn ProvisioningModule,
    action: &str,
    server: &ServerConn,
    svc: &ServiceCtx,
    params: &Value,
) -> ProvisionResult<Value> {
    if !m.custom_actions().contains(&action) {
        return Err(ProvisionError::Config(format!(
            "action {action} is not supported by this module"
        )));
    }
    m.custom(server, svc, action, params).await
}

pub const ACTIONS: &[&str] = &[
    "create",
    "suspend",
    "unsuspend",
    "terminate",
    "change_package",
];

/// Run one named action. `create` returns `Some(Created)`.
pub async fn run(
    m: &dyn ProvisioningModule,
    action: &str,
    server: &ServerConn,
    svc: &ServiceCtx,
) -> ProvisionResult<Option<Created>> {
    match action {
        "create" => m.create(server, svc).await.map(Some),
        "suspend" => m.suspend(server, svc).await.map(|_| None),
        "unsuspend" => m.unsuspend(server, svc).await.map(|_| None),
        "terminate" => m.terminate(server, svc).await.map(|_| None),
        "change_package" => m.change_package(server, svc).await.map(|_| None),
        other => Err(ProvisionError::Config(format!("unknown action {other}"))),
    }
}

#[derive(Default, Clone)]
pub struct Registry {
    /// Shared between clones so addons (un)registered at runtime are seen by
    /// every holder of the registry.
    modules: Arc<std::sync::RwLock<BTreeMap<String, Arc<dyn ProvisioningModule>>>>,
}

impl Registry {
    pub fn register(&mut self, m: Arc<dyn ProvisioningModule>) {
        self.register_dyn(m);
    }
    /// Registers (or replaces) a module through a shared reference.
    pub fn register_dyn(&self, m: Arc<dyn ProvisioningModule>) {
        let id = m.info().id.to_string();
        self.modules
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, m);
    }
    pub fn unregister(&self, id: &str) -> bool {
        self.modules
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
            .is_some()
    }
    pub fn get(&self, id: &str) -> Option<Arc<dyn ProvisioningModule>> {
        self.modules
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }
    pub fn list(&self) -> Vec<ModuleInfo> {
        self.modules
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|m| {
                let mut i = m.info();
                i.actions = m.custom_actions().to_vec();
                // Every module can use the generic SSO redirect template.
                if !i.server_fields.iter().any(|f| f.key == "sso_url") {
                    i.server_fields.push(FieldSpec {
                        key: "sso_url",
                        label: "provisioning.field.sso_url",
                        kind: "text",
                        required: false,
                        default: None,
                    });
                }
                i
            })
            .collect()
    }
}

#[doc(hidden)]
pub fn remote_id(svc: &ServiceCtx) -> ProvisionResult<&str> {
    svc.remote_id
        .as_deref()
        .ok_or_else(|| ProvisionError::Config("service has no remote id; create it first".into()))
}

#[doc(hidden)]
pub fn cfg_u64(v: &Value, key: &str) -> ProvisionResult<u64> {
    match v.get(key) {
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| ProvisionError::Config(format!("{key} must be a non-negative integer"))),
        Some(Value::String(s)) => s
            .trim()
            .parse()
            .map_err(|_| ProvisionError::Config(format!("{key} must be a non-negative integer"))),
        _ => Err(ProvisionError::Config(format!("missing {key}"))),
    }
}

#[doc(hidden)]
pub fn cfg_u64_or(v: &Value, key: &str, default: u64) -> ProvisionResult<u64> {
    if v.get(key).is_none_or(|x| x.is_null() || x == "") {
        Ok(default)
    } else {
        cfg_u64(v, key)
    }
}

#[doc(hidden)]
pub fn cfg_str<'a>(v: &'a Value, key: &str) -> ProvisionResult<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ProvisionError::Config(format!("missing {key}")))
}

/// Panel usernames: lowercase alnum/underscore derived from the email, plus a
/// short service suffix for uniqueness.
#[doc(hidden)]
pub fn username(email: &str, service_id: &str) -> String {
    let local: String = email
        .split('@')
        .next()
        .unwrap_or("user")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(20)
        .collect::<String>()
        .to_ascii_lowercase();
    let local = if local.len() < 3 {
        "user".to_string()
    } else {
        local
    };
    let suffix: String = service_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(6)
        .collect();
    format!("{local}_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_safe() {
        assert_eq!(
            username("John.Doe+x@example.com", "ab12-cd34-ef"),
            "johndoex_ab12cd"
        );
        assert_eq!(username("a@b.c", "123456789"), "user_123456");
    }

    #[test]
    fn config_numbers_accept_strings() {
        let v = serde_json::json!({"memory": "1024", "disk": 2048, "bad": -1});
        assert_eq!(cfg_u64(&v, "memory").unwrap(), 1024);
        assert_eq!(cfg_u64(&v, "disk").unwrap(), 2048);
        assert!(cfg_u64(&v, "bad").is_err());
        assert_eq!(cfg_u64_or(&v, "swap", 0).unwrap(), 0);
    }
}

#[cfg(test)]
mod checkout_config_tests {
    use super::*;
    use serde_json::json;

    struct Demo;
    impl ProvisioningModule for Demo {
        fn info(&self) -> ModuleInfo {
            ModuleInfo {
                id: "demo",
                name: "Demo",
                server_fields: vec![],
                product_fields: vec![],
                actions: vec![],
            }
        }
        fn checkout_fields(&self) -> Vec<CheckoutField> {
            vec![
                CheckoutField {
                    key: "os",
                    name: "OS",
                    description: "",
                    kind: "select",
                    choices: &["linux", "windows"],
                    pattern: "",
                    required: true,
                },
                CheckoutField {
                    key: "license",
                    name: "Windows license",
                    description: "",
                    kind: "text",
                    choices: &[],
                    pattern: "",
                    required: true,
                },
            ]
        }
        fn checkout_config(&self, _: &Value, selected: &Value) -> Vec<DynamicCheckoutField> {
            self.checkout_fields()
                .into_iter()
                .filter(|f| {
                    f.key != "license"
                        || selected.get("os").and_then(|v| v.as_str()) == Some("windows")
                })
                .map(|f| DynamicCheckoutField::from(&f))
                .collect()
        }
        fn test_connection<'a>(&'a self, _: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
            Box::pin(async { Ok(()) })
        }
        fn create<'a>(
            &'a self,
            _: &'a ServerConn,
            _: &'a ServiceCtx,
        ) -> BoxFuture<'a, ProvisionResult<Created>> {
            Box::pin(async { Err(ProvisionError::Config("n/a".into())) })
        }
        fn suspend<'a>(
            &'a self,
            _: &'a ServerConn,
            _: &'a ServiceCtx,
        ) -> BoxFuture<'a, ProvisionResult<()>> {
            Box::pin(async { Ok(()) })
        }
        fn unsuspend<'a>(
            &'a self,
            _: &'a ServerConn,
            _: &'a ServiceCtx,
        ) -> BoxFuture<'a, ProvisionResult<()>> {
            Box::pin(async { Ok(()) })
        }
        fn terminate<'a>(
            &'a self,
            _: &'a ServerConn,
            _: &'a ServiceCtx,
        ) -> BoxFuture<'a, ProvisionResult<()>> {
            Box::pin(async { Ok(()) })
        }
        fn change_package<'a>(
            &'a self,
            _: &'a ServerConn,
            _: &'a ServiceCtx,
        ) -> BoxFuture<'a, ProvisionResult<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn visible_when_filters_license() {
        let m = Demo;
        let linux = m.checkout_config(&json!({}), &json!({"os": "linux"}));
        assert_eq!(
            linux.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(),
            ["os"]
        );
        let win = m.checkout_config(&json!({}), &json!({"os": "windows"}));
        assert_eq!(
            win.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(),
            ["os", "license"]
        );
    }
}
