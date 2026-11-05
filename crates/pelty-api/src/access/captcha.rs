//! Captcha protection for public forms (WHMCS "Captcha Form Protection").
//!
//! Providers are addons (`kind: captcha`): Cloudflare Turnstile, hCaptcha,
//! Google reCAPTCHA v2 / v3 and a built-in arithmetic challenge. The global
//! policy (`security.captcha` system setting) picks the provider, the mode
//! (`never` | `guests` | `always`) and the protected forms. Clients send the
//! widget token in the `x-captcha-token` header; verification is server-side
//! through the shared [`pelty_payments::HttpTransport`] (mockable in tests).
use crate::addons::{Manifest, SettingSpec};
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission, SESSION_COOKIE};
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use pelty_core::{AppError, AppResult};
use pelty_database::{addon_registry as reg, audit, settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use time::OffsetDateTime;

pub const SETTING_KEY: &str = "security.captcha";
pub const HEADER: &str = "x-captcha-token";
pub const FORMS: &[&str] = &["login", "register", "contact", "ticket", "checkout", "password_reset", "domain_lookup"];
pub const ENGINES: &[&str] = &["turnstile", "hcaptcha", "recaptcha_v2", "recaptcha_v3", "internal"];

/// (engine, display name, default verify URL, has score)
fn engine_info(e: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match e {
        "turnstile" => ("Cloudflare Turnstile", "https://challenges.cloudflare.com/turnstile/v0/siteverify", false),
        "hcaptcha" => ("hCaptcha", "https://api.hcaptcha.com/siteverify", true),
        "recaptcha_v2" => ("Google reCAPTCHA v2", "https://www.google.com/recaptcha/api/siteverify", false),
        "recaptcha_v3" => ("Google reCAPTCHA v3", "https://www.google.com/recaptcha/api/siteverify", true),
        "internal" => ("Built-in challenge", "", false),
        _ => return None,
    })
}

fn spec(key: &str, kind: &str, required: bool, default: Option<Value>) -> SettingSpec {
    SettingSpec { key: key.into(), label: format!("addons.captcha.setting.{key}"), kind: kind.into(), required, choices: vec![], default }
}

/// Built-in captcha addon manifests (merged into the addon catalog).
pub fn builtin_manifests() -> Vec<Manifest> {
    // Remote providers are WASM addons (extensions/addons/captcha-*); only the host-side
    // arithmetic challenge (one-time nonce in the DB) stays built in.
    ["internal"]
        .iter()
        .filter_map(|e| {
            let (name, _, score) = engine_info(e)?;
            let mut settings = vec![];
            if *e != "internal" {
                settings.push(spec("site_key", "text", false, Some(json!(""))));
                settings.push(spec("secret_key", "text", false, Some(json!(""))));
                if score {
                    settings.push(spec("min_score", "number", false, Some(json!(0.5))));
                }
                settings.push(spec("verify_url", "text", false, Some(json!(""))));
            }
            Some(Manifest {
                id: crate::addons::builtin_id("captcha", e),
                name: name.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                kind: "captcha".into(),
                engine: (*e).into(),
                description: format!("addons.desc.captcha.{e}"),
                author: "MythicalSystems".into(),
                permissions: if *e == "internal" { vec![] } else { vec!["network.outbound".into()] },
                settings,
                migrations: vec![],
                files: BTreeMap::new(),
                ..Default::default()
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct CaptchaPolicy {
    /// never | guests | always
    pub mode: String,
    /// Addon id of the provider (`captcha-turnstile`, ...), empty = none.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub forms: Vec<String>,
}
impl Default for CaptchaPolicy {
    fn default() -> Self {
        CaptchaPolicy { mode: "never".into(), provider: String::new(), forms: vec![] }
    }
}

/// Forms protected when captcha is switched on by `pelty-server setup`.
pub const SETUP_FORMS: &[&str] = &["login", "register", "password_reset", "contact"];

/// Set up captcha from the setup wizard: install and enable the provider addon
/// when needed (as the console would; bundled zips are unpacked first), store
/// the keys sealed like the admin UI does, and switch the policy to `guests`
/// on [`SETUP_FORMS`]. An empty `provider` turns captcha off.
pub async fn configure_from_setup(s: &AppState, provider: &str, site_key: &str, secret_key: &str) -> AppResult<()> {
    let policy = if provider.is_empty() {
        CaptchaPolicy::default()
    } else {
        let actor = crate::addons::Actor::CONSOLE;
        if reg::get(&s.pool, provider).await?.is_none() {
            crate::wasm_addons::seed_bundled(s).await?;
            if crate::addons::find(s, provider)?.manifest.kind != "captcha" {
                return Err(AppError::Validation("error.addon_not_installed"));
            }
            crate::addons::install_addon(s, provider, &json!({}), crate::addons::Actor::CONSOLE).await?;
        }
        let e = crate::addons::find(s, provider)?;
        if e.manifest.kind != "captcha" {
            return Err(AppError::Validation("error.addon_not_installed"));
        }
        crate::addons::set_addon_enabled(s, provider, true, actor).await?;
        let row = reg::get(&s.pool, provider).await?.ok_or(AppError::Conflict("error.addon_not_installed"))?;
        if row.engine != "internal" {
            if secret_key.is_empty() {
                return Err(AppError::Validation("error.captcha_misconfigured"));
            }
            let mut v = crate::addon_secrets::open(s, &row.settings);
            if !v.is_object() {
                v = json!({});
            }
            v["site_key"] = json!(site_key);
            v["secret_key"] = json!(secret_key);
            let clean = crate::addons::validate_settings(&e.manifest.settings, &v)?;
            let stored = crate::addon_secrets::seal(s, &e.manifest.settings, &clean, &crate::addon_secrets::open(s, &row.settings));
            reg::set_settings(&s.pool, provider, &stored).await?;
        }
        CaptchaPolicy { mode: "guests".into(), provider: provider.into(), forms: SETUP_FORMS.iter().map(|f| f.to_string()).collect() }
    };
    settings::set(&s.pool, SETTING_KEY, serde_json::to_value(&policy).map_err(AppError::internal)?).await?;
    audit::record(&s.pool, None, "security.captcha_updated", None, None, json!({ "mode": policy.mode, "provider": policy.provider, "forms": policy.forms, "via": "setup" })).await?;
    Ok(())
}

pub async fn policy(pool: &pelty_database::Pool) -> AppResult<CaptchaPolicy> {
    Ok(settings::get(pool, SETTING_KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

/// Active provider: (engine, settings) when the policy's addon is installed+enabled.
async fn active(s: &AppState, p: &CaptchaPolicy) -> AppResult<Option<(String, Value)>> {
    if p.mode == "never" || p.provider.is_empty() {
        return Ok(None);
    }
    match reg::get(&s.pool, &p.provider).await? {
        Some(a) if a.enabled && a.kind == "captcha" => Ok(Some((a.engine, crate::addon_secrets::open(s, &a.settings)))),
        _ => Ok(None),
    }
}

// ----- internal challenge (HMAC-signed, one-time) -----

fn key() -> &'static [u8; 32] {
    static K: OnceLock<[u8; 32]> = OnceLock::new();
    K.get_or_init(|| {
        let t = pelty_auth::token::generate();
        pelty_auth::token::hash(&t)
    })
}

fn sign(payload: &str) -> String {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key()).unwrap_or_else(|_| unreachable!("hmac accepts any key length"));
    m.update(payload.as_bytes());
    URL_SAFE_NO_PAD.encode(m.finalize().into_bytes())
}

/// New arithmetic challenge: (token, question).
pub fn new_challenge() -> (String, String) {
    let r = pelty_auth::token::hash(&pelty_auth::token::generate());
    let (a, b) = (2 + (r[0] % 18) as u32, 1 + (r[1] % 9) as u32);
    let exp = OffsetDateTime::now_utc().unix_timestamp() + 600;
    let nonce = URL_SAFE_NO_PAD.encode(&r[2..18]);
    let payload = URL_SAFE_NO_PAD.encode(json!({"s": a + b, "e": exp, "n": nonce}).to_string());
    (format!("{payload}.{}", sign(&payload)), format!("{a} + {b}"))
}

/// Verify `<token>:<answer>` of the internal challenge (one-time).
pub async fn verify_internal(pool: &pelty_database::Pool, value: &str) -> AppResult<bool> {
    let Some((tok, answer)) = value.rsplit_once(':') else { return Ok(false) };
    let Some((payload, sig)) = tok.split_once('.') else { return Ok(false) };
    if !pelty_auth::token::constant_eq(&sign(payload), sig) {
        return Ok(false);
    }
    let Ok(raw) = URL_SAFE_NO_PAD.decode(payload) else { return Ok(false) };
    let Ok(v) = serde_json::from_slice::<Value>(&raw) else { return Ok(false) };
    let (Some(sum), Some(exp), Some(nonce)) = (v["s"].as_u64(), v["e"].as_i64(), v["n"].as_str()) else { return Ok(false) };
    if exp < OffsetDateTime::now_utc().unix_timestamp() || answer.trim().parse::<u64>().ok() != Some(sum) {
        return Ok(false);
    }
    let exp = OffsetDateTime::from_unix_timestamp(exp).map_err(AppError::internal)?;
    pelty_database::security::consume_captcha_nonce(pool, nonce, exp).await
}

/// Remote siteverify through the provider's WASM addon. Returns Ok(false)
/// on a negative answer; Err on misconfiguration or transport failure.
async fn verify_remote(s: &AppState, engine: &str, cfg: &Value, token: &str, ip: Option<&str>) -> AppResult<bool> {
    if cfg["secret_key"].as_str().unwrap_or("").is_empty() {
        return Err(AppError::Validation("error.captcha_misconfigured"));
    }
    let addon = crate::wasm_addons::loaded(s, "captcha", engine).ok_or(AppError::Validation("error.captcha_unavailable"))?;
    let ctx = pelty_wasm::CallCtx { settings: cfg.clone(), ..Default::default() };
    let out = addon.call_async(ctx, "verify".into(), json!({ "token": token, "ip": ip, "settings": cfg })).await.map_err(|e| {
        let m = e.to_string();
        if m.contains("misconfigured") {
            return AppError::Validation("error.captcha_misconfigured");
        }
        tracing::warn!(error = %m, "captcha verify failed");
        AppError::Validation("error.captcha_unavailable")
    })?;
    Ok(out["ok"].as_bool() == Some(true))
}

/// Throttle `form` per IP, then enforce captcha if the policy protects it.
pub async fn require(s: &AppState, headers: &HeaderMap, form: &str, ip: Option<&str>) -> AppResult<()> {
    // Per-form, per-IP throttle on every protected form, captcha or not
    // (budget = the login rate from the security policy).
    if !s.auth_limiter.check(&format!("form:{form}:{}", ip.unwrap_or("unknown"))) {
        return Err(AppError::RateLimited);
    }
    let p = policy(&s.pool).await?;
    if !p.forms.iter().any(|f| f == form) {
        return Ok(());
    }
    let Some((engine, cfg)) = active(s, &p).await? else { return Ok(()) };
    if p.mode == "guests" {
        if let Some(tok) = crate::extract::cookie(headers, SESSION_COOKIE) {
            if s.auth.authenticate(&tok).await?.is_some() {
                return Ok(());
            }
        }
    }
    let token = headers.get(HEADER).and_then(|v| v.to_str().ok()).unwrap_or("").trim();
    if token.is_empty() || token.len() > 4096 {
        return Err(AppError::Validation("error.captcha_required"));
    }
    let ok = if engine == "internal" { verify_internal(&s.pool, token).await? } else { verify_remote(s, &engine, &cfg, token, ip).await? };
    if ok {
        Ok(())
    } else {
        Err(AppError::Validation("error.captcha_failed"))
    }
}

// ----- HTTP -----

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/captcha/config", get(public_config))
        .route("/captcha/challenge", get(challenge))
        .route("/admin/security/captcha", get(get_policy).put(put_policy))
}

/// Public: what the frontend must render. Never exposes secrets.
#[utoipa::path(get, path = "/api/v1/captcha/config", responses((status = 200)))]
pub async fn public_config(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let p = policy(&s.pool).await?;
    let Some((engine, cfg)) = active(&s, &p).await? else {
        return Ok(Json(json!({ "enabled": false, "forms": [] })));
    };
    Ok(Json(json!({
        "enabled": true,
        "engine": engine,
        "mode": p.mode,
        "site_key": cfg["site_key"].as_str().unwrap_or(""),
        "forms": p.forms,
        // Browser widget declared by the captcha addon (script, global, mode).
        "widget": crate::wasm_addons::loaded(&s, "captcha", &engine).map(|a| a.descriptor["widget"].clone()).unwrap_or(Value::Null),
    })))
}

#[utoipa::path(get, path = "/api/v1/captcha/challenge", responses((status = 200)))]
pub async fn challenge() -> Json<Value> {
    let (token, question) = new_challenge();
    Json(json!({ "token": token, "question": question }))
}

#[utoipa::path(get, path = "/api/v1/admin/security/captcha", responses((status = 200, body = CaptchaPolicy), (status = 403)))]
pub async fn get_policy(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    let p = policy(&s.pool).await?;
    Ok(Json(json!({ "policy": p, "forms": FORMS, "modes": ["never", "guests", "always"] })))
}

#[utoipa::path(put, path = "/api/v1/admin/security/captcha", request_body = CaptchaPolicy, responses((status = 200, body = CaptchaPolicy), (status = 422)))]
pub async fn put_policy(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(mut b): Json<CaptchaPolicy>,
) -> ApiResult<Json<CaptchaPolicy>> {
    if !["never", "guests", "always"].contains(&b.mode.as_str()) || b.forms.iter().any(|f| !FORMS.contains(&f.as_str())) {
        return Err(AppError::Validation("error.captcha_policy").into());
    }
    b.forms.sort();
    b.forms.dedup();
    if b.mode != "never" {
        match reg::get(&s.pool, &b.provider).await? {
            Some(a) if a.kind == "captcha" && a.enabled => {
                if a.engine != "internal" && a.settings["secret_key"].as_str().unwrap_or("").is_empty() {
                    return Err(AppError::Validation("error.captcha_misconfigured").into());
                }
            }
            _ => return Err(AppError::Validation("error.addon_not_installed").into()),
        }
    }
    settings::set(&s.pool, SETTING_KEY, serde_json::to_value(&b).map_err(AppError::internal)?).await?;
    audit::record(&s.pool, Some(u.user_id), "security.captcha_updated", None, u.ip.as_deref(), json!({ "mode": b.mode, "provider": b.provider, "forms": b.forms })).await?;
    Ok(Json(b))
}

