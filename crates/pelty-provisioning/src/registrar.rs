//! Domain registrar modules. Real registrars implement [`RegistrarModule`];
//! [`MockRegistrar`] is an in-memory implementation for tests and demos and
//! never touches the network.
use crate::BoxFuture;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum RegistrarError {
    #[error("domain unavailable")]
    Unavailable,
    #[error("invalid transfer authorization code")]
    BadAuthCode,
    #[error("unknown domain")]
    UnknownDomain,
    #[error("registrar error: {0}")]
    Remote(String),
}

pub type RegistrarResult<T> = Result<T, RegistrarError>;

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Contact {
    pub name: String,
    #[serde(default)]
    pub organization: String,
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
}

/// Registrar cost price of a TLD (decimal strings, one year).
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TldCost {
    pub tld: String,
    pub currency: String,
    pub register: String,
    pub renew: String,
    pub transfer: String,
}

/// Registry premium price for a name (decimal strings, one year).
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PremiumQuote {
    pub currency: String,
    pub register: String,
    pub renew: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RemoteDomain {
    /// ISO date `YYYY-MM-DD`.
    pub expires_on: String,
    /// `active`, `expired`, `transferring` or `cancelled`.
    pub status: String,
    pub nameservers: Vec<String>,
}

/// Custom domain fields as flat `(key, value)` registrar parameters (pure).
/// Only scalar values and safe keys pass; keys in `reserved` (compared
/// case-insensitively) are dropped so a field can't override credentials
/// or core parameters.
pub fn field_params(fields: &serde_json::Value, reserved: &[&str]) -> Vec<(String, String)> {
    let Some(o) = fields.as_object() else { return vec![] };
    let mut out: Vec<(String, String)> = o
        .iter()
        .filter(|(k, _)| {
            let ok = !k.is_empty() && k.len() <= 64 && k.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && k.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
            ok && !reserved.iter().any(|r| r.eq_ignore_ascii_case(k))
        })
        .filter_map(|(k, v)| {
            let v = match v {
                serde_json::Value::String(s) => s.trim().to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => if *b { "1".into() } else { "0".into() },
                _ => return None,
            };
            Some((k.clone(), v.chars().take(500).collect()))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod field_tests {
    use super::field_params;
    #[test]
    fn filters_and_flattens() {
        let f = serde_json::json!({"RegistrantNexus": "C11", "cnp": 123, "ok": true, "ApiKey": "x", "bad key": "y", "nested": {"a": 1}, "1x": "z"});
        assert_eq!(field_params(&f, &["apikey"]), vec![("RegistrantNexus".into(), "C11".into()), ("cnp".into(), "123".into()), ("ok".into(), "1".into())]);
        assert!(field_params(&serde_json::Value::Null, &[]).is_empty());
    }
}

pub trait RegistrarModule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>>;
    /// Register for `years`; `expires_on` is what the caller computed.
    fn register<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>>;
    /// Register with the TLD's custom domain fields (`{key: value}` as set
    /// up per TLD in Admin → Domain pricing). Default: fields are ignored.
    fn register_with_fields<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String], fields: &'a serde_json::Value) -> BoxFuture<'a, RegistrarResult<()>> {
        let _ = fields;
        self.register(domain, years, expires_on, registrant, nameservers)
    }
    fn renew<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>>;
    fn transfer<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>>;
    /// Transfer with custom domain fields. Default: fields are ignored.
    fn transfer_with_fields<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str, fields: &'a serde_json::Value) -> BoxFuture<'a, RegistrarResult<()>> {
        let _ = fields;
        self.transfer(domain, auth_code, expires_on)
    }
    fn set_nameservers<'a>(&'a self, domain: &'a str, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>>;
    fn set_contact<'a>(&'a self, domain: &'a str, registrant: &'a Contact) -> BoxFuture<'a, RegistrarResult<()>>;
    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>>;
    /// Transfer-out authorization (EPP) code. `None` = not supported.
    fn epp_code<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async { Ok(None) })
    }
    /// Cost price list for TLD price sync. `None` = not supported.
    fn tld_prices<'a>(&'a self) -> BoxFuture<'a, RegistrarResult<Option<Vec<TldCost>>>> {
        Box::pin(async { Ok(None) })
    }
    /// Premium price of `domain`; `None` = standard pricing (or unsupported).
    fn premium_price<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<PremiumQuote>>> {
        Box::pin(async { Ok(None) })
    }
    /// A copy configured with the registrar addon's settings (API keys…).
    /// `None` = the module needs no settings and `self` is used as is.
    fn configure(&self, _settings: &serde_json::Value) -> Option<Arc<dyn RegistrarModule>> {
        None
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistrarInfo {
    pub id: &'static str,
    pub name: &'static str,
}

#[derive(Clone)]
pub struct RegistrarRegistry {
    modules: Arc<std::sync::RwLock<BTreeMap<String, Arc<dyn RegistrarModule>>>>,
    transport: Arc<dyn crate::registrars::RegTransport>,
}

impl Default for RegistrarRegistry {
    fn default() -> Self {
        RegistrarRegistry { modules: Default::default(), transport: crate::registrars::default_transport() }
    }
}

impl RegistrarRegistry {
    /// Empty registry whose RDAP/WHOIS lookups go through `t` (tests pass a
    /// [`crate::registrars::MockRegTransport`]). Registrars come from addons.
    pub fn with_transport(t: Arc<dyn crate::registrars::RegTransport>) -> Self {
        RegistrarRegistry { modules: Default::default(), transport: t }
    }
    /// Transport for RDAP/WHOIS lookups.
    pub fn transport(&self) -> Arc<dyn crate::registrars::RegTransport> {
        self.transport.clone()
    }
    pub fn register(&mut self, m: Arc<dyn RegistrarModule>) {
        self.register_dyn(m);
    }
    /// Registers (or replaces) a registrar through a shared reference.
    pub fn register_dyn(&self, m: Arc<dyn RegistrarModule>) {
        self.modules.write().unwrap_or_else(|e| e.into_inner()).insert(m.id().to_string(), m);
    }
    pub fn unregister(&self, id: &str) -> bool {
        self.modules.write().unwrap_or_else(|e| e.into_inner()).remove(id).is_some()
    }
    pub fn get(&self, id: &str) -> Option<Arc<dyn RegistrarModule>> {
        self.modules.read().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }
    pub fn list(&self) -> Vec<RegistrarInfo> {
        self.modules.read().unwrap_or_else(|e| e.into_inner()).values().map(|m| RegistrarInfo { id: m.id(), name: m.name() }).collect()
    }
}
