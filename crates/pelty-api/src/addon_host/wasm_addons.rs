//! Runtime side of WASM addons: compile the component of an installed and
//! enabled addon and register it with the matching host registry (gateways,
//! provisioning, registrars) or keep it for captcha / SSO lookups.
//! Disabling or uninstalling unregisters it again. See `make docs`.
use crate::addons::{self, CatalogEntry};
use crate::AppState;
use pelty_core::{AppError, AppResult};
use pelty_database::addon_registry as reg;
use pelty_payments::{BoxFuture, HttpRequestSpec, HttpTransport};
use std::sync::{Arc, RwLock};

/// Host HTTP for addons: whatever transport the app currently uses
/// (SSRF-guarded in production, a mock in tests), read per request.
pub struct StateHttp(pub Arc<RwLock<Arc<dyn HttpTransport>>>);

impl HttpTransport for StateHttp {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        let t = match self.0.read() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        };
        Box::pin(async move { t.send(req).await })
    }
}

/// DB-backed addon kv (`addon_kv`), values sealed with the app key when set.
pub struct PgKv {
    pub pool: pelty_database::Pool,
    pub secret_box: Option<pelty_auth::crypto::SecretBox>,
}

impl pelty_wasm::KvStore for PgKv {
    fn get<'a>(&'a self, addon: &'a str, key: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            let (v, sealed) = pelty_database::addon_kv::get(&self.pool, addon, key).await.ok()??;
            let plain = if sealed { self.secret_box.as_ref()?.open(&v)? } else { v };
            String::from_utf8(plain).ok()
        })
    }
    fn set<'a>(&'a self, addon: &'a str, key: &'a str, value: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let (bytes, sealed) = match &self.secret_box {
                Some(b) => (b.seal(value.as_bytes()), true),
                None => (value.as_bytes().to_vec(), false),
            };
            match pelty_database::addon_kv::set(&self.pool, addon, key, &bytes, sealed).await {
                Ok(true) => Ok(()),
                Ok(false) => Err("kv quota exceeded".into()),
                Err(e) => Err(format!("kv: {e}")),
            }
        })
    }
    fn delete<'a>(&'a self, addon: &'a str, key: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _ = pelty_database::addon_kv::delete(&self.pool, addon, key).await;
        })
    }
}

/// Deliver a domain event to every loaded addon subscribed to it
/// (`capabilities.events`). Failures are logged, never propagated.
pub async fn on_event(s: &AppState, event: &pelty_extensions::events::DomainEvent) {
    // Lifecycle listeners of core domain events (`after:invoice.paid`, …).
    crate::addon_events::notify(s, "after", event.name, event.payload.clone(), 0);
    crate::addon_events::notify(s, "finished", event.name, serde_json::json!({ "status": "ok", "payload": event.payload }), 0);
    let subs: Vec<Arc<pelty_wasm::WasmAddon>> = match s.wasm_addons.read() {
        Ok(m) => m.values().filter(|a| a.subscribes(event.name)).cloned().collect(),
        Err(_) => return,
    };
    // One event at a time per addon, in emit order per addon, so a handler's
    // kv read-modify-write never races another delivery to the same addon.
    static LOCKS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = std::sync::LazyLock::new(Default::default);
    for a in subs {
        let lock = LOCKS.lock().map(|mut m| m.entry(a.id.clone()).or_default().clone()).ok();
        let _guard = match &lock {
            Some(l) => Some(l.lock().await),
            None => None,
        };
        let ctx = pelty_wasm::CallCtx { settings: addon_settings(s, &a.id).await, ..Default::default() };
        if let Err(e) = a.on_event_async(ctx, event.name.to_string(), event.payload.clone()).await {
            tracing::warn!(addon = %a.id, event = %event.name, error = %e, "addon event handler failed");
        }
    }
}

/// Opened settings of an installed addon row (`{}` when missing).
pub async fn addon_settings(s: &AppState, id: &str) -> serde_json::Value {
    match reg::get(&s.pool, id).await {
        Ok(Some(r)) => crate::addon_secrets::open(s, &r.settings),
        _ => serde_json::json!({}),
    }
}

pub fn services(s: &AppState) -> pelty_wasm::HostServices {
    pelty_wasm::HostServices { http: Arc::new(StateHttp(s.http_transport.clone())), kv: s.addon_kv.clone(), power: Arc::new(crate::addon_power::DbPower(s.pool.clone())), events: Arc::new(crate::addon_events::Bus(s.clone())) }
}

fn key(kind: &str, engine: &str) -> String {
    format!("{kind}:{engine}")
}

/// Loaded captcha / SSO addon for `engine`, if enabled.
pub fn loaded(s: &AppState, kind: &str, engine: &str) -> Option<Arc<pelty_wasm::WasmAddon>> {
    s.wasm_addons.read().ok()?.get(&key(kind, engine)).cloned()
}

fn load_err(e: impl std::fmt::Display) -> AppError {
    tracing::warn!(error = %e, "wasm addon failed to load");
    AppError::Validation("error.addon_wasm")
}

/// Compile `e` and register it. Errors leave nothing registered.
pub async fn activate(s: &AppState, e: &CatalogEntry) -> AppResult<()> {
    let m = &e.manifest;
    if !m.is_wasm() {
        return Ok(());
    }
    crate::addon_trust::verify_entry(s, e)?;
    addons::verify(e)?;
    let dir = e.dir.clone().ok_or(AppError::Validation("error.addon_integrity"))?;
    let wasm_rel = m.wasm.clone().ok_or(AppError::Validation("error.addon_manifest"))?;
    let raw = serde_json::to_value(m).map_err(|e| AppError::Internal(e.to_string()))?;
    let caps: pelty_wasm::AddonCapabilities = m.capabilities.clone().map(serde_json::from_value).transpose().map_err(load_err)?.unwrap_or_default();
    let (id, kind, version) = (m.id.clone(), m.kind.clone(), m.version.clone());
    let svc = services(s);
    let limits = m.limits.clone();
    let addon = tokio::task::spawn_blocking(move || -> Result<pelty_wasm::WasmAddon, String> {
        let wasm = std::fs::read(dir.join(&wasm_rel)).map_err(|e| e.to_string())?;
        let sandbox = pelty_wasm::Sandbox::from_manifest(&kind, limits.as_ref())?;
        pelty_wasm::WasmAddon::from_bytes(&id, &kind, &version, &wasm, caps, raw, sandbox, svc).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(load_err)?;
    let addon = Arc::new(addon);
    let want = m.engine_id();
    let got: String = match m.kind.as_str() {
        "gateway" => pelty_wasm::gateway::register(&s.gateways, addon.clone()).map_err(load_err)?.id.to_string(),
        "provisioning" => pelty_wasm::provisioning::register(&s.provisioning, addon.clone()).map_err(load_err)?.to_string(),
        "registrar" => pelty_wasm::provisioning::register_registrar(&s.registrars, addon.clone()).map_err(load_err)?.to_string(),
        _ => want.to_string(),
    };
    // Every loaded addon is tracked here: captcha / SSO lookups and event
    // delivery (`capabilities.events`) for all kinds.
    if let Ok(mut g) = s.wasm_addons.write() {
        g.insert(key(&m.kind, &got), addon.clone());
    }
    if got != want {
        deactivate(s, &m.kind, &got);
        return Err(load_err(format!("addon {} describes engine {got:?}, manifest provides {want:?}", m.id)));
    }
    Ok(())
}

pub fn deactivate(s: &AppState, kind: &str, engine: &str) {
    if let Ok(mut g) = s.wasm_addons.write() {
        g.remove(&key(kind, engine));
    }
    match kind {
        "gateway" => s.gateways.unregister(engine),
        "provisioning" => {
            s.provisioning.unregister(engine);
        }
        "registrar" => {
            s.registrars.unregister(engine);
        }
        _ => {}
    }
}

/// Unregister the engine of `row` when it is a WASM addon and no other
/// enabled addon provides the same engine.
pub async fn deactivate_row(s: &AppState, row: &reg::InstalledAddon) -> AppResult<()> {
    let wasm = addons::catalog(s).iter().any(|e| e.manifest.id == row.id && e.manifest.is_wasm());
    let others = reg::list(&s.pool).await?.iter().any(|r| r.id != row.id && r.kind == row.kind && r.engine == row.engine && r.enabled);
    if wasm && !others {
        deactivate(s, &row.kind, &row.engine);
    }
    Ok(())
}

/// Startup: adopt rows from the built-in era, then load every enabled WASM addon.
pub async fn load_enabled(s: &AppState) -> AppResult<()> {
    let cat = addons::catalog(s);
    for row in reg::list(&s.pool).await? {
        let Some(e) = cat.iter().find(|e| e.manifest.id == row.id) else { continue };
        if !e.manifest.is_wasm() {
            continue;
        }
        if row.source == "builtin" {
            reg::adopt_package(&s.pool, &row.id, &e.manifest.version, &e.hash, e.source, &e.manifest.permissions).await?;
            tracing::info!(id = %row.id, "built-in addon migrated to its WASM package");
        } else if row.manifest_hash != e.hash {
            tracing::warn!(id = %row.id, "addon package changed on disk; not loaded");
            continue;
        }
        if row.enabled {
            if let Err(err) = activate(s, e).await {
                tracing::error!(id = %row.id, error = %err, "wasm addon not loaded");
            }
        }
    }
    Ok(())
}

/// Make the loaded WASM addons match the database (another instance
/// installed, enabled, disabled, upgraded or removed one): unload engines no
/// enabled addon provides any more, (re)load every enabled one.
pub async fn sync(s: &AppState) -> AppResult<()> {
    let cat = addons::catalog(s);
    let rows = reg::list(&s.pool).await?;
    let want: std::collections::HashSet<String> = rows.iter().filter(|r| r.enabled).map(|r| key(&r.kind, &r.engine)).collect();
    let loaded: Vec<String> = s.wasm_addons.read().map(|g| g.keys().cloned().collect()).unwrap_or_default();
    for k in loaded.iter().filter(|k| !want.contains(*k)) {
        if let Some((kind, engine)) = k.split_once(':') {
            deactivate(s, kind, engine);
        }
    }
    for row in rows.iter().filter(|r| r.enabled) {
        let Some(e) = cat.iter().find(|e| e.manifest.id == row.id && e.manifest.is_wasm()) else {
            if self::loaded(s, &row.kind, &row.engine).is_some() {
                deactivate(s, &row.kind, &row.engine);
            }
            continue;
        };
        if row.manifest_hash != e.hash {
            tracing::warn!(id = %row.id, "addon package differs on this instance; not loaded");
            deactivate(s, &row.kind, &row.engine);
            continue;
        }
        if let Err(err) = activate(s, e).await {
            deactivate(s, &row.kind, &row.engine);
            tracing::error!(id = %row.id, error = %err, "wasm addon not reloaded");
        }
    }
    Ok(())
}

/// Directory with the bundled first-party addon zips (`cargo xtask addons`).
pub fn dist_dir() -> std::path::PathBuf {
    // Directory with the bundled first-party addon zips (default `extensions/addons/dist`).
    std::env::var("PELTY_ADDONS_DIST").map(Into::into).unwrap_or_else(|_| std::path::PathBuf::from("extensions/addons/dist"))
}

/// Unpack every bundled zip that is missing or changed in the addons dir.
/// New ones become `available`; installed ones are upgraded in place (an
/// enabled one is re-activated by [`load_enabled`]).
pub async fn seed_bundled(s: &AppState) -> AppResult<usize> {
    let Ok(rd) = std::fs::read_dir(dist_dir()) else { return Ok(0) };
    let mut n = 0;
    let mut zips: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip")).collect();
    zips.sort();
    for p in zips {
        let bytes = std::fs::read(&p).map_err(|e| AppError::Internal(e.to_string()))?;
        let Ok(pkg) = pelty_extensions::addon_pkg::inspect(&bytes, addons::PKG_LIMITS) else {
            tracing::warn!(path = %p.display(), "bundled addon zip invalid");
            continue;
        };
        let on_disk = std::fs::read(addons::addons_dir().join(&pkg.id).join("addon.json")).ok();
        if on_disk.as_deref() == Some(pkg.manifest_raw.as_slice()) {
            continue;
        }
        match addons::accept_package_with(s, &bytes, true, true).await {
            Ok(_) => n += 1,
            Err(e) => tracing::warn!(path = %p.display(), error = %e, "bundled addon rejected"),
        }
    }
    Ok(n)
}
