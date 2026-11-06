//! Addon system: catalog (built-in + on-disk manifests), install / enable /
//! disable / uninstall, schema-validated settings and the runtime gate that
//! keeps uninstalled provisioning modules and gateways from running.
//! See `make docs`.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::{AppError, AppResult};
use pelty_database::addon_registry::{self as reg, NewAddon};
use pelty_extensions::addon_pkg::{self, Limits, PkgError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const KINDS: &[&str] = &[
    "provisioning",
    "gateway",
    "registrar",
    "theme",
    "plugin",
    "captcha",
    "sso",
    "integration",
];
pub const PERMISSIONS: &[&str] = &[
    "payments.process",
    "provisioning.manage",
    "domains.manage",
    "network.outbound",
    "db.migrate",
    "ui.extend",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingSpec {
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// text | number | bool | select | textarea | secret (sealed at rest, masked on read)
    #[serde(default = "text_kind")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}
fn text_kind() -> String {
    "text".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub engine: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub settings: Vec<SettingSpec>,
    #[serde(default)]
    pub migrations: Vec<String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// WASM addons (`engine: "wasm"`): the engine id this addon provides
    /// (`stripe`, `pterodactyl`, ...), the WIT ABI and the component path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provides: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm: Option<String>,
    /// Sandbox capabilities approved at install (http allowlist, secrets, kv, events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    /// Declarative admin/client pages and widgets (see `addon_ext::validate_ui`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Value>,
    /// Strings for `ui`, served as `plugin.<id>.<key>`: `{ "en": {...}, "ro": {...} }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub i18n: Option<Value>,
    /// Per-addon sandbox limits (`memory_mib`, `timeout_secs`, `fuel`), at
    /// most the admin maxima (`pelty_wasm::limit_maxima`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Value>,
}

impl Manifest {
    pub fn is_wasm(&self) -> bool {
        self.engine == "wasm"
    }
    /// Engine id used by the registries and gates: `provides` for WASM addons.
    pub fn engine_id(&self) -> &str {
        if self.is_wasm() {
            self.provides.as_deref().unwrap_or_default()
        } else {
            &self.engine
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub manifest: Manifest,
    pub source: &'static str,
    /// `builtin` or sha256 of addon.json.
    pub hash: String,
    pub dir: Option<PathBuf>,
}

pub fn builtin_id(kind: &str, engine: &str) -> String {
    format!("{kind}-{}", engine.replace('_', "-"))
}

fn id_ok(s: &str) -> bool {
    (3..=64).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !s.starts_with('-')
}

pub fn addons_dir() -> PathBuf {
    // Default `data/addons`; an existing `./addons` (the pre-restructure
    // default) keeps working so older installs are not orphaned.
    std::env::var("PELTY_ADDONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let legacy = PathBuf::from("addons");
            if legacy.is_dir() {
                legacy
            } else {
                PathBuf::from("data/addons")
            }
        })
}

fn sha_hex(b: &[u8]) -> String {
    hex_lower(&Sha256::digest(b))
}
fn hex_lower(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Host-implemented addons that are not WASM (yet): see `make docs`.
fn builtins(_s: &AppState) -> Vec<CatalogEntry> {
    let mut out = vec![];
    for m in crate::captcha::builtin_manifests()
        .into_iter()
        .chain(crate::oauth::builtin_manifests())
        .chain(crate::saml::builtin_manifests())
        .chain(crate::whmcs_api::builtin_manifests())
    {
        out.push(CatalogEntry {
            manifest: m,
            source: "builtin",
            hash: "builtin".into(),
            dir: None,
        });
    }
    out
}

/// Parse and structurally validate a manifest (no file access).
pub fn validate_manifest(s: &AppState, m: &Manifest) -> AppResult<()> {
    let bad = AppError::Validation("error.addon_manifest");
    if !id_ok(&m.id)
        || m.name.trim().is_empty()
        || m.version.trim().is_empty()
        || !KINDS.contains(&m.kind.as_str())
    {
        return Err(bad);
    }
    if m.permissions
        .iter()
        .any(|p| !PERMISSIONS.contains(&p.as_str()))
    {
        return Err(AppError::Validation("error.addon_permission"));
    }
    if m.is_wasm() {
        let provides = m.provides.as_deref().unwrap_or_default();
        let abi_ok = m
            .abi
            .as_deref()
            .is_some_and(pelty_wasm::runtime::abi_supported);
        if !pelty_wasm::runtime::KINDS.contains(&m.kind.as_str())
            || !id_ok(&provides.replace('_', "-"))
            || !abi_ok
        {
            return Err(AppError::Validation("error.addon_engine"));
        }
        let Some(w) = m.wasm.as_deref() else {
            return Err(bad);
        };
        if !m.files.contains_key(w) {
            return Err(AppError::Validation("error.addon_integrity"));
        }
        if let Some(c) = &m.capabilities {
            let Ok(caps) = serde_json::from_value::<pelty_wasm::AddonCapabilities>(c.clone())
            else {
                return Err(bad);
            };
            crate::addon_ext::validate_caps(&caps)?;
        }
        if pelty_wasm::Sandbox::from_manifest(&m.kind, m.limits.as_ref()).is_err() {
            return Err(AppError::Validation("error.addon_limits"));
        }
        if let Some(ui) = &m.ui {
            let iframe = wants_iframe(m);
            let has = |p: &str| m.files.contains_key(p);
            crate::addon_ext::parse_ui(&m.id, ui, iframe, Some(&has))?;
        }
        if let Some(i) = &m.i18n {
            crate::addon_ext::validate_i18n(i)?;
        }
    }
    let engine_known = m.is_wasm()
        || match m.kind.as_str() {
            "provisioning" => s.provisioning.get(&m.engine).is_some(),
            "gateway" => s.gateways.descriptor(&m.engine).is_some(),
            "registrar" => s.registrars.get(&m.engine).is_some(),
            "captcha" => crate::captcha::ENGINES.contains(&m.engine.as_str()),
            "sso" => {
                crate::oauth::ENGINES.contains(&m.engine.as_str())
                    || m.engine == crate::saml::ENGINE
            }
            "integration" => crate::whmcs_api::ENGINES.contains(&m.engine.as_str()),
            _ => id_ok(&m.engine.replace('_', "-")),
        };
    if !engine_known {
        return Err(AppError::Validation("error.addon_engine"));
    }
    for st in &m.settings {
        if st.key.is_empty()
            || !["text", "number", "bool", "select", "textarea", "secret"]
                .contains(&st.kind.as_str())
            || (st.kind == "select" && st.choices.is_empty())
        {
            return Err(bad);
        }
    }
    let mut migration_names = std::collections::BTreeSet::new();
    for mig in &m.migrations {
        if !migration_names.insert(mig) || !m.files.contains_key(mig) {
            return Err(AppError::Validation("error.addon_integrity"));
        }
    }
    Ok(())
}

fn disk(s: &AppState) -> Vec<CatalogEntry> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(addons_dir()) else {
        return out;
    };
    for e in rd.flatten() {
        let dir = e.path();
        // `extensions/addons/<src>/` source crates (dev checkouts) are not installs.
        if dir.join("Cargo.toml").exists() {
            continue;
        }
        let Ok(raw) = std::fs::read(dir.join("addon.json")) else {
            continue;
        };
        let Ok(m) = serde_json::from_slice::<Manifest>(&raw) else {
            tracing::warn!(dir = %dir.display(), "invalid addon.json");
            continue;
        };
        if validate_manifest(s, &m).is_err() {
            tracing::warn!(id = %m.id, "addon manifest rejected");
            continue;
        }
        out.push(CatalogEntry {
            hash: sha_hex(&raw),
            manifest: m,
            source: "disk",
            dir: Some(dir),
        });
    }
    out
}

pub fn catalog(s: &AppState) -> Vec<CatalogEntry> {
    let mut all = builtins(s);
    for d in disk(s) {
        if !all.iter().any(|b| b.manifest.id == d.manifest.id) {
            all.push(d);
        }
    }
    all.sort_by(|a, b| (&a.manifest.kind, &a.manifest.id).cmp(&(&b.manifest.kind, &b.manifest.id)));
    all
}

/// Verify every declared file hash; returns (name, checksum, sql) for migrations.
pub fn verify(e: &CatalogEntry) -> AppResult<Vec<(String, String, String)>> {
    let Some(dir) = &e.dir else { return Ok(vec![]) };
    let integrity = AppError::Validation("error.addon_integrity");
    for (rel, want) in &e.manifest.files {
        if !addon_pkg::safe_rel(rel) || rel == "addon.json" || rel == "addon.sig" {
            return Err(integrity);
        }
        let b = std::fs::read(dir.join(rel))
            .map_err(|_| AppError::Validation("error.addon_integrity"))?;
        if !sha_hex(&b).eq_ignore_ascii_case(want) {
            return Err(integrity);
        }
    }
    let prefix = format!("addon_{}_", e.manifest.id.replace('-', "_"));
    let mut out = vec![];
    for name in &e.manifest.migrations {
        let sql = std::fs::read_to_string(dir.join(name))
            .map_err(|_| AppError::Validation("error.addon_integrity"))?;
        if !migration_safe(&sql, &prefix) {
            return Err(AppError::Validation("error.addon_migration"));
        }
        out.push((name.clone(), e.manifest.files[name].clone(), sql));
    }
    Ok(out)
}

/// Addon migrations may only create tables/indexes named `addon_<id>_*`.
pub fn migration_safe(sql: &str, prefix: &str) -> bool {
    crate::addon_upgrade::migration_safe(sql, prefix)
}

/// Validate settings against the schema; fills defaults, rejects unknown keys.
pub fn validate_settings(spec: &[SettingSpec], v: &Value) -> AppResult<Value> {
    let bad = AppError::Validation("error.addon_settings");
    let obj = v
        .as_object()
        .ok_or(AppError::Validation("error.addon_settings"))?;
    if obj.keys().any(|k| !spec.iter().any(|s| &s.key == k)) {
        return Err(bad);
    }
    let mut out = Map::new();
    for s in spec {
        let val = obj
            .get(&s.key)
            .filter(|x| !x.is_null() && *x != "")
            .cloned()
            .or_else(|| s.default.clone());
        let Some(val) = val else {
            if s.required {
                return Err(AppError::Validation("error.addon_settings"));
            }
            continue;
        };
        let ok = match s.kind.as_str() {
            "bool" => val.is_boolean(),
            "number" => {
                val.is_number()
                    || val
                        .as_str()
                        .is_some_and(|x| x.trim().parse::<f64>().is_ok())
            }
            "select" => val
                .as_str()
                .is_some_and(|x| s.choices.iter().any(|c| c == x)),
            _ => val.as_str().is_some_and(|x| x.len() <= 10_000),
        };
        if !ok {
            return Err(AppError::Validation("error.addon_settings"));
        }
        out.insert(s.key.clone(), val);
    }
    Ok(Value::Object(out))
}

// ----- runtime gate -----

pub async fn provisioning_enabled(s: &AppState, module: &str) -> AppResult<bool> {
    if s.provisioning.get(module).is_none() {
        return Ok(false);
    }
    reg::engine_enabled(&s.pool, "provisioning", module).await
}

/// Registrar module `id`, configured with its addon settings, when the
/// registrar addon is installed and enabled; `None` otherwise.
pub async fn registrar(
    s: &AppState,
    id: &str,
) -> AppResult<Option<std::sync::Arc<dyn pelty_provisioning::registrar::RegistrarModule>>> {
    let Some(m) = s.registrars.get(id) else {
        return Ok(None);
    };
    let Some(settings) = reg::engine_settings(&s.pool, "registrar", id).await? else {
        return Ok(None);
    };
    let settings = crate::addon_secrets::open(s, &settings);
    Ok(Some(m.configure(&settings).unwrap_or(m)))
}

/// True when some catalog addon (installed or not) provides `engine` of `kind`.
pub fn provided(s: &AppState, kind: &str, engine: &str) -> bool {
    catalog(s)
        .iter()
        .any(|e| e.manifest.kind == kind && e.manifest.engine_id() == engine)
}

/// `id` is a payment_gateways row id (`stripe`, `custom-foo`).
pub async fn gateway_enabled(s: &AppState, id: &str) -> AppResult<bool> {
    let Some(d) = s.gateways.descriptor(id) else {
        return Ok(false);
    };
    reg::engine_enabled(&s.pool, "gateway", d.id).await
}

/// Install + enable every bundled first-party addon (`extensions/addons/dist/*.zip`,
/// plus host-implemented ones). Used by tests and `pelty-server --install-builtin-addons`.
pub async fn install_builtins(s: &AppState) -> AppResult<()> {
    crate::wasm_addons::seed_bundled(s).await?;
    let bundled: Vec<String> = std::fs::read_dir(crate::wasm_addons::dist_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    e.path()
                        .file_stem()
                        .map(|x| x.to_string_lossy().into_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    for e in catalog(s) {
        if e.source != "builtin" && !bundled.contains(&e.manifest.id) {
            continue;
        }
        if reg::get(&s.pool, &e.manifest.id).await?.is_none() {
            install_entry(s, &e).await.or_else(|err| {
                if matches!(err, AppError::Conflict(_)) {
                    Ok(())
                } else {
                    Err(err)
                }
            })?;
        }
        reg::set_enabled(&s.pool, &e.manifest.id, true).await?;
        crate::wasm_addons::activate(s, &e).await?;
    }
    Ok(())
}

async fn install_entry(s: &AppState, e: &CatalogEntry) -> AppResult<()> {
    crate::addon_trust::verify_entry(s, e)?;
    let migs = verify(e)?;
    let defaults = validate_settings(&e.manifest.settings, &json!({})).unwrap_or(json!({}));
    reg::install(
        &s.pool,
        &NewAddon {
            id: &e.manifest.id,
            kind: &e.manifest.kind,
            engine: e.manifest.engine_id(),
            version: &e.manifest.version,
            source: e.source,
            settings: &defaults,
            permissions: &e.manifest.permissions,
            manifest_hash: &e.hash,
        },
        &migs,
    )
    .await
}

// ----- HTTP -----

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/addons", get(list))
        .route("/admin/addons/{id}", get(detail).delete(uninstall))
        .route("/admin/addons/{id}/install", post(install))
        .route("/admin/addons/{id}/enable", post(enable))
        .route("/admin/addons/{id}/disable", post(disable))
        .route(
            "/admin/addons/{id}/settings",
            get(get_settings).put(put_settings),
        )
        .route("/admin/addons/mixins", get(mixins_list))
        .route("/admin/addons/{id}/mixins", post(mixins_toggle))
        .route("/admin/addons/health", get(health_all))
        .route("/admin/addons/{id}/health", get(health_one))
        .route("/admin/addons/{id}/health/reset", post(health_reset))
        .route(
            "/admin/addons/upload",
            post(upload).layer(DefaultBodyLimit::max(
                pelty_core::config::upload_limits()
                    .addon
                    .min(PKG_LIMITS.max_zip_bytes)
                    + 1024,
            )),
        )
        .route("/admin/addons/registry", get(registry_index))
        .route(
            "/admin/addons/registry/settings",
            get(registry_settings).put(put_registry_settings),
        )
        .route(
            "/admin/addons/registry/oauth2/link",
            get(registry_oauth2_link),
        )
        .route(
            "/admin/addons/registry/oauth2/callback",
            post(registry_oauth2_callback),
        )
        .route(
            "/admin/addons/registry/oauth2/disconnect",
            post(registry_oauth2_disconnect),
        )
        .route(
            "/admin/addons/registry/{id}/install",
            post(registry_install),
        )
}

// ----- zip upload & registry (marketplace) -----

/// Upload limits: WASM packages may be up to 64 MiB; others stay at 10 MiB
/// (checked after the manifest is read, see [`accept_package`]).
pub const PKG_LIMITS: Limits = Limits {
    max_zip_bytes: 64 * 1024 * 1024,
    max_unpacked_bytes: 128 * 1024 * 1024,
    max_entries: 1000,
};
pub const NON_WASM_MAX_ZIP: usize = 10 * 1024 * 1024;
const REGISTRY_KEY: &str = "addons.registry";
const INDEX_MAX: usize = 2 * 1024 * 1024;
/// Official MythicalSystems marketplace (same host family FeatherPanel uses for Mythic addons).
pub const DEFAULT_MYTHICAL_REGISTRY: &str =
    "https://market.mythicalsystems.org/api/pelty/addons/registry.json";
/// Mythic team OAuth consent page (same endpoint FeatherPanel uses).
pub const DEFAULT_MYTHIC_OAUTH_URL: &str = "https://my.mythicalsystems.org/oauth2";

fn pkg_err(e: PkgError) -> AppError {
    match e {
        PkgError::TooLarge => AppError::Validation("error.addon_too_large"),
        PkgError::BadZip => AppError::Validation("error.addon_zip"),
        PkgError::UnsafePath(_) => AppError::Validation("error.addon_unsafe_path"),
        PkgError::BadManifest => AppError::Validation("error.addon_manifest"),
        PkgError::Signature(_) => AppError::Validation("error.addon_signature"),
        PkgError::Integrity(_) | PkgError::Undeclared(_) => {
            AppError::Validation("error.addon_integrity")
        }
        PkgError::Io(m) => AppError::Internal(format!("addon extract: {m}")),
    }
}

#[derive(Deserialize, Default)]
struct UploadQuery {
    #[serde(default)]
    replace: bool,
}

/// Validate a package and place it in `PELTY_ADDONS_DIR/<id>`. The addon is
/// then `available` in the catalog; installing stays an explicit step.
pub async fn accept_package(s: &AppState, bytes: &[u8], replace: bool) -> AppResult<CatalogEntry> {
    accept_package_with(s, bytes, replace, false).await
}

/// `restore`: bundled first-party package (startup seeding), may replace an
/// enabled install in place; migration checks still apply.
pub(crate) async fn accept_package_with(
    s: &AppState,
    bytes: &[u8],
    replace: bool,
    restore: bool,
) -> AppResult<CatalogEntry> {
    let pkg = addon_pkg::inspect(bytes, PKG_LIMITS).map_err(pkg_err)?;
    crate::addon_trust::verify_package(s, &pkg)?;
    let m: Manifest = serde_json::from_slice(&pkg.manifest_raw)
        .map_err(|_| AppError::Validation("error.addon_manifest"))?;
    validate_manifest(s, &m)?;
    let _lock = crate::addon_upgrade::lock(&s.pool, &m.id).await?;
    if !m.is_wasm() && bytes.len() > NON_WASM_MAX_ZIP {
        return Err(AppError::Validation("error.addon_too_large"));
    }
    if builtins(s).iter().any(|b| b.manifest.id == m.id) {
        return Err(AppError::Conflict("error.addon_builtin"));
    }
    let prefix = format!("addon_{}_", m.id.replace('-', "_"));
    for name in &m.migrations {
        let sql = pkg
            .files
            .get(name)
            .and_then(|b| std::str::from_utf8(b).ok())
            .ok_or(AppError::Validation("error.addon_integrity"))?;
        if !migration_safe(sql, &prefix) {
            return Err(AppError::Validation("error.addon_migration"));
        }
    }
    if let Some(row) = reg::get(&s.pool, &m.id)
        .await?
        .filter(|r| r.source != "builtin")
    {
        if !replace {
            return Err(AppError::Conflict("error.addon_installed"));
        }
        if row.enabled && !restore {
            return Err(AppError::Conflict("error.addon_disable_first"));
        }
        let applied = reg::applied_migrations(&s.pool, &m.id).await?;
        if m.migrations.iter().any(|x| !applied.contains(x)) {
            // New schema needs a clean reinstall; never half-apply on upgrade.
            return Err(AppError::Conflict("error.addon_upgrade_migrations"));
        }
    }
    crate::addon_upgrade::check(s, &m, restore).await?;
    let root = addons_dir();
    if root.join(&m.id).exists() && !replace {
        return Err(AppError::Conflict("error.addon_exists"));
    }
    // Never unpack over an addon's source crate (dev checkouts where the
    // addons dir points into the repo's `extensions/addons/`); set PELTY_ADDONS_DIR instead.
    if root.join(&m.id).join("Cargo.toml").exists() {
        tracing::warn!(id = %m.id, dir = %root.display(), "addons dir holds source crates; set PELTY_ADDONS_DIR");
        return Err(AppError::Conflict("error.addon_dir_source"));
    }
    let dir = tokio::task::spawn_blocking(move || addon_pkg::extract(&pkg, &root, true))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(pkg_err)?;
    let e = CatalogEntry {
        hash: sha_hex(
            &std::fs::read(dir.join("addon.json"))
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        manifest: m,
        source: "disk",
        dir: Some(dir),
    };
    verify(&e)?;
    if reg::get(&s.pool, &e.manifest.id).await?.is_some() {
        reg::set_package(&s.pool, &e.manifest.id, &e.manifest.version, &e.hash).await?;
    }
    Ok(e)
}

/// Who performed an addon action, for the audit log. `user_id: None` is
/// the server console (`pelty-server` stdin), audited with `"via": "console"`.
#[derive(Clone, Copy)]
pub struct Actor<'a> {
    pub user_id: Option<uuid::Uuid>,
    pub ip: Option<&'a str>,
}

impl Actor<'static> {
    pub const CONSOLE: Actor<'static> = Actor {
        user_id: None,
        ip: None,
    };
}

impl<'a> From<&'a crate::extract::CurrentUser> for Actor<'a> {
    fn from(u: &'a crate::extract::CurrentUser) -> Self {
        Actor {
            user_id: Some(u.user_id),
            ip: u.ip.as_deref(),
        }
    }
}

impl Actor<'_> {
    async fn record(&self, s: &AppState, action: &str, id: &str, mut meta: Value) -> AppResult<()> {
        if self.user_id.is_none() {
            meta["via"] = json!("console");
        }
        pelty_database::audit::record(&s.pool, self.user_id, action, Some(id), self.ip, meta).await
    }
}

/// [`accept_package`] + audit (`addon.uploaded`).
pub async fn upload_package(
    s: &AppState,
    bytes: &[u8],
    replace: bool,
    actor: Actor<'_>,
) -> AppResult<CatalogEntry> {
    let e = accept_package(s, bytes, replace).await?;
    actor
        .record(
            s,
            "addon.uploaded",
            &e.manifest.id,
            json!({ "version": e.manifest.version, "sha256": sha_hex(bytes) }),
        )
        .await?;
    Ok(e)
}

async fn upload(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let e = upload_package(&s, &body, q.replace, Actor::from(&u)).await?;
    let row = reg::get(&s.pool, &e.manifest.id).await?;
    Ok(Json(entry_json(&e, row.as_ref())))
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct RegistrySettings {
    #[serde(default)]
    url: String,
    /// MythicCloud / panel public key (`X-Panel-Public-Key`) for paid addon downloads.
    #[serde(default)]
    panel_public_key: String,
    /// MythicCloud / panel private key (`X-Panel-Private-Key`).
    #[serde(default)]
    panel_private_key: String,
    /// Mythic team UUID returned by the OAuth finish callback.
    #[serde(default)]
    team_uuid: String,
    /// Mythic user who authorized the link.
    #[serde(default)]
    mythic_user_id: String,
    #[serde(default)]
    team_name: String,
    #[serde(default)]
    linked_at: String,
    /// Soft lock while an OAuth relink is in flight (RFC3339).
    #[serde(default)]
    oauth_pending_at: String,
}

/// True only when a Mythic team is bound and both panel identity keys are present.
fn panel_is_linked(cfg: &RegistrySettings) -> bool {
    !cfg.team_uuid.trim().is_empty()
        && !cfg.panel_public_key.is_empty()
        && !cfg.panel_private_key.is_empty()
}

fn registry_public_json(cfg: &RegistrySettings) -> Value {
    json!({
        "url": cfg.url,
        "panel_public_key": cfg.panel_public_key,
        "panel_private_key": if cfg.panel_private_key.is_empty() { "" } else { "********" },
        "panel_linked": panel_is_linked(cfg),
        "oauth_pending": oauth_pending_recent(&cfg.oauth_pending_at) && !panel_is_linked(cfg),
        "team_uuid": cfg.team_uuid,
        "team_name": cfg.team_name,
        "mythic_user_id": cfg.mythic_user_id,
        "linked_at": cfg.linked_at,
    })
}

fn generate_panel_identity_keys() -> AppResult<(String, String)> {
    use base64::Engine;
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut pub_raw = [0u8; 18];
    let mut priv_raw = [0u8; 48];
    rng.fill(&mut pub_raw)
        .map_err(|_| AppError::Internal("rng".into()))?;
    rng.fill(&mut priv_raw)
        .map_err(|_| AppError::Internal("rng".into()))?;
    let public = format!(
        "FCPUB-{}",
        pub_raw
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>()
    );
    let private = format!(
        "FCPRIV-{}",
        base64::engine::general_purpose::STANDARD.encode(priv_raw)
    );
    Ok((public, private))
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn oauth_pending_recent(pending_at: &str) -> bool {
    let Ok(ts) = time::OffsetDateTime::parse(
        pending_at.trim(),
        &time::format_description::well_known::Rfc3339,
    ) else {
        return false;
    };
    let age = time::OffsetDateTime::now_utc() - ts;
    !age.is_negative() && age < time::Duration::minutes(30)
}

async fn load_registry(s: &AppState) -> AppResult<RegistrySettings> {
    let mut cfg: RegistrySettings = pelty_database::settings::get(&s.pool, REGISTRY_KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if cfg.url.trim().is_empty() {
        cfg.url = DEFAULT_MYTHICAL_REGISTRY.to_string();
    }
    Ok(cfg)
}

async fn save_registry(s: &AppState, cfg: &RegistrySettings) -> AppResult<()> {
    pelty_database::settings::set(&s.pool, REGISTRY_KEY, json!(cfg)).await
}

async fn registry_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    let cfg = load_registry(&s).await?;
    Ok(Json(registry_public_json(&cfg)))
}

async fn put_registry_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Json(v): Json<RegistrySettings>,
) -> ApiResult<Json<Value>> {
    // Empty → official Mythical marketplace. Custom URLs stay allowed for
    // self-hosted mirrors / integration tests; the admin UI only offers Mythical.
    let mut url = v.url.trim().to_string();
    if url.is_empty() {
        url = DEFAULT_MYTHICAL_REGISTRY.to_string();
    }
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.len() > 500
        || reqwest_like_has_creds(&url)
    {
        return Err(AppError::Validation("error.invalid_url").into());
    }
    let prev = load_registry(&s).await?;
    let panel_public_key = v
        .panel_public_key
        .trim()
        .chars()
        .take(200)
        .collect::<String>();
    let incoming_priv = v.panel_private_key.trim();
    let panel_private_key = if incoming_priv.is_empty() || incoming_priv == "********" {
        prev.panel_private_key
    } else {
        incoming_priv.chars().take(400).collect::<String>()
    };
    let keys_cleared = panel_public_key.is_empty() || panel_private_key.is_empty();
    let cfg = RegistrySettings {
        url: url.clone(),
        panel_public_key: panel_public_key.clone(),
        panel_private_key: panel_private_key.clone(),
        team_uuid: if keys_cleared {
            String::new()
        } else {
            prev.team_uuid
        },
        mythic_user_id: if keys_cleared {
            String::new()
        } else {
            prev.mythic_user_id
        },
        team_name: if keys_cleared {
            String::new()
        } else {
            prev.team_name
        },
        linked_at: if keys_cleared {
            String::new()
        } else {
            prev.linked_at
        },
        oauth_pending_at: if keys_cleared {
            String::new()
        } else {
            prev.oauth_pending_at
        },
    };
    save_registry(&s, &cfg).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "addon.registry_updated",
        None,
        u.ip.as_deref(),
        json!({
            "url": url,
            "panel_linked": panel_is_linked(&cfg),
        }),
    )
    .await?;
    Ok(Json(registry_public_json(&cfg)))
}

/// FeatherPanel-shaped MythicCloud OAuth start: mint panel identity keys, redirect to Mythic consent.
#[utoipa::path(
    get,
    path = "/api/v1/admin/addons/registry/oauth2/link",
    responses((status = 200), (status = 403), (status = 409), (status = 500))
)]
pub async fn registry_oauth2_link(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    let mut cfg = load_registry(&s).await?;
    if cfg.panel_public_key.is_empty() && cfg.panel_private_key.is_empty() {
        let (public, private) = generate_panel_identity_keys()?;
        cfg.panel_public_key = public;
        cfg.panel_private_key = private;
    } else if cfg.panel_public_key.is_empty() || cfg.panel_private_key.is_empty() {
        return Err(AppError::Conflict("error.addon_registry_panel_incomplete").into());
    }
    cfg.oauth_pending_at = now_rfc3339();
    save_registry(&s, &cfg).await?;

    let general = pelty_database::general_settings::load(&s.pool).await.ok();
    let panel_name = general
        .as_ref()
        .map(|g| g.general.company_name.trim())
        .filter(|n| !n.is_empty())
        .unwrap_or("PeltyCMS")
        .to_string();
    let panel_url = s.cfg.public_url.trim_end_matches('/').to_string();
    let logo_raw = general
        .as_ref()
        .map(|g| g.general.logo_url.trim().to_string())
        .unwrap_or_default();
    // Mythic consent page needs an absolute logo URL; lift same-origin media paths.
    let logo_url = if logo_raw.starts_with("https://") || logo_raw.starts_with("http://") {
        logo_raw
    } else if logo_raw.starts_with('/') {
        format!("{panel_url}{logo_raw}")
    } else {
        String::new()
    };
    let callback_url = format!("{panel_url}/admin/marketplace/finish");
    let oauth_base = DEFAULT_MYTHIC_OAUTH_URL;
    let mut params = vec![
        ("panel_url", panel_url.as_str()),
        ("panel_name", panel_name.as_str()),
        ("public_identity_key", cfg.panel_public_key.as_str()),
        ("public_key", cfg.panel_public_key.as_str()),
        ("private_key", cfg.panel_private_key.as_str()),
        ("callback_url", callback_url.as_str()),
        ("redirect_url", callback_url.as_str()),
    ];
    if !logo_url.is_empty() {
        params.push(("panel_logo", logo_url.as_str()));
        params.push(("logo_url", logo_url.as_str()));
    }
    let qs = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    let oauth2_url = format!("{oauth_base}?{qs}");

    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "addon.registry_oauth_started",
        None,
        u.ip.as_deref(),
        // Never log the OAuth URL: it carries panel private_key query params.
        json!({ "panel_url": panel_url, "callback_url": callback_url }),
    )
    .await?;

    Ok(Json(json!({
        "oauth2_url": oauth2_url,
        "oauth_base_url": oauth_base,
        "callback_url": callback_url,
    })))
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, utoipa::ToSchema)]
pub struct OAuthCallbackBody {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub public_identity_key: String,
    #[serde(default)]
    pub panel_public_key: String,
    #[serde(default)]
    pub private_key: String,
    #[serde(default)]
    pub panel_private_key: String,
    #[serde(default)]
    pub mythic_user_id: String,
    #[serde(default)]
    pub user_uuid: String,
    #[serde(default)]
    pub authorizer_user_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub team_uuid: String,
    #[serde(default)]
    pub team_name: String,
    #[serde(default)]
    pub team_slug: String,
}

/// Mythic OAuth finish: persist identity keys + team link (same contract as FeatherPanel).
#[utoipa::path(
    post,
    path = "/api/v1/admin/addons/registry/oauth2/callback",
    request_body = OAuthCallbackBody,
    responses((status = 200), (status = 400), (status = 403), (status = 422))
)]
pub async fn registry_oauth2_callback(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Json(body): Json<OAuthCallbackBody>,
) -> ApiResult<Json<Value>> {
    let status = body.status.trim().to_ascii_lowercase();
    if matches!(status.as_str(), "error" | "cancelled" | "cancel") {
        // Drop the soft lock so reconnect is not stuck until the 30m window expires.
        let mut cfg = load_registry(&s).await?;
        if !cfg.oauth_pending_at.is_empty() {
            cfg.oauth_pending_at.clear();
            save_registry(&s, &cfg).await?;
        }
        return Ok(Json(json!({
            "cancelled": true,
            "panel_linked": panel_is_linked(&cfg),
            "oauth_pending": false,
        })));
    }

    let mut identity_public = first_nonempty([&body.public_identity_key, &body.panel_public_key]);
    let mut identity_private = first_nonempty([&body.panel_private_key, &body.private_key]);
    let mythic_user_id = first_nonempty([
        &body.mythic_user_id,
        &body.user_uuid,
        &body.authorizer_user_id,
        &body.user_id,
    ]);
    let team_uuid = body.team_uuid.trim().chars().take(80).collect::<String>();
    if team_uuid.is_empty() || mythic_user_id.is_empty() {
        return Err(AppError::Validation("error.addon_registry_oauth_missing").into());
    }

    let mut cfg = load_registry(&s).await?;
    if identity_public.is_empty() {
        identity_public = cfg.panel_public_key.clone();
    }
    if identity_private.is_empty() {
        identity_private = cfg.panel_private_key.clone();
    }
    if identity_public.is_empty() || identity_private.is_empty() {
        return Err(AppError::Validation("error.addon_registry_oauth_missing").into());
    }

    let has_pair = !cfg.panel_public_key.is_empty() && !cfg.panel_private_key.is_empty();
    if has_pair
        && (!constant_time_eq(cfg.panel_public_key.as_bytes(), identity_public.as_bytes())
            || !constant_time_eq(cfg.panel_private_key.as_bytes(), identity_private.as_bytes()))
        && !oauth_pending_recent(&cfg.oauth_pending_at)
    {
        return Err(AppError::Forbidden.into());
    }

    let linked_at = now_rfc3339();
    cfg.panel_public_key = identity_public.chars().take(200).collect();
    cfg.panel_private_key = identity_private.chars().take(400).collect();
    cfg.team_uuid = team_uuid.clone();
    cfg.mythic_user_id = mythic_user_id.chars().take(80).collect();
    cfg.team_name = first_nonempty([&body.team_name, &body.team_slug])
        .chars()
        .take(120)
        .collect();
    cfg.linked_at = linked_at.clone();
    cfg.oauth_pending_at.clear();
    if cfg.url.trim().is_empty() {
        cfg.url = DEFAULT_MYTHICAL_REGISTRY.to_string();
    }
    save_registry(&s, &cfg).await?;

    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "addon.registry_oauth_linked",
        None,
        u.ip.as_deref(),
        json!({ "team_uuid": team_uuid, "linked_at": linked_at }),
    )
    .await?;

    Ok(Json(json!({
        "linked": true,
        "team_uuid": cfg.team_uuid,
        "team_name": cfg.team_name,
        "mythic_user_id": cfg.mythic_user_id,
        "linked_at": cfg.linked_at,
        "panel_linked": panel_is_linked(&cfg),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/addons/registry/oauth2/disconnect",
    responses((status = 200), (status = 403))
)]
pub async fn registry_oauth2_disconnect(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    let mut cfg = load_registry(&s).await?;
    cfg.panel_public_key.clear();
    cfg.panel_private_key.clear();
    cfg.team_uuid.clear();
    cfg.mythic_user_id.clear();
    cfg.team_name.clear();
    cfg.linked_at.clear();
    cfg.oauth_pending_at.clear();
    save_registry(&s, &cfg).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "addon.registry_oauth_disconnected",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(registry_public_json(&cfg)))
}

fn first_nonempty<const N: usize>(vals: [&str; N]) -> String {
    vals.into_iter()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn reqwest_like_has_creds(url: &str) -> bool {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .is_some_and(|auth| auth.contains('@'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryItem {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub download_url: String,
    pub sha256: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub icon_url: String,
    #[serde(default)]
    pub price: String,
    #[serde(default = "registry_item_free")]
    pub is_free: bool,
    #[serde(default)]
    pub purchase_url: String,
}

fn registry_item_free() -> bool {
    true
}

fn registry_auth_headers(cfg: &RegistrySettings) -> Vec<(&str, &str)> {
    let mut h = Vec::new();
    if !cfg.panel_public_key.is_empty() && !cfg.panel_private_key.is_empty() {
        h.push(("X-Panel-Public-Key", cfg.panel_public_key.as_str()));
        h.push(("X-Panel-Private-Key", cfg.panel_private_key.as_str()));
    }
    h
}

fn registry_fetch_url(cfg: &RegistrySettings) -> String {
    let url = cfg.url.trim().to_string();
    if cfg.panel_public_key.is_empty() || cfg.panel_private_key.is_empty() {
        return url;
    }
    // Prefer MythicCloud-authenticated catalog when panel keys are linked.
    if url.contains("/api/pelty/addons/registry.json") {
        return url.replace(
            "/api/pelty/addons/registry.json",
            "/api/pelty/cloud/addons/registry.json",
        );
    }
    url
}

async fn fetch_index(s: &AppState) -> AppResult<Vec<RegistryItem>> {
    let cfg = load_registry(s).await?;
    if cfg.url.is_empty() {
        return Err(AppError::Conflict("error.addon_registry_unset"));
    }
    let fetch_url = registry_fetch_url(&cfg);
    let hdrs = registry_auth_headers(&cfg);
    let raw = pelty_provisioning::http::get_bytes_with_headers(&fetch_url, INDEX_MAX, &hdrs)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "addon registry fetch failed");
            AppError::Validation("error.addon_registry_fetch")
        })?;
    let v: Value = serde_json::from_slice(&raw)
        .map_err(|_| AppError::Validation("error.addon_registry_format"))?;
    let arr = v
        .get("addons")
        .cloned()
        .ok_or(AppError::Validation("error.addon_registry_format"))?;
    let items: Vec<RegistryItem> = serde_json::from_value(arr)
        .map_err(|_| AppError::Validation("error.addon_registry_format"))?;
    Ok(items
        .into_iter()
        .filter(|i| {
            addon_pkg::id_ok(&i.id)
                && i.sha256.len() == 64
                && i.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && KINDS.contains(&i.kind.as_str())
        })
        .take(1000)
        .collect())
}

async fn registry_index(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    let items = fetch_index(&s).await?;
    let cat = catalog(&s);
    let rows = reg::list(&s.pool).await?;
    let out: Vec<Value> = items
        .iter()
        .map(|i| {
            let local = cat.iter().find(|e| e.manifest.id == i.id);
            json!({
                "id": i.id, "name": i.name, "version": i.version, "kind": i.kind, "description": i.description,
                "author": i.author, "homepage": i.homepage, "sha256": i.sha256,
                "icon_url": i.icon_url, "price": i.price, "is_free": i.is_free, "purchase_url": i.purchase_url,
                "local_version": local.map(|e| e.manifest.version.clone()),
                "builtin": local.is_some_and(|e| e.source == "builtin"),
                "installed": rows.iter().any(|r| r.id == i.id),
            })
        })
        .collect();
    Ok(Json(json!({ "items": out })))
}

async fn registry_install(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Query(q): Query<UploadQuery>,
) -> ApiResult<Json<Value>> {
    let item = fetch_index(&s)
        .await?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or(AppError::NotFound)?;
    if !item.is_free {
        let cfg = load_registry(&s).await?;
        if !panel_is_linked(&cfg) {
            return Err(AppError::Conflict("error.addon_registry_panel_required").into());
        }
    }
    let cfg = load_registry(&s).await?;
    let hdrs = registry_auth_headers(&cfg);
    let bytes = pelty_provisioning::http::get_bytes_with_headers(
        &item.download_url,
        PKG_LIMITS.max_zip_bytes,
        &hdrs,
    )
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, id = %id, "addon download failed");
        AppError::Validation("error.addon_registry_fetch")
    })?;
    if !sha_hex(&bytes).eq_ignore_ascii_case(&item.sha256) {
        return Err(AppError::Validation("error.addon_integrity").into());
    }
    let e = accept_package(&s, &bytes, q.replace).await?;
    if e.manifest.id != item.id {
        return Err(AppError::Validation("error.addon_manifest").into());
    }
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "addon.downloaded",
        Some(&id),
        u.ip.as_deref(),
        json!({ "version": e.manifest.version, "sha256": item.sha256 }),
    )
    .await?;
    let row = reg::get(&s.pool, &id).await?;
    Ok(Json(entry_json(&e, row.as_ref())))
}

fn entry_json(e: &CatalogEntry, row: Option<&reg::InstalledAddon>) -> Value {
    let status = match row {
        None => "available",
        Some(r) if r.enabled => "enabled",
        Some(_) => "disabled",
    };
    json!({
        "id": e.manifest.id, "name": e.manifest.name, "version": e.manifest.version,
        "kind": e.manifest.kind, "engine": e.manifest.engine, "description": e.manifest.description,
        "author": e.manifest.author, "permissions": e.manifest.permissions, "settings_schema": e.manifest.settings,
        "source": e.source, "hash": e.hash, "status": status,
        "installed_version": row.map(|r| r.version.clone()),
        "installed_at": row.map(|r| r.installed_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()),
        "integrity_ok": row.is_none_or(|r| r.manifest_hash == e.hash),
        // WASM sandbox: what the admin approves at install.
        "wasm": e.manifest.is_wasm(),
        "provides": e.manifest.provides, "abi": e.manifest.abi,
        "capabilities": e.manifest.is_wasm().then(|| {
            let c: pelty_wasm::AddonCapabilities = e.manifest.capabilities.clone().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
            let pages = e.manifest.ui.as_ref().map_or(0, |u| u.get("pages").and_then(Value::as_array).map_or(0, Vec::len) + u.get("slots").and_then(Value::as_array).map_or(0, Vec::len));
            let mut v = json!({ "http": c.http, "secrets": c.secrets, "kv": c.kv, "events": c.events, "hooks": c.hooks, "routes": c.routes, "pages": pages, "ui": c.ui, "bridge": c.bridge,
                "http_private": c.http_private, "money": c.money, "db": c.db, "fs": c.fs, "exec": c.exec, "jobs": c.jobs, "auth_provider": c.auth_provider, "emits": c.emits, "mixins": c.mixins,
                "dangerous": c.dangerous(), "exec_blocked": !c.exec.is_empty() && pelty_wasm::in_container(), "limits": e.manifest.limits });
            v["filters"] = json!(c.filters);
            v
        }),
    })
}

pub(crate) fn find(s: &AppState, id: &str) -> AppResult<CatalogEntry> {
    catalog(s)
        .into_iter()
        .find(|e| e.manifest.id == id)
        .ok_or(AppError::NotFound)
}

/// The catalog as shown by `GET /admin/addons` (one JSON object per addon, with `status`).
pub async fn list_entries(s: &AppState) -> AppResult<Vec<Value>> {
    let rows = reg::list(&s.pool).await?;
    Ok(catalog(s)
        .iter()
        .map(|e| entry_json(e, rows.iter().find(|r| r.id == e.manifest.id)))
        .collect())
}

async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": list_entries(&s).await? })))
}

async fn detail(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let e = find(&s, &id)?;
    let row = reg::get(&s.pool, &id).await?;
    Ok(Json(entry_json(&e, row.as_ref())))
}

/// Does the manifest ask for sandboxed iframes (`capabilities.ui: ["iframe"]`)?
pub fn wants_iframe(m: &Manifest) -> bool {
    m.capabilities
        .as_ref()
        .and_then(|c| serde_json::from_value::<pelty_wasm::AddonCapabilities>(c.clone()).ok())
        .is_some_and(|c| c.iframe())
}

/// Dangerous capabilities the request did not approve.
/// Body: `{ "approve": ["ui.iframe", "db.read", ...] }` (`approve_iframe:
/// true` is accepted for `ui.iframe`).
pub fn unapproved(m: &Manifest, opts: &Value) -> Vec<&'static str> {
    let caps: pelty_wasm::AddonCapabilities = m
        .capabilities
        .as_ref()
        .and_then(|c| serde_json::from_value(c.clone()).ok())
        .unwrap_or_default();
    let list: Vec<&str> = opts
        .get("approve")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let legacy = opts.get("approve_iframe") == Some(&Value::Bool(true));
    caps.dangerous()
        .into_iter()
        .filter(|d| !(list.contains(d) || (legacy && *d == "ui.iframe")))
        .collect()
}

/// Every dangerous capability (red warning in the install dialog) must be
/// ticked by the admin; otherwise 422 `error.addon_capability_approval`
/// (`error.addon_iframe_approval` when only the iframe is missing).
async fn install(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let opts: Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body).map_err(|_| AppError::BadRequest("error.invalid_json"))?
    };
    let e = install_addon(&s, &id, &opts, Actor::from(&u)).await?;
    let row = reg::get(&s.pool, &id).await?;
    Ok(Json(entry_json(&e, row.as_ref())))
}

/// Dangerous capabilities of a manifest (each must be approved at install).
pub fn dangerous_caps(m: &Manifest) -> Vec<&'static str> {
    m.capabilities
        .as_ref()
        .and_then(|c| serde_json::from_value::<pelty_wasm::AddonCapabilities>(c.clone()).ok())
        .map(|c| c.dangerous())
        .unwrap_or_default()
}

/// Install a catalog addon. `opts` carries the capability approvals (see [`unapproved`]).
pub async fn install_addon(
    s: &AppState,
    id: &str,
    opts: &Value,
    actor: Actor<'_>,
) -> AppResult<CatalogEntry> {
    let _lock = crate::addon_upgrade::lock(&s.pool, id).await?;
    let e = find(s, id)?;
    if reg::get(&s.pool, id).await?.is_some() {
        return Err(AppError::Conflict("error.addon_installed"));
    }
    let missing = unapproved(&e.manifest, opts);
    if missing == ["ui.iframe"] {
        return Err(AppError::Validation("error.addon_iframe_approval"));
    }
    if !missing.is_empty() {
        return Err(AppError::Validation("error.addon_capability_approval"));
    }
    install_entry(s, &e).await?;
    actor.record(s, "addon.installed", id, json!({})).await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "addon.installed",
        json!({ "addon_id": id, "version": e.manifest.version, "kind": e.manifest.kind }),
    ));
    let approved = dangerous_caps(&e.manifest);
    if !approved.is_empty() {
        let _ = actor
            .record(
                s,
                "addon.capabilities_approved",
                id,
                json!({ "approved": approved }),
            )
            .await;
    }
    Ok(e)
}

async fn toggle(
    s: &AppState,
    u: &crate::extract::CurrentUser,
    id: &str,
    on: bool,
) -> ApiResult<StatusCode> {
    set_addon_enabled(s, id, on, Actor::from(u)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Enable (after re-verifying integrity) or disable an installed addon.
pub async fn set_addon_enabled(
    s: &AppState,
    id: &str,
    on: bool,
    actor: Actor<'_>,
) -> AppResult<()> {
    let _lock = crate::addon_upgrade::lock(&s.pool, id).await?;
    let row = reg::get(&s.pool, id)
        .await?
        .ok_or(AppError::Conflict("error.addon_not_installed"))?;
    if on {
        // Re-verify integrity before letting it run.
        let e = catalog(s)
            .into_iter()
            .find(|e| e.manifest.id == id)
            .ok_or(AppError::Validation("error.addon_integrity"))?;
        {
            if e.hash != row.manifest_hash {
                return Err(AppError::Validation("error.addon_integrity"));
            }
            crate::addon_trust::verify_entry(s, &e)?;
            verify(&e)?;
            crate::wasm_addons::activate(s, &e).await?;
        }
    } else {
        if row.kind == "gateway" {
            reg::disable_gateway_rows(&s.pool, &row.engine, pelty_payments::CUSTOM_PREFIX).await?;
        }
        crate::wasm_addons::deactivate_row(s, &row).await?;
    }
    reg::set_enabled(&s.pool, id, on).await?;
    actor
        .record(
            s,
            if on {
                "addon.enabled"
            } else {
                "addon.disabled"
            },
            id,
            json!({}),
        )
        .await?;
    if on {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "addon.enabled",
            json!({ "addon_id": id }),
        ));
    } else {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "addon.disabled",
            json!({ "addon_id": id }),
        ));
    }
    Ok(())
}

async fn enable(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    toggle(&s, &u, &id, true).await
}
async fn disable(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    toggle(&s, &u, &id, false).await
}

/// Mixins of loaded addons, their kill-switch state and conflicts.
async fn mixins_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = s
        .wasm_addons
        .read()
        .map(|m| m.values().filter(|a| !a.caps.mixins.is_empty()).map(|a| json!({ "addon": a.id, "enabled": crate::addon_events::mixins_enabled(&s, &a.id), "mixins": a.caps.mixins })).collect())
        .unwrap_or_default();
    Ok(Json(
        json!({ "items": items, "conflicts": crate::addon_events::conflicts(&s) }),
    ))
}

#[derive(serde::Deserialize)]
struct MixinToggle {
    enabled: bool,
}

/// Call statistics and circuit-breaker state of every addon that ran a
/// hook, mixin or filter since start.
async fn health_all(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = s
        .addon_health
        .all()
        .into_iter()
        .map(|(id, st)| json!({ "addon": id, "stats": st }))
        .collect();
    Ok(Json(json!({ "items": items })))
}

async fn health_one(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "addon": id, "stats": s.addon_health.get(&id) }),
    ))
}

/// Close the breaker and clear the counters.
async fn health_reset(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    s.addon_health.reset(&id);
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "addon.health_reset",
        Some(&id),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Kill switch: `{"enabled": false}` stops every mixin of the addon at once.
async fn mixins_toggle(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Json(b): Json<MixinToggle>,
) -> ApiResult<StatusCode> {
    crate::addon_events::set_mixins_enabled(&s, &id, b.enabled).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        if b.enabled {
            "addon.mixins_enabled"
        } else {
            "addon.mixins_disabled"
        },
        Some(&id),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn uninstall(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    uninstall_addon(&s, &id, Actor::from(&u)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Uninstall an addon (refused while products / domains still use its engine).
pub async fn uninstall_addon(s: &AppState, id: &str, actor: Actor<'_>) -> AppResult<()> {
    let _lock = crate::addon_upgrade::lock(&s.pool, id).await?;
    let row = reg::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    // Another installed addon providing the same engine keeps it available.
    let others = reg::list(&s.pool)
        .await?
        .iter()
        .any(|r| r.id != row.id && r.kind == row.kind && r.engine == row.engine);
    if others {
        reg::remove(&s.pool, id).await?;
        actor.record(s, "addon.uninstalled", id, json!({})).await?;
        s.emit(pelty_extensions::events::DomainEvent::new(
            "addon.uninstalled",
            json!({ "addon_id": id }),
        ));
        return Ok(());
    }
    if row.kind == "provisioning" && reg::products_using_module(&s.pool, &row.engine).await? > 0 {
        return Err(AppError::Conflict("error.addon_in_use"));
    }
    if row.kind == "registrar" && reg::registrar_in_use(&s.pool, &row.engine).await? > 0 {
        return Err(AppError::Conflict("error.addon_in_use"));
    }
    if row.kind == "gateway" {
        reg::disable_gateway_rows(&s.pool, &row.engine, pelty_payments::CUSTOM_PREFIX).await?;
    }
    crate::wasm_addons::deactivate_row(s, &row).await?;
    pelty_database::addon_kv::clear(&s.pool, id).await?;
    // Its cron jobs: runtime registrations and edited schedules.
    sqlx::query("DELETE FROM addon_cron_jobs WHERE addon_id = $1")
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    sqlx::query("DELETE FROM automation_settings WHERE kind LIKE $1")
        .bind(format!("{}:%", id.replace('%', "\\%").replace('_', "\\_")))
        .execute(&s.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    reg::remove(&s.pool, id).await?;
    actor.record(s, "addon.uninstalled", id, json!({})).await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "addon.uninstalled",
        json!({ "addon_id": id }),
    ));
    Ok(())
}

async fn get_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let e = find(&s, &id)?;
    let row = reg::get(&s.pool, &id)
        .await?
        .ok_or(AppError::Conflict("error.addon_not_installed"))?;
    let values = crate::addon_secrets::mask(
        &e.manifest.settings,
        &crate::addon_secrets::open(&s, &row.settings),
    );
    Ok(Json(
        json!({ "schema": e.manifest.settings, "values": values }),
    ))
}

async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> ApiResult<Json<Value>> {
    let e = find(&s, &id)?;
    let row = reg::get(&s.pool, &id)
        .await?
        .ok_or(AppError::Conflict("error.addon_not_installed"))?;
    let clean = validate_settings(&e.manifest.settings, &v)?;
    let stored = crate::addon_secrets::seal(
        &s,
        &e.manifest.settings,
        &clean,
        &crate::addon_secrets::open(&s, &row.settings),
    );
    reg::set_settings(&s.pool, &id, &stored).await?;
    Actor::from(&u)
        .record(&s, "addon.settings_updated", &id, json!({}))
        .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "addon.settings_updated",
        json!({ "addon_id": id }),
    ));
    Ok(Json(
        json!({ "values": crate::addon_secrets::mask(&e.manifest.settings, &clean) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_limited_to_prefix() {
        let p = "addon_hello_";
        assert!(migration_safe("CREATE TABLE addon_hello_log (id int);\n-- c\nCREATE INDEX addon_hello_ix ON addon_hello_log (id);", p));
        assert!(!migration_safe("CREATE TABLE users2 (id int);", p));
        assert!(!migration_safe("DROP TABLE users;", p));
        assert!(!migration_safe(
            "CREATE TABLE addon_hello_x (id int); DELETE FROM users",
            p
        ));
        assert!(!migration_safe(
            "CREATE INDEX addon_hello_ix ON users (id)",
            p
        ));
        assert!(!migration_safe(
            "CREATE TABLE addon_hello_x (u uuid references users(id))",
            p
        ));
        assert!(!migration_safe("", p));
    }

    #[test]
    fn settings_validated() {
        let spec = vec![
            SettingSpec {
                key: "url".into(),
                label: String::new(),
                kind: "text".into(),
                required: true,
                choices: vec![],
                default: None,
            },
            SettingSpec {
                key: "mode".into(),
                label: String::new(),
                kind: "select".into(),
                required: false,
                choices: vec!["a".into(), "b".into()],
                default: Some(json!("a")),
            },
            SettingSpec {
                key: "on".into(),
                label: String::new(),
                kind: "bool".into(),
                required: false,
                choices: vec![],
                default: None,
            },
        ];
        assert_eq!(
            validate_settings(&spec, &json!({"url": "x"})).unwrap(),
            json!({"url": "x", "mode": "a"})
        );
        assert!(validate_settings(&spec, &json!({})).is_err());
        assert!(validate_settings(&spec, &json!({"url": "x", "mode": "c"})).is_err());
        assert!(validate_settings(&spec, &json!({"url": "x", "on": "yes"})).is_err());
        assert!(validate_settings(&spec, &json!({"url": "x", "zzz": 1})).is_err());
    }

    #[test]
    fn builtin_ids() {
        assert_eq!(
            builtin_id("provisioning", "https_agent"),
            "provisioning-https-agent"
        );
        assert_eq!(builtin_id("gateway", "custom_http"), "gateway-custom-http");
    }
}
