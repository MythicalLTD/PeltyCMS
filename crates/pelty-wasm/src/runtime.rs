//! Generic addon runtime for WIT world `pelty:addon/pelty-addon@0.2.0`.
//!
//! One component per addon; every call gets a fresh store with fuel, a memory
//! cap and a wall-clock deadline. Host imports enforce the capabilities from
//! the manifest (HTTP allowlist, declared secrets, kv opt-in).
use crate::{Sandbox, WasmError};
use pelty_extensions::addon_pkg::{self, Limits, Package};
use pelty_payments::{HttpRequestSpec, HttpTransport};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Engine, Store, StoreLimits};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({ path: "wit/pelty.wit", world: "pelty-addon" });
}
use bindings::pelty::addon::events;
use bindings::pelty::addon::host::{self, HttpRequest, HttpResponse, Level};
use bindings::pelty::addon::power::{self, ExecOutput};

/// ABI implemented by the generic runtime.
pub const ABI: &str = "0.5.0";

/// Manifest `abi` values the host loads: 0.5 (`scheduled` export, cron
/// jobs), 0.4 (`context` interface, filters,
/// core-function mixins), 0.3 (lifecycle phases, custom events, mixins) and
/// 0.2. All share the `pelty:addon@0.2.0` WIT package and only add imports
/// (0.5 adds one optional export), so older components keep linking.
/// Requires a full semantic version: historical 0.2 through 0.4 accept any
/// numeric patch; the current minor accepts only 0.5.0. Prereleases are
/// rejected. Valid build metadata is allowed and ignored for compatibility.
pub fn abi_supported(abi: &str) -> bool {
    let Ok(version) = semver::Version::parse(abi) else {
        return false;
    };
    version.major == 0
        && version.pre.is_empty()
        && match version.minor {
            2..=4 => true,
            5 => version.patch == 0,
            _ => false,
        }
}
/// Addon kinds served by the generic world.
pub const KINDS: &[&str] = &[
    "gateway",
    "provisioning",
    "registrar",
    "captcha",
    "sso",
    "integration",
];
/// Max outbound request / response body.
pub const HTTP_MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_HTTP_CALLS: usize = 64;
/// Power calls (db/sql/exec/jobs) allowed in one operation.
const MAX_POWER_CALLS: usize = 256;

/// Addon-private key/value storage (DB-backed in the server, memory in
/// tests). Async so a DB store runs on the host runtime; the host blocks on
/// it from the addon's (blocking) call thread.
pub trait KvStore: Send + Sync {
    fn get<'a>(
        &'a self,
        addon: &'a str,
        key: &'a str,
    ) -> pelty_payments::BoxFuture<'a, Option<String>>;
    fn set<'a>(
        &'a self,
        addon: &'a str,
        key: &'a str,
        value: &'a str,
    ) -> pelty_payments::BoxFuture<'a, Result<(), String>>;
    fn delete<'a>(&'a self, addon: &'a str, key: &'a str) -> pelty_payments::BoxFuture<'a, ()>;
}

#[derive(Default)]
pub struct MemoryKv(Mutex<HashMap<(String, String), String>>);
impl KvStore for MemoryKv {
    fn get<'a>(&'a self, a: &'a str, k: &'a str) -> pelty_payments::BoxFuture<'a, Option<String>> {
        let v = self
            .0
            .lock()
            .ok()
            .and_then(|m| m.get(&(a.into(), k.into())).cloned());
        Box::pin(async move { v })
    }
    fn set<'a>(
        &'a self,
        a: &'a str,
        k: &'a str,
        v: &'a str,
    ) -> pelty_payments::BoxFuture<'a, Result<(), String>> {
        let r = self
            .0
            .lock()
            .map_err(|_| "kv poisoned".to_string())
            .map(|mut m| {
                m.insert((a.into(), k.into()), v.into());
            });
        Box::pin(async move { r })
    }
    fn delete<'a>(&'a self, a: &'a str, k: &'a str) -> pelty_payments::BoxFuture<'a, ()> {
        if let Ok(mut m) = self.0.lock() {
            m.remove(&(a.to_string(), k.to_string()));
        }
        Box::pin(async {})
    }
}

/// Capabilities approved at install (from `addon.json`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Allowed hosts: `api.stripe.com`, `*.example.com`, `$server`, `$setting:<key>`.
    #[serde(default)]
    pub http: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub kv: bool,
    #[serde(default)]
    pub events: Vec<String>,
    /// Synchronous pre-action hooks (`invoice.before_create`, …).
    #[serde(default)]
    pub hooks: Vec<Hook>,
    /// HTTP routes served under `/api/v1/addons/<id>/…`.
    #[serde(default)]
    pub routes: Vec<Route>,
    /// UI powers beyond the declarative schema: `iframe` = sandboxed pages
    /// and widgets served from the addon's `assets/` (admin-approved).
    #[serde(default)]
    pub ui: Vec<String>,
    /// Core `GET /api/v1/…` prefixes an iframe may read through the
    /// postMessage bridge (with the viewer's own permissions).
    #[serde(default)]
    pub bridge: Vec<String>,
    /// `http: ["*"]` may also reach loopback/private/LAN addresses.
    /// Link-local and cloud metadata stay refused.
    #[serde(default)]
    pub http_private: bool,
    /// Change money amounts in pre-event payloads (audited, exact Decimal).
    #[serde(default)]
    pub money: MoneyCaps,
    /// Core data access through the host (`power` interface).
    #[serde(default)]
    pub db: DbCaps,
    /// Private data directory, mounted at `/data` (WASI preopen).
    #[serde(default)]
    pub fs: bool,
    /// Absolute program paths the addon may run (no shell). Off in Docker.
    #[serde(default)]
    pub exec: Vec<String>,
    /// Optional per-program argument allowlist: each argument must equal a
    /// pattern or match a `prefix*` pattern. Programs not listed only get the
    /// baseline checks in [`validate_exec_args`].
    #[serde(default)]
    pub exec_args: HashMap<String, Vec<String>>,
    /// Size cap for the `/data` directory in MiB (default 64, max 1024).
    /// Over quota the directory is mounted read-only.
    #[serde(default)]
    pub fs_quota_mb: Option<u32>,
    /// Background jobs: ops the addon may queue, and cron schedules.
    #[serde(default)]
    pub jobs: Vec<Job>,
    /// Cron jobs (ABI 0.5): the host calls the `on-cron(name)` export on
    /// each matching minute (company time zone). Needs admin approval
    /// (`cron`); the schedule can be changed in Admin > Automation.
    #[serde(default)]
    pub cron: Vec<CronJob>,
    /// The addon registers cron jobs at runtime (`power.cron-register`).
    /// Also needs the `cron` approval.
    #[serde(default)]
    pub cron_runtime: bool,
    /// Custom login: the host asks the addon to verify credentials
    /// (op `auth.verify`). It never sees stored password hashes.
    #[serde(default)]
    pub auth_provider: bool,
    /// Custom events this addon may emit (`<id>:<name>`, `*` suffix allowed).
    #[serde(default)]
    pub emits: Vec<String>,
    /// Filter points this addon applies through WIT `filters.apply`
    /// (`<own-id>:<point>`); other addons subscribe in `filters`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filter_points: Vec<String>,
    /// Host-enforced wrappers around approved host operations.
    #[serde(default)]
    pub mixins: Vec<Mixin>,
    /// Value filters (`catalog.public`, `email.content`, …): the host calls
    /// op `filter` with `{filter, value, context}` and keeps the returned
    /// `value` when it has the same JSON schema. Always fail-open.
    #[serde(default)]
    pub filters: Vec<Filter>,
}

/// A filter subscription (see `pelty_extensions::filters::FILTERS`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub name: String,
    /// Lower runs first (each filter sees the previous one's output).
    #[serde(default = "default_priority", deserialize_with = "de_priority")]
    pub priority: i32,
    #[serde(default = "default_hook_ms")]
    pub timeout_ms: u64,
    /// Fuel budget for one call (capped by the sandbox fuel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
}

/// A mixin: wraps an approved host operation (an API route template such
/// as `POST /api/v1/client/orders`). `at`: `before` (rewrite the input),
/// `after` (rewrite the output within its schema), `around` (both, with
/// shared state) or `return` (replace the output, same schema).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Mixin {
    pub target: String,
    pub at: String,
    #[serde(default, deserialize_with = "de_priority")]
    pub priority: i32,
    #[serde(default = "default_hook_ms")]
    pub timeout_ms: u64,
    /// Fuel budget for one call (capped by the sandbox fuel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MoneyCaps {
    #[serde(default)]
    pub modify: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DbCaps {
    /// Entities: `clients`, `services`, `invoices`, `orders`, `tickets`, …
    #[serde(default)]
    pub read: Vec<String>,
    /// Typed write ops: `client.note`, `ticket.reply`, …
    #[serde(default)]
    pub write: Vec<String>,
    /// Free SQL on `addon_<id>_*` tables.
    #[serde(default)]
    pub sql: bool,
}

/// A background job op; `every_secs` > 0 also runs it on a schedule.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Job {
    pub op: String,
    #[serde(default)]
    pub every_secs: u64,
}

/// A cron job declared in `capabilities.cron`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CronJob {
    pub name: String,
    /// Five-field cron expression (company time zone).
    pub schedule: String,
    #[serde(default)]
    pub description: Option<String>,
}

impl Capabilities {
    pub fn iframe(&self) -> bool {
        self.ui
            .iter()
            .any(|u| u == "iframe" || u == "iframe.same_origin")
    }
    /// `ui: ["override"]`: the addon's pages may replace core pages and hide
    /// core sidebar entries (manifest `ui.overrides`).
    pub fn ui_override(&self) -> bool {
        self.ui.iter().any(|u| u == "override")
    }
    /// Frames get `allow-same-origin` (they can then call the API as the viewer).
    pub fn iframe_same_origin(&self) -> bool {
        self.ui.iter().any(|u| u == "iframe.same_origin")
    }
    /// Capabilities shown with a red warning at install; the admin must tick
    /// each one (`approve` list in the install request).
    pub fn dangerous(&self) -> Vec<&'static str> {
        let mut v = vec![];
        if self.iframe() {
            v.push("ui.iframe");
        }
        if self.iframe_same_origin() {
            v.push("ui.iframe.same_origin");
        }
        if self.ui_override() {
            v.push("ui.override");
        }
        if self.http.iter().any(|h| h == "*") {
            v.push("http.any");
        }
        if self.http_private {
            v.push("http.private");
        }
        if self.money.modify {
            v.push("money.modify");
        }
        if !self.db.read.is_empty() {
            v.push("db.read");
        }
        if !self.db.write.is_empty() {
            v.push("db.write");
        }
        if self.db.sql {
            v.push("db.sql");
        }
        if self.fs {
            v.push("fs");
        }
        if !self.cron.is_empty() || self.cron_runtime {
            v.push("cron");
        }
        if !self.exec.is_empty() {
            v.push("exec");
        }
        if self.auth_provider {
            v.push("auth.provider");
        }
        if self
            .hooks
            .iter()
            .any(|h| is_request_pattern(split_phase(&h.event).1))
        {
            v.push("hooks.any");
        }
        if !self.mixins.is_empty() {
            v.push("mixins");
        }
        // Filters rewrite what users see (mails, catalog, UI): approve at install.
        if !self.filters.is_empty() {
            v.push("filters");
        }
        if self
            .mixins
            .iter()
            .any(|m| m.at == "return" || m.at == "around")
        {
            v.push("mixins.return");
        }
        v
    }
}

/// Database, exec and job services behind the `power` interface. The
/// runtime checks capabilities before every call; implementations enforce
/// scopes again (defence in depth).
pub trait PowerHost: Send + Sync {
    fn db_read<'a>(
        &'a self,
        addon: &'a str,
        entity: &'a str,
        query: Value,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>>;
    fn db_write<'a>(
        &'a self,
        addon: &'a str,
        op: &'a str,
        input: Value,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>>;
    fn sql<'a>(
        &'a self,
        addon: &'a str,
        statement: &'a str,
        params: Vec<Value>,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>>;
    fn enqueue<'a>(
        &'a self,
        addon: &'a str,
        op: &'a str,
        input: Value,
        delay_secs: u32,
    ) -> pelty_payments::BoxFuture<'a, Result<(), String>>;
    /// Add/replace (`Some(schedule, description)`) or remove (`None`) a
    /// runtime cron job. Validation of the expression is the host's.
    fn cron_register<'a>(
        &'a self,
        _addon: &'a str,
        _name: &'a str,
        _job: Option<(&'a str, &'a str)>,
    ) -> pelty_payments::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("cron not available".to_string()) })
    }
}

/// No database (tests, `describe`).
pub struct NoPower;
impl PowerHost for NoPower {
    fn db_read<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Value,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err("db not available".to_string()) })
    }
    fn db_write<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Value,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err("db not available".to_string()) })
    }
    fn sql<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Vec<Value>,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err("db not available".to_string()) })
    }
    fn enqueue<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Value,
        _: u32,
    ) -> pelty_payments::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("jobs not available".to_string()) })
    }
}

/// True inside a container (`/.dockerenv`, `PELTY_DOCKER=1`): exec is off.
pub fn in_container() -> bool {
    // `PELTY_DOCKER=1` marks a container: addon exec is disabled.
    std::path::Path::new("/.dockerenv").exists()
        || std::env::var("PELTY_DOCKER").is_ok_and(|v| v == "1" || v == "true")
}

/// Root of per-addon data directories (`PELTY_ADDON_DATA_DIR`, default
/// `./data/addons`). Each addon gets `<root>/<id>`.
pub fn data_dir(addon: &str) -> std::path::PathBuf {
    // Root of per-addon data directories (default `data/addons`), one `<root>/<id>` each.
    let root = std::env::var("PELTY_ADDON_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data/addons"));
    root.join(addon)
}

/// Loopback, RFC 1918, CGNAT, ULA, unspecified or link-local: not public.
pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || (v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64)
                || v.octets()[0] == 0
        }
        std::net::IpAddr::V6(v) => {
            if let Some(m) = v.to_ipv4_mapped() {
                return is_private_ip(std::net::IpAddr::V4(m));
            }
            v.is_loopback()
                || v.is_unspecified()
                || (v.segments()[0] & 0xfe00) == 0xfc00
                || (v.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// A pre-event subscription: the host calls `call("hook", {event, payload})`
/// before the action and the addon may continue, cancel or modify it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Hook {
    pub event: String,
    /// Lower runs first; ties ordered by addon id. Accepts an integer or a
    /// Bukkit-style name: `lowest` (0), `low` (50), `normal` (100),
    /// `high` (150), `highest` (200), `monitor` (1000, observe only).
    #[serde(default = "default_priority", deserialize_with = "de_priority")]
    pub priority: i32,
    /// Also receive the event when an earlier listener cancelled it.
    #[serde(default)]
    pub ignore_cancelled: bool,
    /// On error/timeout: `true` lets the action continue, `false` blocks it.
    #[serde(default = "default_true")]
    pub fail_open: bool,
    /// Hook budget in milliseconds (capped by the sandbox timeout). The
    /// wasm store is interrupted at this deadline, not just abandoned.
    #[serde(default = "default_hook_ms")]
    pub timeout_ms: u64,
    /// Fuel budget for one hook call (capped by the sandbox fuel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
}

fn default_priority() -> i32 {
    100
}

/// `monitor` priority: listeners see the final state and cannot change it.
pub const PRIORITY_MONITOR: i32 = 1000;

fn de_priority<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum P {
        N(i32),
        S(String),
    }
    match <P as serde::Deserialize>::deserialize(d)? {
        P::N(n) if (-1000..=1000).contains(&n) => Ok(n),
        P::N(_) => Err(serde::de::Error::custom("priority out of range")),
        P::S(s) => match s.as_str() {
            "lowest" => Ok(0),
            "low" => Ok(50),
            "normal" => Ok(100),
            "high" => Ok(150),
            "highest" => Ok(200),
            "monitor" => Ok(PRIORITY_MONITOR),
            _ => Err(serde::de::Error::custom("unknown priority")),
        },
    }
}

/// Lifecycle phases, in order.
pub const PHASES: &[&str] = &["before", "during", "after", "finished"];

/// `during:invoice.paid` → (`during`, `invoice.paid`). No known prefix is
/// the `before` phase (legacy typed names such as `invoice.before_create`).
pub fn split_phase(e: &str) -> (&str, &str) {
    for p in PHASES {
        if let Some(rest) = e.strip_prefix(p).and_then(|r| r.strip_prefix(':')) {
            return (p, rest);
        }
    }
    ("before", e)
}

/// `*` or `<METHOD> /api/v1/...`: a pattern over core API requests.
pub fn is_request_pattern(t: &str) -> bool {
    t == "*"
        || t.split_once(' ')
            .is_some_and(|(_, p)| p.starts_with("/api/"))
}

/// `addon-id:name`, an addon-defined event (not a request pattern).
pub fn is_custom_event(e: &str) -> bool {
    match e.split_once(':') {
        Some((id, name)) => {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-*".contains(&b))
        }
        None => false,
    }
}

/// Addon events emitted from inside a call (custom events).
pub trait EventBus: Send + Sync {
    /// Run the `before` phase of `name` and schedule `after`/`finished`.
    /// `depth` counts nested emits (the host refuses deep chains).
    fn emit<'a>(
        &'a self,
        addon: &'a str,
        name: &'a str,
        payload: Value,
        depth: u8,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>>;
    /// Run addon-defined filter point `name` (applied by `addon`, which is
    /// skipped). Always yields a value (the input when nothing applies).
    fn filter<'a>(
        &'a self,
        _addon: &'a str,
        _name: &'a str,
        value: Value,
        _context: Value,
        _depth: u8,
    ) -> pelty_payments::BoxFuture<'a, Value> {
        Box::pin(async move { value })
    }
}

/// No event bus (tests, `describe`).
pub struct NoBus;
impl EventBus for NoBus {
    fn emit<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: Value,
        _: u8,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err("events not available".to_string()) })
    }
}

/// Max nesting of addon-applied filters / emitted events inside one call chain.
pub const MAX_ADDON_DEPTH: u8 = 4;
fn default_true() -> bool {
    true
}
fn default_hook_ms() -> u64 {
    500
}

/// An addon HTTP route. `access`: `admin` (needs `permission`), `client`
/// (any signed-in user) or `public`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Route {
    pub method: String,
    /// Path below the addon prefix, e.g. `/stats` or `/items/:id`.
    pub path: String,
    pub access: String,
    #[serde(default)]
    pub permission: Option<String>,
}

impl Route {
    /// Match `path` against this route; returns `:param` captures.
    pub fn matches(&self, method: &str, path: &str) -> Option<Vec<(String, String)>> {
        if !self.method.eq_ignore_ascii_case(method) {
            return None;
        }
        let a: Vec<&str> = self.path.trim_matches('/').split('/').collect();
        let b: Vec<&str> = path.trim_matches('/').split('/').collect();
        if a.len() != b.len() {
            return None;
        }
        let mut params = vec![];
        for (x, y) in a.iter().zip(&b) {
            match x.strip_prefix(':') {
                Some(n) if !y.is_empty() => params.push((n.to_string(), (*y).to_string())),
                None if x == y => {}
                _ => return None,
            }
        }
        Some(params)
    }
}

/// Shared host services.
#[derive(Clone)]
pub struct HostServices {
    pub http: Arc<dyn HttpTransport>,
    pub kv: Arc<dyn KvStore>,
    pub power: Arc<dyn PowerHost>,
    pub events: Arc<dyn EventBus>,
}

impl Default for HostServices {
    fn default() -> Self {
        HostServices {
            http: Arc::new(pelty_payments::NoTransport),
            kv: Arc::new(MemoryKv::default()),
            power: Arc::new(NoPower),
            events: Arc::new(NoBus),
        }
    }
}

/// Production host HTTP: SSRF-guarded reqwest client (no redirects, link-local
/// and cloud metadata refused, 30 s timeout) with the response capped at
/// [`HTTP_MAX_BYTES`]. Loopback/private hosts stay reachable because admins
/// legitimately point provisioning servers at LAN panels; the per-addon host
/// allowlist decides which hosts an addon may reach at all.
pub struct GuardedHttp;

impl HttpTransport for GuardedHttp {
    fn send(
        &self,
        req: HttpRequestSpec,
    ) -> pelty_payments::BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(pelty_provisioning::http::fetch(
            pelty_provisioning::http::RawRequest {
                method: req.method.into(),
                url: req.url,
                headers: req.headers,
                body: req.body,
            },
            HTTP_MAX_BYTES,
        ))
    }
}

/// Per-call context: settings, decrypted secrets, extra allowed hosts.
#[derive(Clone, Default)]
pub struct CallCtx {
    pub settings: Value,
    pub secrets: HashMap<String, String>,
    /// Host of the configured server (`$server`), e.g. a panel base URL.
    pub server_url: Option<String>,
    /// Per-call transport override (a gateway's configured transport).
    pub http: Option<Arc<dyn HttpTransport>>,
    /// Nesting depth of addon-emitted events (0 = not inside an event).
    pub event_depth: u8,
    /// Per-call wall-clock budget (hooks, mixins, filters); the sandbox
    /// timeout still caps it.
    pub timeout: Option<std::time::Duration>,
    /// Per-call fuel budget; the sandbox fuel still caps it.
    pub fuel: Option<u64>,
    /// Call context exposed to the addon through `host.context()`
    /// (event name, phase, viewer, request id …). `Null` = none.
    pub context: Value,
}

impl std::fmt::Debug for CallCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallCtx")
            .field("settings", &self.settings)
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .field("server_url", &self.server_url)
            .finish()
    }
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    addon_id: String,
    caps: Capabilities,
    ctx: CallCtx,
    services: HostServices,
    rt: Option<tokio::runtime::Handle>,
    logs: Vec<(String, String)>,
    http_calls: usize,
    power_calls: usize,
    deadline: std::time::Instant,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| {
        h.trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase()
    })
}

impl State {
    /// `Some(true)`: allowed only through `http: ["*"]` (public hosts unless
    /// `http_private`); `Some(false)`: an explicit host entry.
    fn allow_kind(&self, url: &str) -> Option<bool> {
        let h = host_of(url)?;
        if self.explicit(&h) {
            return Some(false);
        }
        self.caps.http.iter().any(|p| p == "*").then_some(true)
    }

    fn explicit(&self, h: &str) -> bool {
        let h = h.to_string();
        self.caps.http.iter().any(|p| {
            if p == "*" {
                return false;
            }
            if p == "$server" {
                return self
                    .ctx
                    .server_url
                    .as_deref()
                    .and_then(host_of)
                    .is_some_and(|s| s == h);
            }
            if let Some(k) = p.strip_prefix("$setting:") {
                return self
                    .ctx
                    .settings
                    .get(k)
                    .and_then(Value::as_str)
                    .and_then(host_of)
                    .is_some_and(|s| s == h);
            }
            let p = p.to_ascii_lowercase();
            match p.strip_prefix("*.") {
                Some(suffix) => h.ends_with(&format!(".{suffix}")),
                None => p == h,
            }
        })
    }

    /// Resolve the host and refuse private addresses (`http: ["*"]` without
    /// `http_private`). Literal IPs are checked without DNS.
    fn check_public(&self, url: &str) -> Result<(), String> {
        let u = url::Url::parse(url).map_err(|_| "invalid url".to_string())?;
        let host = host_of(url).ok_or("invalid url")?;
        let port = u.port_or_known_default().unwrap_or(443);
        let ips: Vec<std::net::IpAddr> = match host.parse::<std::net::IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) if host == "localhost" || host.ends_with(".localhost") => {
                return Err("private address refused (needs http_private)".into())
            }
            Err(_) => {
                let h = host.clone();
                self.block(async move {
                    tokio::net::lookup_host((h.as_str(), port))
                        .await
                        .map(|a| a.map(|s| s.ip()).collect::<Vec<_>>())
                })
                .ok_or("dns not ready")?
                .map_err(|e| format!("dns: {e}"))?
            }
        };
        if ips.is_empty() || ips.iter().any(|ip| is_private_ip(*ip)) {
            return Err("private address refused (needs http_private)".into());
        }
        Ok(())
    }

    fn block<F: std::future::Future>(&self, f: F) -> Option<F::Output> {
        match &self.rt {
            Some(rt) => rt.block_on(async {
                match tokio::time::timeout_at(tokio::time::Instant::from_std(self.deadline), f)
                    .await
                {
                    Ok(value) => Some(value),
                    Err(_) => {
                        tracing::warn!(addon = %self.addon_id, "addon host call timed out");
                        None
                    }
                }
            }),
            None => poll_ready(f),
        }
    }
}

/// Replace decrypted secret values (6+ chars) an addon tries to log.
pub fn redact_secrets(msg: &str, secrets: &HashMap<String, String>) -> String {
    let mut out = msg.to_string();
    for v in secrets.values().filter(|v| v.len() >= 6) {
        if out.contains(v.as_str()) {
            out = out.replace(v.as_str(), "[redacted]");
        }
    }
    out
}

/// Poll a future once (tests without a runtime; mock transports are ready).
fn poll_ready<F: std::future::Future>(f: F) -> Option<F::Output> {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

impl host::Host for State {
    fn log(&mut self, level: Level, message: String) {
        let msg: String = redact_secrets(&message, &self.ctx.secrets)
            .chars()
            .take(2000)
            .collect();
        let lv = format!("{level:?}").to_lowercase();
        tracing::info!(addon = %self.addon_id, level = %lv, "{msg}");
        if self.logs.len() < 200 {
            self.logs.push((lv, msg));
        }
    }

    fn http_fetch(&mut self, req: HttpRequest) -> Result<HttpResponse, String> {
        self.http_calls += 1;
        if self.http_calls > MAX_HTTP_CALLS {
            return Err("too many http calls in one operation".into());
        }
        if !(req.url.starts_with("https://") || req.url.starts_with("http://")) {
            return Err("url must be http(s)".into());
        }
        let Some(wildcard) = self.allow_kind(&req.url) else {
            tracing::warn!(addon = %self.addon_id, "http host not in allowlist");
            return Err(format!(
                "host not allowed by addon capabilities: {}",
                host_of(&req.url).unwrap_or_default()
            ));
        };
        let t = self
            .ctx
            .http
            .clone()
            .unwrap_or_else(|| self.services.http.clone());
        let public_only = wildcard && !self.caps.http_private;
        // Mock transports never dial. Reserved names such as `*.example` have no DNS.
        if public_only && !t.offline() {
            self.check_public(&req.url)?;
        }
        if req.body.len() > HTTP_MAX_BYTES || req.headers.len() > 64 {
            return Err("request too large".into());
        }
        let method: &'static str = match req.method.to_ascii_uppercase().as_str() {
            "GET" => "GET",
            "POST" => "POST",
            "PUT" => "PUT",
            "PATCH" => "PATCH",
            "DELETE" => "DELETE",
            "HEAD" => "HEAD",
            _ => return Err("unsupported method".into()),
        };
        let spec = HttpRequestSpec {
            method,
            url: req.url,
            headers: req.headers,
            body: req.body,
        };
        let out = if public_only {
            self.block(pelty_provisioning::http::public_only(t.send(spec)))
        } else {
            self.block(t.send(spec))
        }
        .ok_or_else(|| "http transport not ready".to_string())??;
        if out.1.len() > HTTP_MAX_BYTES {
            return Err("response too large".into());
        }
        Ok(HttpResponse {
            status: out.0,
            body: out.1,
        })
    }

    fn setting(&mut self, key: String) -> Option<String> {
        self.ctx
            .settings
            .get(&key)
            .filter(|v| !v.is_null())
            .map(Value::to_string)
    }

    fn secret(&mut self, key: String) -> Option<String> {
        if !self.caps.secrets.iter().any(|s| *s == key || s == "*") {
            return None;
        }
        self.ctx.secrets.get(&key).cloned()
    }

    fn kv_get(&mut self, key: String) -> Option<String> {
        if !self.caps.kv {
            return None;
        }
        let kv = self.services.kv.clone();
        self.block(kv.get(&self.addon_id, &key)).flatten()
    }

    fn kv_set(&mut self, key: String, value: String) -> Result<(), String> {
        if !self.caps.kv {
            return Err("kv capability not granted".into());
        }
        if key.len() > 200 || value.len() > 256 * 1024 {
            return Err("kv entry too large".into());
        }
        let kv = self.services.kv.clone();
        self.block(kv.set(&self.addon_id, &key, &value))
            .unwrap_or_else(|| Err("kv not ready".into()))
    }

    fn kv_delete(&mut self, key: String) {
        if self.caps.kv {
            let kv = self.services.kv.clone();
            let _ = self.block(kv.delete(&self.addon_id, &key));
        }
    }

    fn now_unix(&mut self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn random_bytes(&mut self, len: u32) -> Vec<u8> {
        use rand::Rng;
        let mut v = vec![0u8; len.min(4096) as usize];
        rand::rng().fill_bytes(&mut v);
        v
    }
}

const EXEC_MAX_OUTPUT: usize = 1024 * 1024;
const POWER_MAX_INPUT: usize = 1024 * 1024;

impl State {
    fn power_json(&self, s: &str) -> Result<Value, String> {
        if s.len() > POWER_MAX_INPUT {
            return Err("input too large".into());
        }
        if s.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(s).map_err(|e| format!("input is not JSON: {e}"))
    }
    /// Rate limit + audit trail (tracing target `pelty::addon_audit`) for
    /// every dangerous call, allowed or not.
    fn power_gate(&mut self, cap: &str, granted: bool) -> Result<(), String> {
        self.power_calls += 1;
        tracing::info!(target: "pelty::addon_audit", addon = %self.addon_id, cap, granted, n = self.power_calls, "addon power call");
        if !granted {
            return Err(self.denied(cap));
        }
        if self.power_calls > MAX_POWER_CALLS {
            return Err("too many power calls in one operation".into());
        }
        Ok(())
    }
    fn denied(&self, cap: &str) -> String {
        tracing::warn!(addon = %self.addon_id, cap, "capability not granted");
        format!("capability not granted: {cap}")
    }
}

impl power::Host for State {
    fn db_read(&mut self, entity: String, query: String) -> Result<String, String> {
        let g = self.caps.db.read.contains(&entity);
        self.power_gate(&format!("db.read:{entity}"), g)?;
        let q = self.power_json(&query)?;
        let p = self.services.power.clone();
        let out = self
            .block(p.db_read(&self.addon_id, &entity, q))
            .ok_or("db not ready")??;
        Ok(out.to_string())
    }

    fn db_write(&mut self, op: String, input: String) -> Result<String, String> {
        let g = self.caps.db.write.contains(&op);
        self.power_gate(&format!("db.write:{op}"), g)?;
        let v = self.power_json(&input)?;
        let p = self.services.power.clone();
        let out = self
            .block(p.db_write(&self.addon_id, &op, v))
            .ok_or("db not ready")??;
        Ok(out.to_string())
    }

    fn sql(&mut self, statement: String, params: String) -> Result<String, String> {
        let g = self.caps.db.sql;
        self.power_gate("db.sql", g)?;
        if statement.len() > 64 * 1024 {
            return Err("statement too large".into());
        }
        let params = match self.power_json(&params)? {
            Value::Null => vec![],
            Value::Array(a) if a.len() <= 100 => a,
            _ => return Err("params must be a JSON array (max 100)".into()),
        };
        let p = self.services.power.clone();
        let out = self
            .block(p.sql(&self.addon_id, &statement, params))
            .ok_or("db not ready")??;
        Ok(out.to_string())
    }

    fn exec(
        &mut self,
        program: String,
        args: Vec<String>,
        stdin: String,
    ) -> Result<ExecOutput, String> {
        let g = self.caps.exec.contains(&program);
        self.power_gate(&format!("exec:{program}"), g)?;
        if in_container() {
            return Err("exec is disabled in containers".into());
        }
        if args.len() > 64 || args.iter().any(|a| a.len() > 4096) || stdin.len() > EXEC_MAX_OUTPUT {
            return Err("arguments too large".into());
        }
        validate_exec_args(&args, self.caps.exec_args.get(&program).map(Vec::as_slice))?;
        let cwd = match self.caps.fs.then(|| data_dir(&self.addon_id)) {
            // A native program would follow symlinks out of the data dir.
            Some(d) if dir_has_symlink(&d) => {
                return Err("exec refused: symlink in data dir".into())
            }
            d => d,
        };
        run_program(&program, &args, &stdin, self.deadline, cwd)
    }

    fn job_enqueue(&mut self, op: String, input: String, delay_secs: u32) -> Result<(), String> {
        let g = self.caps.jobs.iter().any(|j| j.op == op);
        self.power_gate(&format!("jobs:{op}"), g)?;
        let v = self.power_json(&input)?;
        let p = self.services.power.clone();
        self.block(p.enqueue(&self.addon_id, &op, v, delay_secs.min(30 * 86400)))
            .ok_or("jobs not ready")?
    }

    fn cron_register(
        &mut self,
        name: String,
        schedule: String,
        description: String,
    ) -> Result<(), String> {
        let g = !self.caps.cron.is_empty() || self.caps.cron_runtime;
        self.power_gate("cron", g)?;
        if name.is_empty()
            || name.len() > 64
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err("invalid cron job name".into());
        }
        if self.caps.cron.iter().any(|j| j.name == name)
            || self.caps.jobs.iter().any(|j| j.op == name)
        {
            return Err("name clashes with a manifest job".into());
        }
        if schedule.len() > 128 || description.len() > 200 {
            return Err("schedule or description too long".into());
        }
        let p = self.services.power.clone();
        self.block(p.cron_register(&self.addon_id, &name, Some((&schedule, &description))))
            .ok_or("cron not ready")?
    }

    fn cron_unregister(&mut self, name: String) -> Result<(), String> {
        let g = !self.caps.cron.is_empty() || self.caps.cron_runtime;
        self.power_gate("cron", g)?;
        let p = self.services.power.clone();
        self.block(p.cron_register(&self.addon_id, &name, None))
            .ok_or("cron not ready")?
    }
}

/// Baseline exec argument rules: no control characters, no `..` path
/// segments, no absolute paths (the cwd is the addon's data dir). With an
/// allowlist every argument must equal a pattern or match `prefix*`.
pub fn validate_exec_args(args: &[String], allow: Option<&[String]>) -> Result<(), String> {
    for a in args {
        if a.chars().any(|c| c.is_control()) {
            return Err("exec argument contains control characters".into());
        }
        if a.split(['/', '=']).any(|seg| seg == "..") || a.starts_with('/') || a.contains("=/") {
            return Err(format!("exec argument not allowed: {a:?}"));
        }
        if let Some(allow) = allow {
            let ok = allow.iter().any(|p| match p.strip_suffix('*') {
                Some(prefix) => a.starts_with(prefix),
                None => p == a,
            });
            if !ok {
                return Err(format!("exec argument not in allowlist: {a:?}"));
            }
        }
    }
    Ok(())
}

/// Size of `dir` (files only, symlinks not followed) and whether it holds a symlink.
fn dir_usage(dir: &std::path::Path) -> (u64, bool) {
    let (mut size, mut link) = (0u64, false);
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > 100_000 {
                return (u64::MAX, link);
            }
            let Ok(m) = e.path().symlink_metadata() else {
                continue;
            };
            if m.file_type().is_symlink() {
                link = true;
            } else if m.is_dir() {
                stack.push(e.path());
            } else {
                size = size.saturating_add(m.len());
            }
        }
    }
    (size, link)
}

fn dir_has_symlink(dir: &std::path::Path) -> bool {
    dir_usage(dir).1
}

/// Default and max `/data` quota.
const FS_QUOTA_DEFAULT_MB: u32 = 64;
const FS_QUOTA_MAX_MB: u32 = 1024;

impl bindings::pelty::addon::context::Host for State {
    fn current(&mut self) -> String {
        self.ctx.context.to_string()
    }
}

impl bindings::pelty::addon::filters::Host for State {
    fn apply(&mut self, name: String, value: String, context: String) -> String {
        let own = name
            .strip_prefix(&format!("{}:", self.addon_id))
            .is_some_and(|p| !p.contains('*'));
        let declared = self.caps.filter_points.iter().any(|e| e == &name);
        if !own || !is_custom_event(&name) || !declared {
            let _ = self.denied(&format!("filter_points:{name}"));
            return value;
        }
        if value.len() > 256 * 1024
            || context.len() > 16 * 1024
            || self.ctx.event_depth >= MAX_ADDON_DEPTH
        {
            return value;
        }
        let Ok(v) = serde_json::from_str::<Value>(&value) else {
            return value;
        };
        let c: Value = serde_json::from_str(&context).unwrap_or(Value::Null);
        let b = self.services.events.clone();
        match self.block(b.filter(&self.addon_id, &name, v, c, self.ctx.event_depth + 1)) {
            Some(out) => out.to_string(),
            None => value,
        }
    }
}

impl events::Host for State {
    fn emit(&mut self, name: String, payload: String) -> Result<String, String> {
        let own = name.strip_prefix(&format!("{}:", self.addon_id)).is_some();
        let declared = self
            .caps
            .emits
            .iter()
            .any(|e| e == &name || e.strip_suffix('*').is_some_and(|p| name.starts_with(p)));
        if !own || !is_custom_event(&name) || !declared {
            return Err(self.denied(&format!("emits:{name}")));
        }
        if payload.len() > 64 * 1024 {
            return Err("payload too large".into());
        }
        let v: Value =
            serde_json::from_str(&payload).map_err(|_| "payload must be JSON".to_string())?;
        if !v.is_object() {
            return Err("payload must be a JSON object".into());
        }
        if self.ctx.event_depth >= MAX_ADDON_DEPTH {
            return Err("event chain too deep".into());
        }
        let b = self.services.events.clone();
        let out = self
            .block(b.emit(&self.addon_id, &name, v, self.ctx.event_depth + 1))
            .ok_or("events not ready")??;
        Ok(out.to_string())
    }
}

/// Run `program` directly (no shell) with an empty environment, the addon's
/// data dir (or `/`) as cwd, killed at the call deadline; output capped.
fn run_program(
    program: &str,
    args: &[String],
    stdin: &str,
    deadline: std::time::Instant,
    cwd: Option<std::path::PathBuf>,
) -> Result<ExecOutput, String> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.current_dir(
        cwd.filter(|d| d.is_dir())
            .unwrap_or_else(|| std::path::PathBuf::from("/")),
    );
    let mut child = cmd.spawn().map_err(|e| format!("exec: {e}"))?;
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(stdin.as_bytes());
    }
    let read = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = r.take(EXEC_MAX_OUTPUT as u64).read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|o| Box::new(o) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|o| Box::new(o) as Box<dyn Read + Send>),
    );
    let status = loop {
        match child.try_wait().map_err(|e| format!("exec: {e}"))? {
            Some(st) => break st,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("exec timed out".into());
            }
            None => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    };
    Ok(ExecOutput {
        status: status.code().unwrap_or(-1),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// A loaded addon (compiled component + manifest facts).
pub struct WasmAddon {
    engine: Engine,
    /// Imports resolved once per component; each call only instantiates.
    pre: bindings::PeltyAddonPre<State>,
    /// `pelty:addon/scheduled@0.2.0#on-cron`, when exported (ABI 0.5).
    on_cron_export: Option<wasmtime::component::ComponentExportIndex>,
    pub sandbox: Sandbox,
    pub id: String,
    pub kind: String,
    pub version: String,
    pub caps: Capabilities,
    pub manifest: Value,
    /// Cached `describe()` output.
    pub descriptor: Value,
    services: HostServices,
}

/// Output of one call.
#[derive(Debug)]
pub struct CallOutput {
    pub value: String,
    pub logs: Vec<(String, String)>,
}

/// Process-wide engine (fuel + epochs + component model). Compiled code is
/// cached on disk under `PELTY_WASM_CACHE_DIR` (default: OS temp dir) so a
/// restart does not recompile unchanged addons.
fn shared_engine() -> Result<Engine, WasmError> {
    static E: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();
    if let Some(e) = E.get() {
        return Ok(e.clone());
    }
    let mut c = wasmtime::Config::new();
    c.consume_fuel(true)
        .epoch_interruption(true)
        .wasm_component_model(true);
    // Compiled-module cache directory (default `<tmp>/pelty-wasm-cache`).
    let dir = std::env::var("PELTY_WASM_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pelty-wasm-cache"));
    let mut cc = wasmtime::CacheConfig::new();
    cc.with_directory(dir);
    match wasmtime::Cache::new(cc) {
        Ok(cache) => {
            c.cache(Some(cache));
        }
        Err(e) => tracing::warn!(error = %e, "wasm compile cache disabled"),
    }
    let e = Engine::new(&c).map_err(|e| WasmError::Load(e.to_string()))?;
    Ok(E.get_or_init(|| e).clone())
}

type Compiled = (Engine, bindings::PeltyAddonPre<State>);

/// Compile (or reuse) a component; keyed by the sha256 of its bytes.
fn compiled(wasm: &[u8]) -> Result<Compiled, WasmError> {
    use sha2::Digest;
    static C: std::sync::OnceLock<Mutex<HashMap<[u8; 32], Compiled>>> = std::sync::OnceLock::new();
    let key: [u8; 32] = sha2::Sha256::digest(wasm).into();
    let cache = C.get_or_init(Default::default);
    if let Some(c) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Ok(c.clone());
    }
    let engine = shared_engine()?;
    let component = Component::new(&engine, wasm).map_err(|e| WasmError::Load(e.to_string()))?;
    let mut linker: Linker<State> = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
        .map_err(|e| WasmError::Load(e.to_string()))?;
    bindings::PeltyAddon::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
        .map_err(|e| WasmError::Load(e.to_string()))?;
    let pre = linker
        .instantiate_pre(&component)
        .map_err(|e| WasmError::Load(e.to_string()))?;
    let pre = bindings::PeltyAddonPre::new(pre).map_err(|e| WasmError::Load(e.to_string()))?;
    let c = (engine, pre);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, c.clone());
    Ok(c)
}

impl WasmAddon {
    /// Load from a validated package (`engine: "wasm"`, abi 0.2.x).
    pub fn from_package(pkg: &Package, services: HostServices) -> Result<Self, WasmError> {
        let m = &pkg.manifest;
        if m["engine"] != "wasm" {
            return Err(WasmError::Manifest("engine must be \"wasm\"".into()));
        }
        let kind = m["kind"].as_str().unwrap_or_default().to_string();
        if !KINDS.contains(&kind.as_str()) {
            return Err(WasmError::Manifest(format!("unsupported kind {kind:?}")));
        }
        let abi = m["abi"].as_str().unwrap_or_default();
        if !abi_supported(abi) {
            return Err(WasmError::Manifest(format!(
                "unsupported abi {abi:?}, host implements {ABI}"
            )));
        }
        let path = m["wasm"]
            .as_str()
            .ok_or_else(|| WasmError::Manifest("missing \"wasm\"".into()))?;
        let wasm = pkg
            .files
            .get(path)
            .ok_or_else(|| WasmError::Manifest(format!("wasm file {path} not in package")))?;
        let caps: Capabilities = match m.get("capabilities").filter(|v| !v.is_null()) {
            Some(value) if value.is_object() => serde_json::from_value(value.clone())
                .map_err(|e| WasmError::Manifest(format!("capabilities: {e}")))?,
            Some(_) => return Err(WasmError::Manifest("capabilities must be an object".into())),
            None => Capabilities::default(),
        };
        let sandbox =
            Sandbox::from_manifest(&kind, m.get("limits")).map_err(WasmError::Manifest)?;
        if !caps.exec.is_empty()
            && caps
                .exec
                .iter()
                .any(|p| !p.starts_with('/') || p.contains(".."))
        {
            return Err(WasmError::Manifest(
                "exec entries must be absolute program paths".into(),
            ));
        }
        Self::from_bytes(
            &pkg.id,
            &kind,
            m["version"].as_str().unwrap_or("0.0.0"),
            wasm,
            caps,
            m.clone(),
            sandbox,
            services,
        )
    }

    pub fn from_zip(
        bytes: &[u8],
        limits: Limits,
        services: HostServices,
    ) -> Result<Self, WasmError> {
        let pkg = addon_pkg::inspect(bytes, limits)?;
        Self::from_package(&pkg, services)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_bytes(
        id: &str,
        kind: &str,
        version: &str,
        wasm: &[u8],
        caps: Capabilities,
        manifest: Value,
        sandbox: Sandbox,
        services: HostServices,
    ) -> Result<Self, WasmError> {
        let (engine, pre) = compiled(wasm)?;
        let on_cron_export = {
            let c = pre.instance_pre().component();
            c.get_export_index(None, "pelty:addon/scheduled@0.2.0")
                .and_then(|i| c.get_export_index(Some(&i), "on-cron"))
        };
        let mut a = WasmAddon {
            engine,
            pre,
            on_cron_export,
            sandbox,
            id: id.into(),
            kind: kind.into(),
            version: version.into(),
            caps,
            manifest,
            descriptor: Value::Null,
            services,
        };
        let d = a.run(&CallCtx::default(), None, |inst, store| {
            inst.pelty_addon_addon().call_describe(store).map(Ok)
        })?;
        a.descriptor = serde_json::from_str(&d.value)
            .map_err(|e| WasmError::Manifest(format!("describe() is not JSON: {e}")))?;
        Ok(a)
    }

    /// `call(op, input)`.
    /// Synchronous call: host HTTP may only use ready futures (mocks). Use
    /// [`WasmAddon::call_async`] from async code for real network I/O.
    pub fn call(&self, ctx: &CallCtx, op: &str, input: &Value) -> Result<CallOutput, WasmError> {
        self.call_with(ctx, op, input, None)
    }

    fn call_with(
        &self,
        ctx: &CallCtx,
        op: &str,
        input: &Value,
        rt: Option<tokio::runtime::Handle>,
    ) -> Result<CallOutput, WasmError> {
        let input = input.to_string();
        self.run(ctx, rt, |inst, store| {
            inst.pelty_addon_addon().call_call(store, op, &input)
        })
    }

    /// `call` decoded as JSON.
    pub fn call_json(&self, ctx: &CallCtx, op: &str, input: &Value) -> Result<Value, WasmError> {
        let out = self.call(ctx, op, input)?;
        serde_json::from_str(&out.value)
            .map_err(|e| WasmError::Addon(format!("{op}: output is not JSON: {e}")))
    }

    /// `on-event(name, payload)`.
    pub fn on_event(
        &self,
        ctx: &CallCtx,
        name: &str,
        payload: &Value,
    ) -> Result<CallOutput, WasmError> {
        self.on_event_with(ctx, name, payload, None)
    }

    fn on_event_with(
        &self,
        ctx: &CallCtx,
        name: &str,
        payload: &Value,
        rt: Option<tokio::runtime::Handle>,
    ) -> Result<CallOutput, WasmError> {
        let p = payload.to_string();
        self.run(ctx, rt, |inst, store| {
            inst.pelty_addon_addon()
                .call_on_event(store, name, &p)
                .map(|r| r.map(|_| String::new()))
        })
    }

    /// `on-cron(job)` (ABI 0.5 `scheduled` export). Components without the
    /// export (0.2 to 0.4) fail with an addon error.
    pub fn on_cron(&self, ctx: &CallCtx, job: &str) -> Result<CallOutput, WasmError> {
        self.on_cron_with(ctx, job, None)
    }

    fn on_cron_with(
        &self,
        ctx: &CallCtx,
        job: &str,
        rt: Option<tokio::runtime::Handle>,
    ) -> Result<CallOutput, WasmError> {
        let Some(idx) = self.on_cron_export else {
            return Err(WasmError::Addon(
                "addon does not export on-cron (needs abi 0.5)".into(),
            ));
        };
        self.run_raw(ctx, rt, |inst, store| {
            let f = inst.get_typed_func::<(&str,), (Result<String, String>,)>(&mut *store, idx)?;
            let (r,) = f.call(&mut *store, (job,))?;
            Ok(r)
        })
    }

    /// [`WasmAddon::on_cron`] off the async runtime; the result as JSON.
    pub async fn on_cron_async(
        self: &Arc<Self>,
        ctx: CallCtx,
        job: String,
    ) -> Result<Value, WasmError> {
        let me = self.clone();
        let rt = tokio::runtime::Handle::current();
        let out = tokio::task::spawn_blocking(move || me.on_cron_with(&ctx, &job, Some(rt)))
            .await
            .map_err(|e| WasmError::Trap(e.to_string()))??;
        Ok(serde_json::from_str(&out.value).unwrap_or(Value::String(out.value)))
    }

    /// Whether the component exports `on-cron` (ABI 0.5).
    pub fn exports_on_cron(&self) -> bool {
        self.on_cron_export.is_some()
    }

    /// True when the manifest subscribes to `event` (`capabilities.events`;
    /// `*` = all, `invoice.*` = prefix).
    pub fn subscribes(&self, event: &str) -> bool {
        self.caps.events.iter().any(|e| {
            e == "*"
                || e == event
                || e.strip_suffix(".*")
                    .is_some_and(|p| event.strip_prefix(p).is_some_and(|r| r.starts_with('.')))
        })
    }

    /// The pre-event hook this addon registered for `event`, if any.
    pub fn hook(&self, event: &str) -> Option<&Hook> {
        self.caps.hooks.iter().find(|h| h.event == event)
    }

    /// The filter subscription for `name`, if any.
    pub fn filter(&self, name: &str) -> Option<&Filter> {
        self.caps.filters.iter().find(|f| f.name == name)
    }

    /// [`WasmAddon::on_event`] off the async runtime.
    pub async fn on_event_async(
        self: &Arc<Self>,
        ctx: CallCtx,
        name: String,
        payload: Value,
    ) -> Result<(), WasmError> {
        let me = self.clone();
        let rt = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || me.on_event_with(&ctx, &name, &payload, Some(rt)))
            .await
            .map_err(|e| WasmError::Trap(e.to_string()))??;
        Ok(())
    }

    fn run<F>(
        &self,
        ctx: &CallCtx,
        rt: Option<tokio::runtime::Handle>,
        f: F,
    ) -> Result<CallOutput, WasmError>
    where
        F: FnOnce(
            &bindings::PeltyAddon,
            &mut Store<State>,
        ) -> wasmtime::Result<Result<String, String>>,
    {
        self.run_raw(ctx, rt, |inst, store| {
            let typed = bindings::PeltyAddon::new(&mut *store, inst)?;
            f(&typed, store)
        })
    }

    /// One sandboxed call against the raw component instance.
    fn run_raw<F>(
        &self,
        ctx: &CallCtx,
        rt: Option<tokio::runtime::Handle>,
        f: F,
    ) -> Result<CallOutput, WasmError>
    where
        F: FnOnce(
            &wasmtime::component::Instance,
            &mut Store<State>,
        ) -> wasmtime::Result<Result<String, String>>,
    {
        let timeout = ctx
            .timeout
            .map_or(self.sandbox.timeout, |t| t.min(self.sandbox.timeout));
        let fuel = ctx
            .fuel
            .map_or(self.sandbox.fuel, |f| f.min(self.sandbox.fuel));
        let deadline = std::time::Instant::now() + timeout;
        let mut wasi = WasiCtx::builder();
        if self.caps.fs {
            let dir = data_dir(&self.id);
            std::fs::create_dir_all(&dir)
                .map_err(|e| WasmError::Load(format!("addon data dir: {e}")))?;
            let quota = u64::from(
                self.caps
                    .fs_quota_mb
                    .unwrap_or(FS_QUOTA_DEFAULT_MB)
                    .min(FS_QUOTA_MAX_MB),
            ) * 1024
                * 1024;
            let (used, _) = dir_usage(&dir);
            let perms = if used >= quota {
                tracing::warn!(target: "pelty::addon_audit", addon = %self.id, used, quota, "addon data dir over quota: mounted read-only");
                wasmtime_wasi::FsPerms::ReadOnly
            } else {
                wasmtime_wasi::FsPerms::ReadWrite
            };
            wasi.preopened_dir(&dir, "/data", perms)
                .map_err(|e| WasmError::Load(format!("addon data dir: {e}")))?;
        }
        let state = State {
            wasi: wasi.build(),
            table: ResourceTable::new(),
            limits: crate::store_limits(self.sandbox, 8),
            addon_id: self.id.clone(),
            caps: self.caps.clone(),
            ctx: ctx.clone(),
            services: self.services.clone(),
            rt,
            logs: Vec::new(),
            http_calls: 0,
            power_calls: 0,
            deadline,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(fuel)
            .map_err(|e| WasmError::Load(e.to_string()))?;
        // The engine is shared by every addon, so the watchdog's epoch tick
        // reaches all running stores; each store checks its own deadline.
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(move |_| {
            if std::time::Instant::now() >= deadline {
                Err(wasmtime::Error::msg("addon timed out"))
            } else {
                Ok(wasmtime::UpdateDeadline::Continue(1))
            }
        });
        let engine = self.engine.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if rx.recv_timeout(timeout).is_err() {
                engine.increment_epoch();
            }
        });
        let out = (|| {
            let inst = self
                .pre
                .instance_pre()
                .instantiate(&mut store)
                .map_err(|e| WasmError::Trap(format!("{e:#}")))?;
            f(&inst, &mut store).map_err(|e| WasmError::Trap(format!("{e:#}")))
        })();
        let _ = tx.send(());
        let _ = watchdog.join();
        let value = out?.map_err(WasmError::Addon)?;
        Ok(CallOutput {
            value,
            logs: std::mem::take(&mut store.data_mut().logs),
        })
    }

    /// Run a call off the async runtime (host HTTP blocks on the runtime handle).
    pub async fn call_async(
        self: &Arc<Self>,
        ctx: CallCtx,
        op: String,
        input: Value,
    ) -> Result<Value, WasmError> {
        let me = self.clone();
        let rt = tokio::runtime::Handle::current();
        let out = tokio::task::spawn_blocking(move || me.call_with(&ctx, &op, &input, Some(rt)))
            .await
            .map_err(|e| WasmError::Trap(e.to_string()))??;
        serde_json::from_str(&out.value)
            .map_err(|e| WasmError::Addon(format!("output is not JSON: {e}")))
    }
}

#[cfg(test)]
mod abi_tests {
    #[test]
    fn abi_range() {
        for ok in [
            "0.2.0",
            "0.2.999",
            "0.3.0",
            "0.3.7",
            "0.4.0",
            "0.4.999",
            "0.5.0",
            "0.5.0+build.7",
            "0.4.999+legacy",
            super::ABI,
        ] {
            assert!(super::abi_supported(ok), "{ok}");
        }
        for bad in [
            "0.1.0",
            "0.6.0",
            "1.0.0",
            "0.5.1",
            "0.5.999",
            "0.5.1+build",
            "0.2.0-alpha",
            "0.4.7-rc.1",
            "0.5.0-rc.1",
            "0.5.0-rc.1+build",
            "",
            "0",
            "0.5",
            "0.5.bogus",
            "0.5.999.extra",
            "0.5.0.extra",
            "0.05.0",
            "0.5.00",
            "v0.5.0",
            " 0.5.0",
            "0.5.0 ",
            "0.5.0+",
        ] {
            assert!(!super::abi_supported(bad), "{bad}");
        }
    }
}

#[cfg(test)]
mod route_tests {
    use super::Route;

    #[test]
    fn route_matching() {
        let r = Route {
            method: "GET".into(),
            path: "/items/:id".into(),
            access: "public".into(),
            permission: None,
        };
        assert_eq!(
            r.matches("get", "/items/42"),
            Some(vec![("id".into(), "42".into())])
        );
        assert_eq!(r.matches("POST", "/items/42"), None);
        assert_eq!(r.matches("GET", "/items"), None);
        assert_eq!(r.matches("GET", "/items//"), None);
        assert_eq!(r.matches("GET", "/other/42"), None);
    }
}

#[cfg(test)]
mod exec_fs_tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exec_args_baseline_and_allowlist() {
        assert!(validate_exec_args(&v(&["--json", "status", "file.txt"]), None).is_ok());
        for bad in [
            "../x",
            "a/../../etc",
            "/etc/passwd",
            "--out=/etc/x",
            "--f=..",
            "a\nb",
            "x\u{7}",
        ] {
            assert!(validate_exec_args(&v(&[bad]), None).is_err(), "{bad}");
        }
        let allow = v(&["status", "--format=*"]);
        assert!(validate_exec_args(&v(&["status", "--format=json"]), Some(&allow)).is_ok());
        assert!(validate_exec_args(&v(&["delete"]), Some(&allow)).is_err());
        assert!(validate_exec_args(&v(&["--formatx"]), Some(&allow)).is_err());
    }

    #[test]
    fn dir_usage_counts_bytes_and_finds_symlinks() {
        let d = std::env::temp_dir().join(format!("pelty-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a"), vec![0u8; 1000]).unwrap();
        std::fs::write(d.join("sub/b"), vec![0u8; 24]).unwrap();
        assert_eq!(dir_usage(&d), (1024, false));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", d.join("sub/escape")).unwrap();
            assert_eq!(dir_usage(&d), (1024, true));
        }
        std::fs::remove_dir_all(&d).unwrap();
    }
}

#[cfg(test)]
mod redact_tests {
    #[test]
    fn secrets_are_redacted_from_logs() {
        let mut m = std::collections::HashMap::new();
        m.insert("api_key".to_string(), "sk_live_abcdef123".to_string());
        m.insert("pin".to_string(), "123".to_string());
        assert_eq!(
            super::redact_secrets("key=sk_live_abcdef123 pin=123", &m),
            "key=[redacted] pin=123"
        );
    }
}
