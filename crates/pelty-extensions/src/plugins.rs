//! Plugin model. Built-in plugins implement [`Plugin`] and are compiled in.
//! External plugins are declared by `extensions/plugins/<id>/plugin.json` and run
//! out-of-process (signed HTTP webhooks, see `make docs`). No native
//! dynamic library loading, ever.
use crate::events::{is_domain_event, DomainEvent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};

pub const API_VERSION: u32 = 1;

/// Capabilities a plugin may request. Unknown ones are rejected.
pub const CAPABILITIES: &[&str] = &[
    "events.read",
    "users.read",
    "invoices.read",
    "services.read",
    "admin.settings",
    "ui.embed",
    "ui.core_actions",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    /// External plugins only: HTTPS endpoint receiving signed events.
    #[serde(default)]
    pub webhook_url: Option<String>,
    /// Optional JSON-schema-like description of admin settings.
    #[serde(default)]
    pub settings_schema: Option<serde_json::Value>,
    /// UI contributions rendered by the frontend in named slots.
    #[serde(default)]
    pub ui: PluginUi,
}

/// Named places in the UI that plugins can contribute to.
pub const UI_SLOTS: &[&str] = &[
    "client.nav",
    "admin.nav",
    "client.dashboard",
    "admin.dashboard",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginUi {
    #[serde(default)]
    pub menu: Vec<UiEntry>,
    #[serde(default)]
    pub widgets: Vec<UiEntry>,
    /// Native pages (declarative node trees), see `ui_schema`.
    #[serde(default)]
    pub pages: Vec<crate::ui_schema::UiPage>,
    /// Node trees rendered inside named app slots.
    #[serde(default)]
    pub slots: Vec<crate::ui_schema::UiSlotContribution>,
}

/// A menu link or dashboard widget. `label` is a translation key that must
/// live in the plugin's namespace (`plugin.<id>.*`). `path` is an in-app
/// route for menu entries; for widgets it is a JSON endpoint under
/// `/api/v1/plugins/<id>/` that returns `{ "title"?: key, "value": string }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UiEntry {
    pub slot: String,
    pub label: String,
    pub path: String,
    /// Optional permission required to see the entry (e.g. `admin.access`).
    #[serde(default)]
    pub permission: Option<String>,
}

fn safe_path(p: &str) -> bool {
    p.starts_with('/')
        && !p.starts_with("//")
        && p.len() <= 200
        && !p.split('/').any(|seg| seg == ".." || seg == ".")
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_/.".contains(c))
}

/// Settings schema subset we support: `{ "type": "object", "properties":
/// { name: { "type": "string"|"boolean"|"number"|"integer", "title"?: key,
/// "secret"?: bool } } }`.
pub fn validate_settings_schema(schema: &serde_json::Value) -> Result<(), String> {
    if schema.get("type").and_then(|t| t.as_str()) != Some("object") {
        return Err("settings_schema.type must be object".into());
    }
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .ok_or("settings_schema.properties required")?;
    for (k, v) in props {
        if !crate::valid_id(k) && !k.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(format!("bad setting name {k}"));
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("string" | "boolean" | "number" | "integer") => {}
            _ => return Err(format!("bad type for setting {k}")),
        }
    }
    Ok(())
}

/// Validate a settings object against a plugin's schema: no unknown keys,
/// matching primitive types.
pub fn validate_settings(
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), String> {
    let obj = value.as_object().ok_or("settings must be an object")?;
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .ok_or("no settings")?;
    for (k, v) in obj {
        let ty = props
            .get(k)
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str())
            .ok_or(format!("unknown setting {k}"))?;
        let ok = match ty {
            "string" => v.is_string() && v.as_str().is_some_and(|s| s.len() <= 2000),
            "boolean" => v.is_boolean(),
            "number" => v.is_number(),
            "integer" => v.is_i64() || v.is_u64(),
            _ => false,
        };
        if !ok {
            return Err(format!("invalid value for {k}"));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("invalid plugin manifest: {0}")]
pub struct ManifestError(pub String);

impl PluginManifest {
    pub fn parse(raw: &str) -> Result<Self, ManifestError> {
        let m: PluginManifest =
            serde_json::from_str(raw).map_err(|e| ManifestError(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        let err = |s: String| Err(ManifestError(s));
        if !crate::valid_id(&self.id) {
            return err("id".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 80 || self.version.len() > 32 {
            return err("name/version".into());
        }
        if self.api_version != API_VERSION {
            return err(format!(
                "unsupported api_version {} (expected {API_VERSION})",
                self.api_version
            ));
        }
        if let Some(c) = self
            .capabilities
            .iter()
            .find(|c| !CAPABILITIES.contains(&c.as_str()))
        {
            return err(format!("unknown capability {c}"));
        }
        if let Some(h) = self.hooks.iter().find(|h| *h != "*" && !is_domain_event(h)) {
            return err(format!("unknown hook {h}"));
        }
        if !self.hooks.is_empty() && !self.capabilities.iter().any(|c| c == "events.read") {
            return err("hooks require the events.read capability".into());
        }
        if let Some(u) = &self.webhook_url {
            if !(u.starts_with("https://")
                || u.starts_with("http://127.0.0.1")
                || u.starts_with("http://localhost"))
            {
                return err("webhook_url must be https".into());
            }
        }
        if let Some(schema) = &self.settings_schema {
            validate_settings_schema(schema).map_err(ManifestError)?;
        }
        let ns = format!("plugin.{}.", self.id);
        for (e, widget) in self
            .ui
            .menu
            .iter()
            .map(|e| (e, false))
            .chain(self.ui.widgets.iter().map(|e| (e, true)))
        {
            if !UI_SLOTS.contains(&e.slot.as_str()) {
                return err(format!("unknown ui slot {}", e.slot));
            }
            if widget != e.slot.ends_with(".dashboard") {
                return err(format!(
                    "{} entries cannot use slot {}",
                    if widget { "widget" } else { "menu" },
                    e.slot
                ));
            }
            if !e.label.starts_with(&ns) {
                return err(format!("ui label {} must be in namespace {ns}*", e.label));
            }
            if !safe_path(&e.path)
                || (widget && !e.path.starts_with(&format!("/api/v1/plugins/{}/", self.id)))
            {
                return err(format!("unsafe ui path {}", e.path));
            }
        }
        if self.ui.menu.len() + self.ui.widgets.len() > 20 {
            return err("too many ui entries".into());
        }
        let rules = crate::ui_schema::UiRules {
            plugin_id: &self.id,
            allow_embed: self.capabilities.iter().any(|c| c == "ui.embed"),
            allow_core_actions: self.capabilities.iter().any(|c| c == "ui.core_actions"),
            own_api: None,
            allow_iframe: false,
        };
        if self.ui.pages.len() > 30 || self.ui.slots.len() > 30 {
            return err("too many ui pages or slots".into());
        }
        let mut seen = HashSet::new();
        for p in &self.ui.pages {
            if !seen.insert(&p.id) {
                return err(format!("duplicate page id {}", p.id));
            }
            crate::ui_schema::validate_page(&rules, p)
                .map_err(|e| ManifestError(format!("page {}: {e}", p.id)))?;
        }
        for sl in &self.ui.slots {
            crate::ui_schema::validate_slot(&rules, sl)
                .map_err(|e| ManifestError(format!("slot {}: {e}", sl.slot)))?;
        }
        Ok(())
    }

    fn subscribes(&self, event: &str) -> bool {
        self.hooks.iter().any(|h| h == "*" || h == event)
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A compiled-in plugin.
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn on_event<'a>(&'a self, event: &'a DomainEvent) -> BoxFuture<'a, Result<(), String>>;
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    #[serde(flatten)]
    pub manifest: PluginManifest,
    pub kind: &'static str,
    pub enabled: bool,
}

/// Registry + hook bus.
#[derive(Default)]
pub struct PluginRegistry {
    builtin: BTreeMap<String, Arc<dyn Plugin>>,
    external: BTreeMap<String, PluginManifest>,
    enabled: RwLock<HashSet<String>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, p: Arc<dyn Plugin>) {
        let id = p.manifest().id.clone();
        self.builtin.insert(id, p);
    }

    /// Load external plugin manifests (invalid ones are skipped with a warning).
    pub fn load_external(&mut self, dir: &Path) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let mf = e.path().join("plugin.json");
            let Ok(raw) = std::fs::read_to_string(&mf) else {
                continue;
            };
            match PluginManifest::parse(&raw) {
                Ok(m) if self.builtin.contains_key(&m.id) => {
                    tracing::warn!(id = %m.id, "external plugin id clashes with built-in")
                }
                Ok(m) => {
                    self.external.insert(m.id.clone(), m);
                }
                Err(err) => tracing::warn!(path = %mf.display(), %err, "invalid plugin skipped"),
            }
        }
    }

    pub fn exists(&self, id: &str) -> bool {
        self.builtin.contains_key(id) || self.external.contains_key(id)
    }

    pub fn set_enabled(&self, id: &str, on: bool) {
        if let Ok(mut s) = self.enabled.write() {
            if on {
                s.insert(id.to_string());
            } else {
                s.remove(id);
            }
        }
    }

    fn is_enabled(&self, id: &str) -> bool {
        self.enabled.read().map(|s| s.contains(id)).unwrap_or(false)
    }

    pub fn manifest(&self, id: &str) -> Option<PluginManifest> {
        self.builtin
            .get(id)
            .map(|p| p.manifest().clone())
            .or_else(|| self.external.get(id).cloned())
    }

    pub fn enabled(&self, id: &str) -> bool {
        self.is_enabled(id)
    }

    /// UI entries of enabled plugins, tagged with the plugin id.
    pub fn ui_entries(&self) -> Vec<(String, bool, UiEntry)> {
        self.list()
            .into_iter()
            .filter(|p| p.enabled)
            .flat_map(|p| {
                let id = p.manifest.id.clone();
                let menu = p.manifest.ui.menu.into_iter().map({
                    let id = id.clone();
                    move |e| (id.clone(), false, e)
                });
                let widgets = p
                    .manifest
                    .ui
                    .widgets
                    .into_iter()
                    .map(move |e| (id.clone(), true, e));
                menu.chain(widgets).collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let b = self.builtin.values().map(|p| PluginInfo {
            manifest: p.manifest().clone(),
            kind: "builtin",
            enabled: self.is_enabled(&p.manifest().id),
        });
        let e = self.external.values().map(|m| PluginInfo {
            manifest: m.clone(),
            kind: "external",
            enabled: self.is_enabled(&m.id),
        });
        b.chain(e).collect()
    }

    /// Enabled external plugins subscribed to `event` that declare a
    /// `webhook_url`: (plugin id, manifest url). Delivery is durable and
    /// lives in `pelty-api::plugin_hooks`.
    pub fn webhook_targets(&self, event: &str) -> Vec<(String, String)> {
        self.external
            .values()
            .filter(|m| self.is_enabled(&m.id) && m.subscribes(event))
            .filter_map(|m| m.webhook_url.clone().map(|u| (m.id.clone(), u)))
            .collect()
    }

    /// Deliver an event to every enabled built-in subscriber. Returns how many
    /// handlers ran. Handler errors are logged, never propagated.
    /// External plugins get durable signed webhooks (see `webhook_targets`).
    pub async fn dispatch(&self, event: &DomainEvent) -> usize {
        let mut n = 0;
        for p in self.builtin.values() {
            let m = p.manifest();
            if self.is_enabled(&m.id) && m.subscribes(event.name) {
                n += 1;
                if let Err(e) = p.on_event(event).await {
                    tracing::warn!(plugin = %m.id, event = event.name, error = %e, "plugin hook failed");
                }
            }
        }
        n
    }
}

/// Example built-in plugin: logs events and keeps the last 100 in memory.
pub struct EventLogger {
    manifest: PluginManifest,
    recent: Mutex<Vec<DomainEvent>>,
}

impl Default for EventLogger {
    fn default() -> Self {
        EventLogger {
            manifest: PluginManifest {
                id: "event-logger".into(),
                name: "Event logger".into(),
                version: "1.0.0".into(),
                api_version: API_VERSION,
                description: "Logs domain events (example plugin).".into(),
                capabilities: vec!["events.read".into()],
                hooks: vec![
                    "user.registered".into(),
                    "user.logged_in".into(),
                    "payment.succeeded".into(),
                ],
                webhook_url: None,
                settings_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "keep": { "type": "integer", "title": "plugin.event-logger.settings.keep" } }
                })),
                ui: PluginUi {
                    menu: vec![UiEntry {
                        slot: "admin.nav".into(),
                        label: "plugin.event-logger.menu".into(),
                        path: "/admin/plugins/event-logger".into(),
                        permission: Some("plugins.manage".into()),
                    }],
                    widgets: vec![UiEntry {
                        slot: "admin.dashboard".into(),
                        label: "plugin.event-logger.widget".into(),
                        path: "/api/v1/plugins/event-logger/summary".into(),
                        permission: Some("plugins.manage".into()),
                    }],
                    ..Default::default()
                },
            },
            recent: Mutex::new(Vec::new()),
        }
    }
}

impl EventLogger {
    pub fn recent(&self) -> Vec<DomainEvent> {
        self.recent.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl Plugin for EventLogger {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_event<'a>(&'a self, event: &'a DomainEvent) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            tracing::info!(event = event.name, "event-logger received event");
            let mut v = self.recent.lock().map_err(|_| "poisoned".to_string())?;
            if v.len() >= 100 {
                v.remove(0);
            }
            v.push(event.clone());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn hook_bus_delivers_to_enabled_subscribers() {
        let logger = Arc::new(EventLogger::default());
        let mut reg = PluginRegistry::new();
        reg.register(logger.clone());
        let ev = DomainEvent::new("user.registered", json!({"user_id": "x"}));
        assert_eq!(reg.dispatch(&ev).await, 0, "disabled plugins get nothing");
        reg.set_enabled("event-logger", true);
        assert_eq!(reg.dispatch(&ev).await, 1);
        assert_eq!(
            reg.dispatch(&DomainEvent::new("invoice.paid", json!({})))
                .await,
            0,
            "not subscribed"
        );
        assert_eq!(logger.recent().len(), 1);
        assert_eq!(logger.recent()[0].name, "user.registered");
    }

    #[test]
    fn manifest_validation() {
        let ok = r#"{"id":"slack-notify","name":"Slack","version":"0.1.0","api_version":1,"capabilities":["events.read"],"hooks":["invoice.paid"],"webhook_url":"https://example.com/hook"}"#;
        assert!(PluginManifest::parse(ok).is_ok());
        assert!(
            PluginManifest::parse(&ok.replace("\"api_version\":1", "\"api_version\":9")).is_err()
        );
        assert!(PluginManifest::parse(&ok.replace("invoice.paid", "rm.rf")).is_err());
        assert!(PluginManifest::parse(&ok.replace("events.read", "root")).is_err());
        assert!(PluginManifest::parse(&ok.replace("https://", "http://")).is_err());
        assert!(PluginManifest::parse(&ok.replace("}", ",\"library\":\"x.so\"}")).is_err());
    }

    #[test]
    fn ui_entries_are_namespaced_and_safe() {
        let base = |ui: &str| {
            format!(r#"{{"id":"demo","name":"D","version":"1","api_version":1,"ui":{ui}}}"#)
        };
        let ok = base(
            r#"{"menu":[{"slot":"client.nav","label":"plugin.demo.menu","path":"/demo"}],"widgets":[{"slot":"client.dashboard","label":"plugin.demo.w","path":"/api/v1/plugins/demo/w"}]}"#,
        );
        assert!(
            PluginManifest::parse(&ok).is_ok(),
            "{:?}",
            PluginManifest::parse(&ok)
        );
        for bad in [
            r#"{"menu":[{"slot":"root","label":"plugin.demo.m","path":"/d"}]}"#,
            r#"{"menu":[{"slot":"client.nav","label":"nav.users","path":"/d"}]}"#,
            r#"{"menu":[{"slot":"client.nav","label":"plugin.demo.m","path":"https://evil"}]}"#,
            r#"{"menu":[{"slot":"client.nav","label":"plugin.demo.m","path":"/../x"}]}"#,
            r#"{"widgets":[{"slot":"client.dashboard","label":"plugin.demo.w","path":"/api/v1/auth/me"}]}"#,
            r#"{"menu":[{"slot":"client.dashboard","label":"plugin.demo.m","path":"/d"}]}"#,
        ] {
            assert!(PluginManifest::parse(&base(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn settings_validation() {
        let schema = serde_json::json!({"type":"object","properties":{"keep":{"type":"integer"},"name":{"type":"string"}}});
        assert!(validate_settings(&schema, &serde_json::json!({"keep": 5})).is_ok());
        assert!(validate_settings(&schema, &serde_json::json!({"keep": "5"})).is_err());
        assert!(validate_settings(&schema, &serde_json::json!({"other": 1})).is_err());
        assert!(validate_settings_schema(
            &serde_json::json!({"type":"object","properties":{"x":{"type":"function"}}})
        )
        .is_err());
    }

    #[test]
    fn loads_example_external_manifest() {
        let mut reg = PluginRegistry::new();
        reg.load_external(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extensions/plugins"),
        );
        assert!(reg.exists("example-webhook"));
        let m = reg
            .manifest("example-provisioning")
            .expect("declarative UI example loads");
        assert_eq!(m.ui.pages.len(), 1);
        assert_eq!(m.ui.slots[0].slot, "admin.service.detail");
    }

    #[test]
    fn manifest_ui_pages_are_validated() {
        let base = |ui: serde_json::Value| {
            serde_json::json!({"id": "demo", "name": "Demo", "version": "1", "api_version": 1, "ui": ui}).to_string()
        };
        let page = |body: serde_json::Value| {
            base(
                serde_json::json!({"pages": [{"id": "pg", "area": "admin", "title": "plugin.demo.t", "body": body}]}),
            )
        };
        PluginManifest::parse(&page(serde_json::json!({"type": "text", "text": "hi"}))).unwrap();
        assert!(PluginManifest::parse(&page(serde_json::json!({"type": "iframe"}))).is_err());
        assert!(
            PluginManifest::parse(&page(
                serde_json::json!({"type": "embed", "src": "https://x.example"})
            ))
            .is_err(),
            "embed needs ui.embed"
        );
        let bad_title = base(
            serde_json::json!({"pages": [{"id": "pg", "area": "admin", "title": "other.t", "body": {"type": "text"}}]}),
        );
        assert!(PluginManifest::parse(&bad_title).is_err());
        let bad_slot = base(
            serde_json::json!({"slots": [{"slot": "admin.nowhere", "body": {"type": "text"}}]}),
        );
        assert!(PluginManifest::parse(&bad_slot).is_err());
        let bad_data = base(
            serde_json::json!({"slots": [{"slot": "admin.dashboard", "data": {"x": {"get": "https://evil.example/"}}, "body": {"type": "text"}}]}),
        );
        assert!(PluginManifest::parse(&bad_data).is_err());
    }
}
