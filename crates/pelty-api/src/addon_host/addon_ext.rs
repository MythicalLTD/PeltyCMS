//! WASM addon extension points beyond the per-kind contracts:
//! synchronous pre-action hooks, addon HTTP routes under
//! `/api/v1/addons/<id>/…` and declarative admin/client UI.
//! All of them go through the addon's `call` export (ABI 0.2): op `hook`,
//! op `http`. See `make docs`.
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::AppState;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use pelty_core::{AppError, AppResult};
use serde_json::{json, Value};

/// A cron job registered at runtime: (name, schedule, description, created).
pub type RuntimeCron = (String, String, String, i64);
static RUNTIME_CRON: std::sync::RwLock<
    Option<std::collections::HashMap<String, Vec<RuntimeCron>>>,
> = std::sync::RwLock::new(None);
/// Max runtime cron jobs per addon.
pub const MAX_RUNTIME_CRON: usize = 20;

/// Reload the runtime cron jobs (`addon_cron_jobs`) into memory; called on
/// every scheduler tick so all nodes see registrations within a minute.
pub async fn refresh_runtime_cron(pool: &sqlx::PgPool) {
    type Row = (String, String, String, String, time::OffsetDateTime);
    let rows: Result<Vec<Row>, _> = sqlx::query_as("SELECT addon_id, name, schedule, description, created_at FROM addon_cron_jobs ORDER BY addon_id, name").fetch_all(pool).await;
    match rows {
        Ok(rows) => {
            let mut m: std::collections::HashMap<String, Vec<RuntimeCron>> = Default::default();
            for (a, n, sch, d, c) in rows {
                m.entry(a)
                    .or_default()
                    .push((n, sch, d, c.unix_timestamp()));
            }
            *RUNTIME_CRON.write().unwrap_or_else(|e| e.into_inner()) = Some(m);
        }
        Err(e) => tracing::warn!(error = %e, "addon cron jobs unavailable"),
    }
}

/// Runtime cron jobs of one addon (from the last refresh).
pub fn runtime_cron(addon: &str) -> Vec<RuntimeCron> {
    RUNTIME_CRON
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(addon).cloned())
        .unwrap_or_default()
}

/// `power.cron-register` / `cron-unregister` (`job: None`).
pub async fn cron_register(
    pool: &sqlx::PgPool,
    addon: &str,
    name: &str,
    job: Option<(&str, &str)>,
) -> Result<(), String> {
    let r = match job {
        Some((schedule, description)) => {
            crate::cron::parse(schedule).map_err(|e| format!("invalid schedule: {e}"))?;
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM addon_cron_jobs WHERE addon_id = $1 AND name <> $2",
            )
            .bind(addon)
            .bind(name)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("db: {e}"))?;
            if n as usize >= MAX_RUNTIME_CRON {
                return Err(format!("at most {MAX_RUNTIME_CRON} runtime cron jobs"));
            }
            sqlx::query("INSERT INTO addon_cron_jobs (addon_id, name, schedule, description) VALUES ($1, $2, $3, $4) ON CONFLICT (addon_id, name) DO UPDATE SET schedule = EXCLUDED.schedule, description = EXCLUDED.description, created_at = CASE WHEN addon_cron_jobs.schedule = EXCLUDED.schedule THEN addon_cron_jobs.created_at ELSE now() END")
                .bind(addon)
                .bind(name)
                .bind(schedule)
                .bind(description)
                .execute(pool)
                .await
        }
        None => {
            sqlx::query("DELETE FROM addon_cron_jobs WHERE addon_id = $1 AND name = $2")
                .bind(addon)
                .bind(name)
                .execute(pool)
                .await
        }
    };
    r.map_err(|e| format!("db: {e}"))?;
    refresh_runtime_cron(pool).await;
    Ok(())
}
use std::sync::Arc;

const MAX_BODY: usize = 64 * 1024;

/// Typed pre-events, or a generic request pre-event pattern:
/// `before:*`, `before:<METHOD> /api/v1/<prefix>*` or an exact route
/// template (`before:POST /api/v1/admin/invoices/{id}/payments`).
pub fn hook_event_ok(e: &str) -> bool {
    if pelty_extensions::events::is_pre_event(e) {
        return true;
    }
    let (phase, rest) = pelty_wasm::split_phase(e);
    if rest.len() == e.len() {
        return false;
    }
    if rest == "*" {
        return true;
    }
    if pelty_wasm::is_custom_event(rest) {
        return true;
    }
    // Core domain events (`after:invoice.paid`) for the observe phases.
    if phase != "before"
        && !rest.contains(' ')
        && !rest.is_empty()
        && rest.len() <= 64
        && rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b"._*".contains(&b))
    {
        return true;
    }
    let Some((m, path)) = rest.split_once(' ') else {
        return false;
    };
    ["*", "POST", "PUT", "PATCH", "DELETE"].contains(&m)
        && path.starts_with("/api/v1/")
        && path.len() <= 200
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_{}*".contains(&b))
}

pub fn validate_caps(c: &pelty_wasm::AddonCapabilities) -> AppResult<()> {
    let bad = || AppError::Validation("error.addon_manifest");
    if c.hooks
        .iter()
        .any(|h| !hook_event_ok(&h.event) || h.timeout_ms == 0 || h.timeout_ms > 5000)
    {
        return Err(bad());
    }
    if c.ui
        .iter()
        .any(|u| u != "iframe" && u != "iframe.same_origin" && u != "override")
    {
        return Err(bad());
    }
    let entities = crate::addon_power::ENTITIES;
    if c.db
        .read
        .iter()
        .any(|e| !entities.iter().any(|(n, ..)| n == e))
        || c.db
            .write
            .iter()
            .any(|o| !crate::addon_power::WRITE_OPS.contains(&o.as_str()))
    {
        return Err(bad());
    }
    let op_ok = |o: &str| {
        !o.is_empty()
            && o.len() <= 64
            && !["hook", "http", "mixin", "filter", "auth.verify"].contains(&o)
            && o.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    if c.jobs.len() > 20
        || c.jobs
            .iter()
            .any(|j| !op_ok(&j.op) || (j.every_secs != 0 && j.every_secs < 60))
    {
        return Err(bad());
    }
    // Cron jobs (ABI 0.5): unique names, valid 5-field expressions.
    if c.cron.len() > 20
        || c.cron.iter().enumerate().any(|(i, j)| {
            !op_ok(&j.name)
                || crate::cron::parse(&j.schedule).is_err()
                || c.cron[..i].iter().any(|o| o.name == j.name)
                || c.jobs.iter().any(|o| o.op == j.name)
        })
    {
        return Err(bad());
    }
    if c.exec.len() > 20
        || c.exec
            .iter()
            .any(|p| !p.starts_with('/') || p.contains("..") || p.len() > 256)
    {
        return Err(bad());
    }
    if c.mixins.len() > 30 || c.mixins.iter().any(|m| !crate::addon_events::mixin_ok(m)) {
        return Err(bad());
    }
    // Core filter points, or another addon's `<id>:<point>` (it may be installed later).
    let custom_point = |n: &str| pelty_wasm::is_custom_event(n) && !n.contains('*');
    if c.filters.len() > 30
        || c.filters.iter().any(|f| {
            !(pelty_extensions::filters::is_filter(&f.name) || custom_point(&f.name))
                || f.timeout_ms == 0
                || f.timeout_ms > 5000
        })
    {
        return Err(bad());
    }
    // Own prefix is checked at runtime (`filters.apply`), like `emits`.
    if c.filter_points.len() > 30 || c.filter_points.iter().any(|p| !custom_point(p)) {
        return Err(bad());
    }
    if c.emits.len() > 50 || c.emits.iter().any(|e| !pelty_wasm::is_custom_event(e)) {
        return Err(bad());
    }
    if c.http_private && c.http.is_empty() {
        return Err(bad());
    }
    let prefix_ok = |p: &str| {
        p.starts_with("/api/v1/")
            && p.len() <= 128
            && !p.contains("..")
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
    };
    if c.bridge.len() > 20
        || c.bridge.iter().any(|p| !prefix_ok(p))
        || (!c.bridge.is_empty() && !c.iframe())
    {
        return Err(bad());
    }
    for r in &c.routes {
        let path_ok = r.path.starts_with('/')
            && r.path.len() <= 128
            && r.path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_:".contains(&b));
        let method_ok = ["GET", "POST", "PUT", "PATCH", "DELETE"]
            .contains(&r.method.to_ascii_uppercase().as_str());
        let access_ok = match r.access.as_str() {
            "admin" => r.permission.as_deref().is_some_and(|p| !p.is_empty()),
            "client" | "public" => true,
            _ => false,
        };
        if !path_ok || !method_ok || !access_ok {
            return Err(bad());
        }
    }
    Ok(())
}

/// Manifest `ui`: `{ pages: [UiPage], slots: [UiSlotContribution] }` in the
/// plugin node schema (see `make docs`), rendered natively with shadcn.
/// Titles/labels live in `plugin.<addon-id>.*`, supplied by manifest `i18n`;
/// UI requests may only target `/api/v1/addons/<addon-id>/`.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddonUi {
    #[serde(default)]
    pub pages: Vec<pelty_extensions::ui_schema::UiPage>,
    #[serde(default)]
    pub slots: Vec<pelty_extensions::ui_schema::UiSlotContribution>,
    /// Frontend overrides; used only with the `ui: ["override"]` capability.
    #[serde(default)]
    pub overrides: UiOverrides,
}

/// Manifest `ui.overrides` (frontend mixins).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiOverrides {
    /// Core pages replaced by one of the addon's pages:
    /// `"<area>.<route-key>": "<page id>"` (same area), e.g.
    /// `"client.invoices": "my-invoices"`.
    #[serde(default)]
    pub pages: std::collections::BTreeMap<String, String>,
    /// Core sidebar entries to hide, by in-app path (`/billing/quotes`).
    #[serde(default)]
    pub hide_nav: Vec<String>,
}

/// `iframe`: the addon holds the `ui.iframe` capability. `files`: package
/// files (frame sources must be shipped), `None` skips that check.
pub fn parse_ui(
    id: &str,
    ui: &Value,
    iframe: bool,
    files: Option<&dyn Fn(&str) -> bool>,
) -> AppResult<AddonUi> {
    let bad = || AppError::Validation("error.addon_ui");
    let ui: AddonUi = serde_json::from_value(ui.clone()).map_err(|_| bad())?;
    if ui.pages.len() > 30 || ui.slots.len() > 30 {
        return Err(bad());
    }
    let own = format!("/api/v1/addons/{id}/");
    let rules = pelty_extensions::ui_schema::UiRules {
        plugin_id: id,
        allow_embed: false,
        allow_core_actions: false,
        own_api: Some(&own),
        allow_iframe: iframe,
    };
    let mut seen = std::collections::HashSet::new();
    for p in &ui.pages {
        if !seen.insert(p.id.clone()) {
            return Err(bad());
        }
        pelty_extensions::ui_schema::validate_page(&rules, p).map_err(|e| {
            tracing::warn!(addon = id, page = %p.id, error = %e, "addon ui rejected");
            bad()
        })?;
    }
    if let Some(has) = files {
        let mut srcs: Vec<String> = ui
            .pages
            .iter()
            .filter_map(|p| p.frame.as_ref())
            .chain(ui.slots.iter().filter_map(|s| s.frame.as_ref()))
            .map(|f| f.src.clone())
            .collect();
        for b in ui
            .pages
            .iter()
            .map(|p| &p.body)
            .chain(ui.slots.iter().map(|s| &s.body))
        {
            frame_srcs(b, &mut srcs);
        }
        if srcs.iter().any(|s| !has(s)) {
            return Err(AppError::Validation("error.addon_integrity"));
        }
    }
    for sl in &ui.slots {
        pelty_extensions::ui_schema::validate_slot(&rules, sl).map_err(|e| {
            tracing::warn!(addon = id, slot = %sl.slot, error = %e, "addon ui rejected");
            bad()
        })?;
    }
    let o = &ui.overrides;
    let key_ok = |k: &str| {
        !k.is_empty()
            && k.len() <= 64
            && k.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let route_ok = |r: &str, page: &str| {
        r.split_once('.').is_some_and(|(area, key)| {
            (area == "admin" || area == "client")
                && key_ok(key)
                && ui.pages.iter().any(|p| p.id == page && p.area == area)
        })
    };
    let path_ok = |p: &str| {
        p.starts_with('/')
            && !p.starts_with("//")
            && p.len() <= 128
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
    };
    if o.pages.len() > 20
        || o.hide_nav.len() > 50
        || !o.pages.iter().all(|(r, p)| route_ok(r, p))
        || !o.hide_nav.iter().all(|p| path_ok(p))
    {
        return Err(bad());
    }
    Ok(ui)
}

fn frame_srcs(n: &Value, out: &mut Vec<String>) {
    match n {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("frame") {
                out.extend(o.get("src").and_then(Value::as_str).map(str::to_string));
            }
            o.values().for_each(|v| frame_srcs(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| frame_srcs(v, out)),
        _ => {}
    }
}

/// Manifest `i18n`: `{ "en": { "title": "…" }, "ro": { … } }`, served as `plugin.<id>.<key>`.
pub fn validate_i18n(v: &Value) -> AppResult<()> {
    let bad = || AppError::Validation("error.addon_ui");
    let langs = v.as_object().ok_or(bad())?;
    let key_ok = |k: &str| {
        !k.is_empty()
            && k.len() <= 64
            && k.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.".contains(&b))
    };
    for (lang, m) in langs {
        let m = m.as_object().ok_or(bad())?;
        if lang.len() > 8
            || m.len() > 500
            || m.iter()
                .any(|(k, v)| !key_ok(k) || v.as_str().is_none_or(|s| s.len() > 2000))
        {
            return Err(bad());
        }
    }
    Ok(())
}

/// `plugin.<id>.*` strings of loaded addons for `lang` (falls back to `en`).
pub fn messages(s: &AppState, lang: &str) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for a in loaded_all(s) {
        let Some(i) = a.manifest.get("i18n") else {
            continue;
        };
        for l in ["en", lang] {
            for (k, v) in i.get(l).and_then(Value::as_object).into_iter().flatten() {
                out.insert(format!("plugin.{}.{k}", a.id), v.clone());
            }
        }
    }
    out
}

/// Pages and slot contributions of loaded addons, in the `/plugins/ui` shape.
pub fn ui_contributions(
    s: &AppState,
    can: impl Fn(Option<&str>, &Option<String>) -> bool,
) -> (Vec<Value>, Vec<Value>) {
    let (mut pages, mut slots) = (vec![], vec![]);
    for a in loaded_all(s) {
        let Some(ui) = a.manifest.get("ui") else {
            continue;
        };
        let Ok(ui) = parse_ui(&a.id, ui, a.caps.iframe(), None) else {
            continue;
        };
        let frame = |f: &Option<pelty_extensions::ui_schema::Frame>| {
            f.as_ref()
                .map(|f| json!({ "src": asset_url(&a.id, &f.src), "height": f.height }))
        };
        for pg in ui
            .pages
            .into_iter()
            .filter(|pg| can(Some(&pg.area), &pg.permission))
        {
            pages.push(json!({ "plugin": a.id, "addon": true, "id": pg.id, "area": pg.area, "title": pg.title, "icon": pg.icon, "nav": pg.nav, "hub": pg.hub, "data": pg.data, "body": pg.body, "frame": frame(&pg.frame), "bridge": a.caps.bridge, "same_origin": a.caps.iframe_same_origin() }));
        }
        for sl in ui
            .slots
            .into_iter()
            .filter(|sl| can(sl.slot.split('.').next(), &sl.permission))
        {
            slots.push(json!({ "plugin": a.id, "addon": true, "slot": sl.slot, "data": sl.data, "body": sl.body, "frame": frame(&sl.frame), "order": sl.order, "bridge": a.caps.bridge, "same_origin": a.caps.iframe_same_origin() }));
        }
    }
    (pages, slots)
}

/// Frontend overrides of addons holding `ui: ["override"]`, for pages the
/// viewer may see: `{pages: [{plugin, route, page}], hide_nav: [{plugin, path}]}`.
/// Several addons overriding one route: the first by addon id wins (the
/// others are listed under `conflicts`).
pub fn ui_overrides(s: &AppState, can: impl Fn(Option<&str>, &Option<String>) -> bool) -> Value {
    let (mut pages, mut hide, mut conflicts) = (Vec::<Value>::new(), vec![], vec![]);
    for a in loaded_all(s).into_iter().filter(|a| a.caps.ui_override()) {
        let Some(ui) = a.manifest.get("ui") else {
            continue;
        };
        let Ok(ui) = parse_ui(&a.id, ui, a.caps.iframe(), None) else {
            continue;
        };
        for (route, page) in &ui.overrides.pages {
            let Some(pg) = ui.pages.iter().find(|p| &p.id == page) else {
                continue;
            };
            if !can(Some(&pg.area), &pg.permission) {
                continue;
            }
            match pages.iter().find(|p| p["route"] == route.as_str()) {
                Some(w) => {
                    conflicts.push(json!({ "route": route, "winner": w["plugin"], "plugin": a.id }))
                }
                None => pages.push(json!({ "plugin": a.id, "route": route, "page": page })),
            }
        }
        hide.extend(
            ui.overrides
                .hide_nav
                .iter()
                .map(|p| json!({ "plugin": a.id, "path": p })),
        );
    }
    json!({ "pages": pages, "hide_nav": hide, "conflicts": conflicts })
}

pub(crate) fn loaded_all(s: &AppState) -> Vec<Arc<pelty_wasm::WasmAddon>> {
    let mut v: Vec<_> = s
        .wasm_addons
        .read()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v.dedup_by(|a, b| a.id == b.id);
    v
}

/// Run the synchronous pre-event `event` through every subscribed addon in
/// priority order. Each may continue, cancel (`409 error.addon_hook_cancelled`)
/// or modify the payload (an object; the caller decides which keys it honours).
/// Errors and timeouts follow the hook's `fail_open`. Money changes from an
/// addon without `money.modify` discard that addon's modification; allowed
/// ones must be exact decimals and are audited. Without the capability the
/// money keys are reverted and the rest of the change is kept.
pub async fn pre_event(s: &AppState, event: &str, payload: Value) -> AppResult<Value> {
    let subs: Vec<(Arc<pelty_wasm::WasmAddon>, pelty_wasm::Hook)> = loaded_all(s)
        .into_iter()
        .filter_map(|a| a.hook(event).cloned().map(|h| (a, h)))
        .collect();
    run_hooks(s, event, subs, payload).await
}

/// A string rewritten by a pre-event: non-blank and at most `max` bytes,
/// otherwise `default` (the submitted value).
pub fn pre_str(p: &Value, key: &str, max: usize, default: &str) -> String {
    p.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty() && v.len() <= max)
        .map(str::to_string)
        .unwrap_or_else(|| default.to_string())
}

/// An optional string: a valid string sets it, `null` clears it, anything
/// else (or a missing key) keeps `default`.
pub fn pre_opt_str(p: &Value, key: &str, max: usize, default: Option<&str>) -> Option<String> {
    match p.get(key) {
        Some(Value::String(v)) if !v.trim().is_empty() && v.len() <= max => Some(v.clone()),
        Some(Value::Null) => None,
        _ => default.map(str::to_string),
    }
}

/// An optional id: a UUID string sets it, `null` clears it, otherwise `default`.
pub fn pre_uuid(p: &Value, key: &str, default: Option<uuid::Uuid>) -> Option<uuid::Uuid> {
    match p.get(key) {
        Some(Value::String(v)) => v.parse().ok().or(default),
        Some(Value::Null) => None,
        _ => default,
    }
}

/// One of `allowed`, otherwise `default`.
pub fn pre_choice(p: &Value, key: &str, allowed: &[&str], default: &str) -> String {
    p.get(key)
        .and_then(Value::as_str)
        .filter(|v| allowed.contains(v))
        .unwrap_or(default)
        .to_string()
}

async fn run_hooks(
    s: &AppState,
    event: &str,
    subs: Vec<(Arc<pelty_wasm::WasmAddon>, pelty_wasm::Hook)>,
    payload: Value,
) -> AppResult<Value> {
    let o = crate::addon_events::run_before(s, event, subs, payload, 0).await?;
    match o.cancelled {
        Some(_) => Err(AppError::Conflict("error.addon_hook_cancelled")),
        None => Ok(o.payload),
    }
}

/// Apply an addon's `modify` to `payload` under the money rules: without
/// `money.modify` money keys are reverted (rest kept); with it every money
/// value must be exact (else the whole change is dropped) and is audited.
pub(crate) async fn apply_modify(
    s: &AppState,
    a: &pelty_wasm::WasmAddon,
    event: &str,
    payload: &Value,
    p: &Value,
) -> Value {
    let mut changes = vec![];
    money_diff("", payload, p, &mut changes);
    if changes.is_empty() {
        p.clone()
    } else if !a.caps.money.modify {
        tracing::warn!(addon = %a.id, event, "money change without money.modify capability reverted");
        let mut p = p.clone();
        restore(
            &mut p,
            changes
                .into_iter()
                .map(|(path, from, _)| (path, from))
                .collect(),
        );
        p
    } else if let Some(bad) = changes.iter().find(|c| exact_money(&c.2).is_none()) {
        tracing::warn!(addon = %a.id, event, path = %bad.0, "invalid money value ignored");
        payload.clone()
    } else {
        for (path, from, to) in &changes {
            let _ = pelty_database::audit::record(
                &s.pool,
                None,
                "addon.money_modified",
                Some(event),
                None,
                json!({ "addon": a.id, "path": path, "from": from, "to": to }),
            )
            .await;
        }
        p.clone()
    }
}

/// Keys treated as money anywhere in a payload.
pub const MONEY_KEYS: &[&str] = &[
    "amount",
    "price",
    "unit_price",
    "total",
    "subtotal",
    "tax",
    "tax_total",
    "discount",
    "setup_fee",
    "recurring_amount",
    "credit",
    "fee",
    "balance",
    "amount_paid",
];

/// Collect `(path, before, after)` for every money key whose value changed,
/// appeared or disappeared between `a` and `b`.
pub fn money_diff(path: &str, a: &Value, b: &Value, out: &mut Vec<(String, Value, Value)>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let p = format!("{path}/{k}");
                let (va, vb) = (
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                );
                if MONEY_KEYS.contains(&k.as_str()) && !va.is_object() && !va.is_array() {
                    if va != vb {
                        out.push((p, va.clone(), vb.clone()));
                    }
                } else {
                    money_diff(&p, va, vb, out);
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            for i in 0..x.len().max(y.len()) {
                money_diff(
                    &format!("{path}/{i}"),
                    x.get(i).unwrap_or(&Value::Null),
                    y.get(i).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Null, b) if b.is_object() => money_diff(path, &json!({}), b, out),
        (Value::Null, b) if b.is_array() => money_diff(path, &json!([]), b, out),
        (a, Value::Null) if a.is_object() => money_diff(path, a, &json!({}), out),
        (a, Value::Null) if a.is_array() => money_diff(path, a, &json!([]), out),
        _ => {}
    }
}

/// An exact decimal money value: a string (or integer) with at most 4
/// decimal places and |v| < 10^12. Floats like `0.1` sent as JSON numbers
/// are refused because they are not exact.
pub fn exact_money(v: &Value) -> Option<rust_decimal::Decimal> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
        _ => return None,
    };
    if s.is_empty()
        || s.len() > 32
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return None;
    }
    let d: rust_decimal::Decimal = s.parse().ok()?;
    (d.scale() <= 4 && d.abs() < rust_decimal::Decimal::from(1_000_000_000_000i64)).then_some(d)
}

/// Body keys never sent to addons (replaced by `"[redacted]"` and restored
/// afterwards, so addons can neither read nor change them).
pub const REDACT_KEYS: &[&str] = &[
    "password",
    "current_password",
    "new_password",
    "password_confirmation",
    "secret",
    "client_secret",
    "token",
    "api_key",
    "totp",
    "otp",
    "mfa_code",
    "recovery_code",
    "card_number",
    "cvc",
    "cvv",
];

pub(crate) fn redact(v: &mut Value, saved: &mut Vec<(String, Value)>, path: &str) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                let p = format!("{path}/{k}");
                if REDACT_KEYS.contains(&k.as_str())
                    || k.ends_with("_password")
                    || k.ends_with("_secret")
                    || k.ends_with("_token")
                {
                    saved.push((p, std::mem::replace(x, json!("[redacted]"))));
                } else {
                    redact(x, saved, &p);
                }
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter_mut().enumerate() {
                redact(x, saved, &format!("{path}/{i}"));
            }
        }
        _ => {}
    }
}

pub(crate) fn restore(v: &mut Value, saved: Vec<(String, Value)>) {
    for (p, x) in saved {
        if let Some(slot) = v.pointer_mut(&p) {
            *slot = x;
        } else if let Some((parent, key)) = p.rsplit_once('/') {
            if let Some(Value::Object(m)) = v.pointer_mut(parent) {
                m.insert(key.to_string(), x);
            }
        }
    }
}

/// Does a `before:` hook pattern match `METHOD template`?
pub fn before_matches(pattern: &str, method: &str, route: &str) -> bool {
    let Some(rest) = pattern.strip_prefix("before:") else {
        return false;
    };
    if rest == "*" {
        return true;
    }
    let Some((m, p)) = rest.split_once(' ') else {
        return false;
    };
    (m == "*" || m == method)
        && match p.strip_suffix('*') {
            Some(prefix) => route.starts_with(prefix),
            None => p == route,
        }
}

pub(crate) const PRE_MAX_BODY: usize = 1024 * 1024;

/// Run a queued addon job (`addon.job`): `call(op, input)` on the addon,
/// only if it is still loaded and still declares `op` in `capabilities.jobs`.
pub async fn run_job(s: &AppState, p: &Value) -> Result<Value, crate::worker::JobError> {
    let id = p["addon"].as_str().unwrap_or_default();
    let op = p["op"].as_str().unwrap_or_default();
    let Some(a) = loaded_all(s).into_iter().find(|a| a.id == id) else {
        return Err(crate::worker::JobError::Permanent(format!(
            "addon {id} not loaded"
        )));
    };
    let ctx = pelty_wasm::CallCtx {
        settings: crate::wasm_addons::addon_settings(s, &a.id).await,
        ..Default::default()
    };
    // Scheduled runs wait while the addon's call breaker is open (retried
    // with backoff, so they do not count as failed runs yet).
    let scheduled =
        p.get("cron").is_some() || p.get("scheduled").is_some() || p.get("catch_up").is_some();
    if scheduled && s.addon_health.open(id) {
        return Err(crate::worker::JobError::Retry(
            "addon circuit breaker open".into(),
        ));
    }
    // Cron job (ABI 0.5): `on-cron(name)`.
    if let Some(job) = p["cron_job"].as_str() {
        if !a.caps.cron.iter().any(|j| j.name == job)
            && !runtime_cron(id).iter().any(|j| j.0 == job)
        {
            return Err(crate::worker::JobError::Permanent(format!(
                "cron job {job} not declared"
            )));
        }
        let t = std::time::Instant::now();
        let r = a.on_cron_async(ctx, job.to_string()).await;
        let err = r.as_ref().err().map(|e| e.to_string());
        s.addon_health
            .record(id, "cron", t.elapsed(), err.as_deref(), false);
        return r.map_err(|e| crate::worker::JobError::Retry(e.to_string()));
    }
    if !a.caps.jobs.iter().any(|j| j.op == op) {
        return Err(crate::worker::JobError::Permanent(format!(
            "job {op} not declared"
        )));
    }
    a.call_async(ctx, op.to_string(), p["input"].clone())
        .await
        .map_err(|e| crate::worker::JobError::Retry(e.to_string()))
}

/// Enqueue scheduled addon jobs (`every_secs`), one per addon/op/window.
pub async fn schedule_jobs(s: &AppState) {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let gate = crate::routes::automation::gate(s).await;
    for a in loaded_all(s) {
        for j in a.caps.jobs.iter().filter(|j| {
            j.every_secs >= 60
                && gate.allows(
                    &format!("{}:{}", a.id, j.op),
                    crate::routes::automation::local_now(),
                )
        }) {
            let key = format!("addon:{}:{}:{}", a.id, j.op, now / j.every_secs as i64);
            if let Err(e) = pelty_database::jobs::enqueue(
                &s.pool,
                crate::addon_power::JOB_KIND,
                json!({ "addon": a.id, "op": j.op, "input": {}, "scheduled": true }),
                None,
                Some(&key),
                3,
            )
            .await
            {
                tracing::warn!(addon = %a.id, error = %e, "schedule addon job failed");
            }
        }
    }
}

/// Public URL of a package file under `assets/`.
pub fn asset_url(id: &str, src: &str) -> String {
    format!("/api/v1/addons/{id}/{src}")
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/addons/{id}/ui-token", axum::routing::post(ui_token))
        .route("/addons/{id}/{*rest}", any(dispatch))
}

/// CSP for addon assets: no network, no top-level navigation, and the
/// `sandbox` directive keeps the document in an opaque origin even when it
/// is opened outside the frame.
/// With the approved `ui.iframe.same_origin` capability the frame shares the
/// panel origin (it can call the API with the user's session): no CSP
/// `sandbox`, and `connect-src 'self'`.
pub const ASSET_CSP_SAME_ORIGIN: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'self'";
pub const ASSET_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors 'self'; sandbox allow-scripts allow-forms";

fn content_type(path: &str) -> Option<&'static str> {
    Some(match path.rsplit('.').next()? {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

/// `GET /api/v1/addons/<id>/assets/<path>`: a static file the package
/// ships (listed and hashed in `addon.json` `files`, checked on every read).
fn serve_asset(a: &pelty_wasm::WasmAddon, rel: &str) -> ApiResult<Response> {
    let Some(ct) = content_type(rel).filter(|_| pelty_extensions::ui_schema::asset_path_ok(rel))
    else {
        return Err(AppError::NotFound.into());
    };
    let Some(want) = a
        .manifest
        .get("files")
        .and_then(|f| f.get(rel))
        .and_then(Value::as_str)
    else {
        return Err(AppError::NotFound.into());
    };
    let bytes = std::fs::read(crate::addons::addons_dir().join(&a.id).join(rel))
        .map_err(|_| AppError::NotFound)?;
    if !want.eq_ignore_ascii_case(&pelty_extensions::addon_pkg::sha256_hex(&bytes)) {
        tracing::warn!(addon = %a.id, file = rel, "addon asset hash mismatch");
        return Err(AppError::NotFound.into());
    }
    let h = [
        (axum::http::header::CONTENT_TYPE, ct),
        (
            axum::http::header::CONTENT_SECURITY_POLICY,
            if a.caps.iframe_same_origin() {
                ASSET_CSP_SAME_ORIGIN
            } else {
                ASSET_CSP
            },
        ),
        (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (axum::http::header::REFERRER_POLICY, "no-referrer"),
        (axum::http::header::X_FRAME_OPTIONS, "SAMEORIGIN"),
        (axum::http::header::CACHE_CONTROL, "no-cache"),
        (
            axum::http::HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
    ];
    Ok((h, bytes).into_response())
}

// ----- short-lived UI token for iframe bridge calls -----

const UI_TOKEN_SECS: i64 = 300;

fn ui_key() -> &'static [u8; 32] {
    static K: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    K.get_or_init(|| pelty_auth::token::hash(&pelty_auth::token::generate()))
}

fn ui_sign(payload: &str) -> String {
    use base64::Engine as _;
    use hmac::{digest::KeyInit, Hmac, Mac};
    let mut m = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(ui_key())
        .unwrap_or_else(|_| unreachable!("hmac accepts any key length"));
    m.update(payload.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(m.finalize().into_bytes())
}

/// `<addon>.<user>.<exp>.<sig>`; valid for one addon and one user.
pub fn issue_ui_token(addon: &str, user: &str, now: i64) -> String {
    let payload = format!("{addon}.{user}.{}", now + UI_TOKEN_SECS);
    format!("{payload}.{}", ui_sign(&payload))
}

pub fn check_ui_token(t: &str, addon: &str, user: &str, now: i64) -> bool {
    let Some((payload, sig)) = t.rsplit_once('.') else {
        return false;
    };
    let mut it = payload.rsplitn(3, '.');
    let (Some(exp), Some(u), Some(a)) = (it.next(), it.next(), it.next()) else {
        return false;
    };
    let sig_ok = {
        use base64::Engine as _;
        use hmac::{Hmac, KeyInit, Mac};
        let mut m = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(ui_key())
            .unwrap_or_else(|_| unreachable!("hmac accepts any key length"));
        m.update(payload.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig)
            .is_ok_and(|raw| m.verify_slice(&raw).is_ok())
    };
    sig_ok && a == addon && u == user && exp.parse::<i64>().is_ok_and(|e| e > now)
}

/// `POST /api/v1/addons/<id>/ui-token`: for the signed-in viewer of an
/// addon iframe (`ui.iframe` only). Sent as `X-Pelty-Ui-Token` with every
/// bridge call to the addon's routes; the addon sees `input.frame = true`.
async fn ui_token(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let Some(_) = loaded_all(&s)
        .into_iter()
        .find(|a| a.id == id && a.caps.iframe())
    else {
        return Err(AppError::NotFound.into());
    };
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    Ok(Json(
        json!({ "token": issue_ui_token(&id, &u.user_id.to_string(), now), "expires_in": UI_TOKEN_SECS }),
    ))
}

async fn dispatch(
    State(s): State<AppState>,
    Path((id, rest)): Path<(String, String)>,
    req: Request,
) -> ApiResult<Response> {
    let Some(a) = loaded_all(&s).into_iter().find(|a| a.id == id) else {
        return Err(AppError::NotFound.into());
    };
    if req.method() == axum::http::Method::GET && rest.starts_with("assets/") {
        return serve_asset(&a, &rest);
    }
    let (mut parts, body) = req.into_parts();
    let method = parts.method.as_str().to_string();
    let path = format!("/{rest}");
    let Some((route, params)) = a
        .caps
        .routes
        .iter()
        .find_map(|r| r.matches(&method, &path).map(|p| (r.clone(), p)))
    else {
        return Err(AppError::NotFound.into());
    };
    // Auth and CSRF are the host's: the same extractor every core route uses.
    let user = match route.access.as_str() {
        "public" => None,
        access => {
            let u = CurrentUser::from_request_parts(&mut parts, &s).await?;
            if access == "admin" {
                let ip = parts
                    .extensions
                    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                    .map(|c| c.0.ip());
                crate::security::admin_ip_allowed(&s, ip)
                    .await
                    .map_err(AppError::Denied)?;
                if !u.has("admin.access") || !route.permission.as_deref().is_some_and(|p| u.has(p))
                {
                    return Err(AppError::Forbidden.into());
                }
                if pelty_database::staff::blocked_by_2fa(&s.pool, u.user_id).await? {
                    return Err(AppError::Denied("error.mfa_required").into());
                }
            }
            Some(u)
        }
    };
    let bytes = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| AppError::BadRequest("error.payload_too_large"))?;
    let body_json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).map_err(|_| AppError::BadRequest("error.invalid_json"))?
    };
    let query: serde_json::Map<String, Value> = axum::extract::Query::<
        std::collections::BTreeMap<String, String>,
    >::try_from_uri(&parts.uri)
    .map(|q| {
        q.0.into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect()
    })
    .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let frame = match (
        parts
            .headers
            .get("x-pelty-ui-token")
            .and_then(|v| v.to_str().ok()),
        &user,
    ) {
        (None, _) => false,
        (Some(t), Some(u)) if check_ui_token(t, &a.id, &u.user_id.to_string(), now) => true,
        _ => return Err(AppError::Forbidden.into()),
    };
    let input = json!({
        "frame": frame,
        "method": method,
        "path": path,
        "params": params.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        "query": query,
        "body": body_json,
        "user": user.as_ref().map(|u| json!({ "id": u.user_id, "roles": u.roles, "admin": u.has("admin.access") })),
    });
    let ctx = pelty_wasm::CallCtx {
        settings: crate::wasm_addons::addon_settings(&s, &a.id).await,
        ..Default::default()
    };
    let out = a.call_async(ctx, "http".into(), input).await.map_err(|e| {
        tracing::warn!(addon = %a.id, error = %e, "addon route failed");
        AppError::Internal("addon route failed".into())
    })?;
    let status = out
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|c| u16::try_from(c).ok())
        .and_then(|c| StatusCode::from_u16(c).ok())
        .filter(|c| c.as_u16() >= 200)
        .unwrap_or(StatusCode::OK);
    Ok((
        status,
        Json(out.get("body").cloned().unwrap_or(Value::Null)),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_validation() {
        let ok = json!({ "pages": [{ "id": "stats", "area": "admin", "title": "plugin.integration-x.title", "data": { "s": { "get": "/api/v1/addons/integration-x/stats" } },
                                     "body": { "type": "stack", "children": [{ "type": "button", "label": "plugin.integration-x.go", "action": { "type": "request", "path": "/api/v1/addons/integration-x/run" } }] } }],
                         "slots": [{ "slot": "client.dashboard", "body": { "type": "text", "text": "hi" } }] });
        assert!(parse_ui("integration-x", &ok, false, None).is_ok());
        for bad in [
            json!({ "pages": [{ "id": "x", "area": "root", "title": "plugin.integration-x.t", "body": { "type": "text" } }] }),
            json!({ "pages": [{ "id": "x", "area": "admin", "title": "plugin.integration-x.t", "body": { "type": "script" } }] }),
            json!({ "pages": [{ "id": "x", "area": "admin", "title": "plugin.other.t", "body": { "type": "text" } }] }),
            json!({ "pages": [{ "id": "x", "area": "admin", "title": "plugin.integration-x.t", "body": { "type": "button", "action": { "type": "request", "path": "/api/v1/admin/users/1" } } }] }),
            json!({ "pages": [{ "id": "x", "area": "admin", "title": "plugin.integration-x.t", "body": { "type": "button", "action": { "type": "request", "path": "/api/v1/plugins/integration-x/a" } } }] }),
            json!({ "slots": [{ "slot": "admin.settings", "body": { "type": "text" } }] }),
            json!({ "pages": [], "script": "x" }),
        ] {
            assert!(
                parse_ui("integration-x", &bad, false, None).is_err(),
                "{bad}"
            );
        }
        assert!(validate_i18n(&json!({ "en": { "title": "T" }, "ro": { "title": "T" } })).is_ok());
        assert!(validate_i18n(&json!({ "en": { "Bad Key": "T" } })).is_err());
    }

    #[test]
    fn ui_overrides_validation() {
        let page = |area: &str| json!({ "id": "mine", "area": area, "title": "plugin.integration-x.t", "body": { "type": "text", "text": "x" } });
        let ok = json!({ "pages": [page("client")], "overrides": { "pages": { "client.invoices": "mine" }, "hide_nav": ["/billing/quotes"] } });
        let ui = parse_ui("integration-x", &ok, false, None).unwrap();
        assert_eq!(ui.overrides.pages["client.invoices"], "mine");
        for bad in [
            // other area than the page
            json!({ "pages": [page("client")], "overrides": { "pages": { "admin.invoices": "mine" } } }),
            // unknown page, bad route key, unknown area
            json!({ "pages": [page("client")], "overrides": { "pages": { "client.invoices": "nope" } } }),
            json!({ "pages": [page("client")], "overrides": { "pages": { "client.Invoices": "mine" } } }),
            json!({ "pages": [page("client")], "overrides": { "pages": { "root.invoices": "mine" } } }),
            // nav paths must be in-app
            json!({ "overrides": { "hide_nav": ["https://evil.example"] } }),
            json!({ "overrides": { "hide_nav": ["//evil.example"] } }),
            json!({ "overrides": { "unknown": [] } }),
        ] {
            assert!(
                parse_ui("integration-x", &bad, false, None).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn iframe_ui() {
        let ui = json!({ "pages": [{ "id": "frame", "area": "client", "title": "plugin.x.t", "frame": { "src": "assets/ui.html" } }],
                         "slots": [{ "slot": "admin.invoices.toolbar", "body": { "type": "frame", "src": "assets/w.html" } }, { "slot": "admin.layout.banner", "body": { "type": "text", "text": "hi" } }] });
        assert!(parse_ui("x", &ui, false, None).is_err());
        let has = |p: &str| p == "assets/ui.html" || p == "assets/w.html";
        assert!(parse_ui("x", &ui, true, Some(&has)).is_ok());
        let only_ui = |p: &str| p == "assets/ui.html";
        assert!(parse_ui("x", &ui, true, Some(&only_ui)).is_err());
    }

    #[test]
    fn ui_tokens() {
        let t = issue_ui_token("x", "7", 1000);
        assert!(check_ui_token(&t, "x", "7", 1100));
        assert!(!check_ui_token(&t, "x", "7", 1000 + UI_TOKEN_SECS));
        assert!(!check_ui_token(&t, "y", "7", 1100));
        assert!(!check_ui_token(&t, "x", "8", 1100));
        assert!(!check_ui_token(&format!("{t}A"), "x", "7", 1100));
        assert!(!check_ui_token("garbage", "x", "7", 1100));
    }

    #[test]
    fn asset_types() {
        assert_eq!(
            content_type("assets/a.html"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(content_type("assets/a.wasm"), None);
        assert!(
            ASSET_CSP.contains("connect-src 'none'") && ASSET_CSP.contains("sandbox allow-scripts")
        );
        assert!(
            !ASSET_CSP_SAME_ORIGIN.contains("sandbox")
                && ASSET_CSP_SAME_ORIGIN.contains("frame-ancestors 'self'")
        );
    }

    #[test]
    fn caps_validation() {
        let c: pelty_wasm::AddonCapabilities = serde_json::from_value(json!({ "hooks": [{ "event": "login.before" }], "routes": [{ "method": "GET", "path": "/stats", "access": "admin", "permission": "reports.read" }] })).unwrap();
        assert!(validate_caps(&c).is_ok());
        let c: pelty_wasm::AddonCapabilities = serde_json::from_value(
            json!({ "ui": ["iframe"], "bridge": ["/api/v1/client/services"] }),
        )
        .unwrap();
        assert!(validate_caps(&c).is_ok());
        for bad in [
            json!({ "hooks": [{ "event": "user.anything" }] }),
            json!({ "routes": [{ "method": "GET", "path": "/x", "access": "admin" }] }),
            json!({ "routes": [{ "method": "GET", "path": "x/../y", "access": "public" }] }),
            json!({ "ui": ["script"] }),
            json!({ "bridge": ["/api/v1/me"] }),
            json!({ "ui": ["iframe"], "bridge": ["/etc/../x"] }),
        ] {
            let c: pelty_wasm::AddonCapabilities = serde_json::from_value(bad.clone()).unwrap();
            assert!(validate_caps(&c).is_err(), "{bad}");
        }
    }

    #[test]
    fn power_caps_validation() {
        let full: Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../extensions/addons/fixture-power/addon.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let c: pelty_wasm::AddonCapabilities =
            serde_json::from_value(full["capabilities"].clone()).unwrap();
        assert!(validate_caps(&c).is_ok());
        for ok in [
            "before:*",
            "before:POST /api/v1/admin/invoices",
            "before:* /api/v1/client/*",
            "before:DELETE /api/v1/admin/services/{id}",
            "during:POST /api/v1/admin/invoices",
            "after:*",
            "finished:invoice.paid",
            "before:fixture-lifecycle:greet",
            "after:other-addon:thing.*",
        ] {
            assert!(hook_event_ok(ok), "{ok}");
        }
        for bad in [
            "before:",
            "before:GET /api/v1/x",
            "before:POST /etc",
            "after:GET /api/v1/x",
            "before:POST /api/v1/a b",
            "during:Bad Name",
            "before:invoice.paid",
        ] {
            assert!(!hook_event_ok(bad), "{bad}");
        }
        for bad in [
            json!({ "db": { "read": ["users"] } }),
            json!({ "db": { "read": ["password_hashes"] } }),
            json!({ "db": { "write": ["invoice.delete"] } }),
            json!({ "exec": ["sh"] }),
            json!({ "exec": ["/usr/bin/../bin/sh"] }),
            json!({ "jobs": [{ "op": "hook" }] }),
            json!({ "jobs": [{ "op": "tick", "every_secs": 5 }] }),
            json!({ "ui": ["iframe.top"] }),
            json!({ "http_private": true }),
        ] {
            let c: pelty_wasm::AddonCapabilities = serde_json::from_value(bad.clone()).unwrap();
            assert!(validate_caps(&c).is_err(), "{bad}");
        }
    }
}

/// `auth_provider` addons: when local credentials fail, ask each enabled
/// provider (op `auth.verify`, input `{email, password}`, output `{ok,
/// email?, display_name?}`) to verify them. The password goes only to
/// addons that the admin approved as `auth.provider`; it is never stored.
/// A verified login maps to an existing *customer-only* account with that
/// email or creates one (random local password). Staff accounts can never
/// be logged in by an addon.
pub async fn external_login(
    s: &AppState,
    email: &str,
    password: &str,
    ip: Option<&str>,
) -> AppResult<Option<pelty_database::users::User>> {
    for a in loaded_all(s).into_iter().filter(|a| a.caps.auth_provider) {
        let ctx = pelty_wasm::CallCtx {
            settings: crate::wasm_addons::addon_settings(s, &a.id).await,
            ..Default::default()
        };
        let fut = a.call_async(
            ctx,
            "auth.verify".into(),
            json!({ "email": email, "password": password }),
        );
        let out = match tokio::time::timeout(std::time::Duration::from_secs(10), fut).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                tracing::warn!(addon = %a.id, error = %e, "auth provider failed");
                continue;
            }
            Err(_) => continue,
        };
        if out.get("ok") != Some(&Value::Bool(true)) {
            continue;
        }
        // The provider confirms the email that was typed, never another one.
        if out
            .get("email")
            .and_then(Value::as_str)
            .is_some_and(|e| !e.eq_ignore_ascii_case(email.trim()))
        {
            continue;
        }
        let user = match pelty_database::users::find_by_email(&s.pool, &email.trim().to_lowercase())
            .await?
        {
            Some(u) => {
                let roles = pelty_database::users::roles(&s.pool, u.id).await?;
                if !u.is_active
                    || pelty_database::users::is_locked(&s.pool, u.id).await?
                    || roles.iter().any(|r| r != "customer")
                {
                    tracing::warn!(addon = %a.id, "auth provider refused for a staff or inactive account");
                    return Ok(None);
                }
                u
            }
            None => {
                let name = out
                    .get("display_name")
                    .and_then(Value::as_str)
                    .filter(|n| !n.trim().is_empty() && n.len() <= 100)
                    .unwrap_or(email)
                    .to_string();
                let pw = format!(
                    "Ext-{}-{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                s.auth
                    .register(
                        pelty_auth::RegisterInput {
                            email,
                            password: &pw,
                            display_name: &name,
                        },
                        "customer",
                        ip,
                    )
                    .await?
            }
        };
        let _ = pelty_database::audit::record(
            &s.pool,
            Some(user.id),
            "auth.login_external",
            Some(&a.id),
            ip,
            json!({ "addon": a.id }),
        )
        .await;
        return Ok(Some(user));
    }
    Ok(None)
}

#[cfg(test)]
mod pre_tests {
    use super::*;

    #[test]
    fn money_and_matching() {
        assert_eq!(
            exact_money(&json!("12.3400")),
            Some("12.34".parse().unwrap())
        );
        assert_eq!(exact_money(&json!(5)), Some(5.into()));
        for bad in [
            json!(0.1),
            json!("1.00001"),
            json!("1e3"),
            json!("abc"),
            json!(null),
            json!("10000000000000"),
        ] {
            assert!(exact_money(&bad).is_none(), "{bad}");
        }
        let mut c = vec![];
        money_diff(
            "",
            &json!({"a": {"amount": "1"}, "items": [{"unit_price": "2", "d": "x"}]}),
            &json!({"a": {"amount": "1"}, "items": [{"unit_price": "3", "d": "y"}]}),
            &mut c,
        );
        assert_eq!(
            c,
            vec![("/items/0/unit_price".to_string(), json!("2"), json!("3"))]
        );
        let mut c = vec![];
        money_diff("", &json!({"x": 1}), &json!({"x": 1, "fee": "9"}), &mut c);
        assert_eq!(c.len(), 1, "added money key counts");
        assert!(before_matches("before:*", "POST", "/api/v1/x"));
        assert!(before_matches(
            "before:POST /api/v1/admin/*",
            "POST",
            "/api/v1/admin/invoices"
        ));
        assert!(!before_matches(
            "before:PUT /api/v1/admin/*",
            "POST",
            "/api/v1/admin/invoices"
        ));
        assert!(!before_matches(
            "before:* /api/v1/client/orders",
            "POST",
            "/api/v1/client/orders/x"
        ));
        assert!(!before_matches("invoice.before_create", "POST", "/x"));
    }

    #[test]
    fn secrets_redacted_and_restored() {
        let mut v =
            json!({"email": "a", "password": "p", "nested": [{"api_key": "k", "x_token": "t"}]});
        let mut saved = vec![];
        redact(&mut v, &mut saved, "");
        assert!(
            !v.to_string().contains("\"p\"")
                && !v.to_string().contains("\"k\"")
                && !v.to_string().contains("\"t\"")
        );
        v["password"] = json!("evil");
        v.as_object_mut().unwrap().remove("nested");
        restore(&mut v, saved);
        assert_eq!(v["password"], "p");
    }
}
