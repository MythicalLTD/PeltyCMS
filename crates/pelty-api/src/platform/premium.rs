//! PeltyCMS Premium client (optional `premium` module).
//!
//! Premium is a monthly subscription per installation sold through
//! MyMythical. The install links to a Mythic team in Admin → Marketplace
//! (MythicCloud panel keys), then asks MyMythical for an **entitlement**:
//! a short-lived token (`base64url(claims).base64url(ed25519 signature)`)
//! bound to this install's id. The token is cached in `system_settings` and
//! refreshed by a daily job and by Admin → Premium → Check now.
//!
//! Verification is **soft** (owner's choice): when MyMythical cannot be
//! reached, answers with an error, or the cached token expired, Premium
//! features keep working and staff see a warning. Premium switches off only
//! when MyMythical answers with a valid signed token that says the install
//! has no active subscription, or when no valid token was ever received.
//!
//! While the module is off nothing is sent to MyMythical.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, feature_modules, jobs, settings};
use pelty_payments::HttpRequestSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "premium";
pub const JOB_KIND: &str = "system.premium_check";
const INSTALL: &str = "premium.install";
const STATE: &str = "premium.state";
const PREFS: &str = "premium.prefs";
const ADDONS: &str = "premium.addons";
/// MyMythical path (below `PELTY_PREMIUM_URL`) that issues entitlements.
pub const ENTITLEMENT_PATH: &str = "/api/pelty/cloud/premium/entitlement";
/// Where admins subscribe when MyMythical did not send a team-specific link.
pub const DEFAULT_BUY_URL: &str = "https://my.mythicalsystems.org";
const TOKEN_MAX: usize = 16 * 1024;

/// Official MyMythical entitlement signing keys (key id → base64 ed25519 public key).
/// Rotate by adding the new key here first, then switching MyMythical's
/// `PELTYCMS_PREMIUM_SIGNING_KEY_ID`.
pub const OFFICIAL_KEYS: &[(&str, &str)] = &[(
    "mythic-2026-11",
    "9cOEsMwpr/1TiN+jE7yaJdegHLuFl7FN/RpPQhYsGwI=",
)];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(default)]
pub struct Entitlements {
    /// Hide "Powered by PeltyCMS" (white-label).
    pub white_label: bool,
    /// Install exclusive premium addons from the official marketplace.
    pub premium_addons: bool,
    /// Priority support on MyMythical.
    pub priority_support: bool,
    /// Higher limits on MythicalSystems services for this install.
    pub higher_limits: bool,
}

/// Signed claims issued by MyMythical.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Claims {
    pub v: u32,
    pub kid: String,
    pub iss: String,
    pub product: String,
    pub install_id: String,
    pub team_uuid: String,
    pub team_name: String,
    pub active: bool,
    pub plan: String,
    /// Why the install is not active (`no_subscription`, `no_seat`, ...).
    pub reason: String,
    pub entitlements: Entitlements,
    pub seats: i64,
    pub seats_used: i64,
    pub manage_url: String,
    pub support_url: String,
    /// Unix seconds.
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cached {
    token: String,
    checked_at: Option<String>,
    ok_at: Option<String>,
    /// Last failure code (`error.premium_*`), cleared on success.
    error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Prefs {
    /// White-label: hide "Powered by PeltyCMS" on the public site (needs the entitlement).
    pub hide_powered_by: bool,
    /// Optional "Source" link kept in the footer while white-labelled (AGPL section 13
    /// asks modified installs to offer their source to network users).
    pub source_url: String,
}

/// Effective Premium state after soft verification.
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct Effective {
    pub active: bool,
    pub entitlements: Entitlements,
    /// `check_failed` | `expired` | `premium_lapsed_addons` (soft warnings only).
    pub warning: Option<String>,
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn trusted_keys(s: &AppState) -> Vec<(String, String)> {
    OFFICIAL_KEYS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .chain(s.cfg.premium_public_keys.iter().cloned())
        .collect()
}

/// Verify a token's signature against `keys` and decode its claims.
pub fn verify_token(token: &str, keys: &[(String, String)]) -> Result<Claims, &'static str> {
    if token.is_empty() || token.len() > TOKEN_MAX {
        return Err("error.premium_token");
    }
    let (payload, sig) = token.split_once('.').ok_or("error.premium_token")?;
    let raw = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "error.premium_token")?;
    let claims: Claims = serde_json::from_slice(&raw).map_err(|_| "error.premium_token")?;
    let sig = URL_SAFE_NO_PAD
        .decode(sig)
        .map_err(|_| "error.premium_token")?;
    let key = keys
        .iter()
        .find(|(id, _)| *id == claims.kid)
        .ok_or("error.premium_unknown_key")?;
    let pk = STANDARD
        .decode(key.1.trim())
        .map_err(|_| "error.premium_unknown_key")?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, pk)
        .verify(payload.as_bytes(), &sig)
        .map_err(|_| "error.premium_signature")?;
    if claims.product != "peltycms_premium" || claims.v != 1 {
        return Err("error.premium_token");
    }
    Ok(claims)
}

/// This install's stable Premium id (created on first use).
pub async fn install_id(s: &AppState) -> AppResult<String> {
    if let Some(id) = settings::get(&s.pool, INSTALL)
        .await?
        .and_then(|v| v["install_id"].as_str().map(str::to_string))
        .filter(|v| Uuid::parse_str(v).is_ok())
    {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    settings::set(
        &s.pool,
        INSTALL,
        json!({ "install_id": id, "created_at": rfc3339(now()) }),
    )
    .await?;
    Ok(id)
}

async fn cached(s: &AppState) -> AppResult<Cached> {
    Ok(settings::get(&s.pool, STATE)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

async fn save_cached(s: &AppState, c: &Cached) -> AppResult<()> {
    settings::set(
        &s.pool,
        STATE,
        serde_json::to_value(c).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

pub async fn prefs(s: &AppState) -> AppResult<Prefs> {
    Ok(settings::get(&s.pool, PREFS)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// Claims of the cached token when it is genuine and bound to this install.
async fn cached_claims(s: &AppState) -> AppResult<(Cached, Option<Claims>)> {
    let c = cached(s).await?;
    if c.token.is_empty() {
        return Ok((c, None));
    }
    let id = install_id(s).await?;
    let claims = verify_token(&c.token, &trusted_keys(s))
        .ok()
        .filter(|cl| cl.install_id == id);
    Ok((c, claims))
}

fn effective_from(enabled: bool, c: &Cached, claims: Option<&Claims>, at: i64) -> Effective {
    let Some(cl) = claims.filter(|_| enabled) else {
        return Effective::default();
    };
    if !cl.active {
        return Effective::default();
    }
    let warning = if c.error.is_some() {
        Some("check_failed".to_string())
    } else if cl.exp <= at {
        Some("expired".to_string())
    } else {
        None
    };
    Effective {
        active: true,
        entitlements: cl.entitlements.clone(),
        warning,
    }
}

/// Current Premium state (module off → inactive).
pub async fn effective(s: &AppState) -> AppResult<Effective> {
    let enabled = feature_modules::is_enabled(&s.pool, MODULE).await?;
    if !enabled {
        return Ok(Effective::default());
    }
    let (c, claims) = cached_claims(s).await?;
    let mut e = effective_from(true, &c, claims.as_ref(), now().unix_timestamp());
    if !e.active && e.warning.is_none() && !premium_addons(s).await?.is_empty() {
        e.warning = Some("premium_lapsed_addons".into());
    }
    Ok(e)
}

/// White-label settings for the public site (all off unless entitled).
pub async fn white_label(s: &AppState) -> AppResult<Value> {
    let e = effective(s).await?;
    let p = prefs(s).await?;
    let on = e.entitlements.white_label && p.hide_powered_by;
    Ok(
        json!({ "hide_powered_by": on, "source_url": if on { p.source_url } else { String::new() } }),
    )
}

async fn premium_addons(s: &AppState) -> AppResult<Vec<String>> {
    Ok(settings::get(&s.pool, ADDONS)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// Remember an installed premium addon (soft warning when Premium lapses).
pub async fn remember_addon(s: &AppState, id: &str) -> AppResult<()> {
    let mut list = premium_addons(s).await?;
    if !list.iter().any(|x| x == id) {
        list.push(id.to_string());
        list.sort();
        settings::set(&s.pool, ADDONS, json!(list)).await?;
    }
    Ok(())
}

/// Ask MyMythical for a fresh entitlement and cache it.
pub async fn check(s: &AppState) -> AppResult<Value> {
    if !feature_modules::is_enabled(&s.pool, MODULE).await? {
        return status(s).await;
    }
    let mut c = cached(s).await?;
    let t = now();
    c.checked_at = Some(rfc3339(t));
    let Some(link) = crate::addons::mythic_link(s).await? else {
        c.error = Some("error.premium_not_linked".into());
        save_cached(s, &c).await?;
        return status(s).await;
    };
    let id = install_id(s).await?;
    let body = json!({
        "install_id": id,
        "team_uuid": link.team_uuid,
        "version": env!("CARGO_PKG_VERSION"),
        "public_url": s.cfg.public_url,
    });
    let res = s
        .http()
        .send(HttpRequestSpec {
            method: "POST",
            url: format!("{}{ENTITLEMENT_PATH}", s.cfg.premium_url),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "application/json".into()),
                ("x-panel-public-key".into(), link.public_key.clone()),
                ("x-panel-private-key".into(), link.private_key.clone()),
            ],
            body: body.to_string(),
        })
        .await;
    let was = cached_claims(s).await?.1;
    let outcome: Result<(String, Claims), &'static str> = match res {
        Ok((200..=299, text)) => serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.pointer("/data/token")
                    .or_else(|| v.get("token"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .ok_or("error.premium_response")
            .and_then(|tok| {
                let cl = verify_token(&tok, &trusted_keys(s))?;
                if cl.install_id != id {
                    return Err("error.premium_install_mismatch");
                }
                Ok((tok, cl))
            }),
        Ok((401 | 403, _)) => Err("error.premium_auth"),
        Ok(_) => Err("error.premium_unavailable"),
        Err(_) => Err("error.premium_unavailable"),
    };
    match outcome {
        Ok((tok, cl)) => {
            c.token = tok;
            c.ok_at = c.checked_at.clone();
            c.error = None;
            save_cached(s, &c).await?;
            if was.as_ref().map(|w| (w.active, &w.entitlements))
                != Some((cl.active, &cl.entitlements))
            {
                audit::record(
                    &s.pool,
                    None,
                    "premium.entitlement_changed",
                    None,
                    None,
                    json!({ "active": cl.active, "reason": cl.reason, "entitlements": cl.entitlements }),
                )
                .await?;
            }
        }
        Err(code) => {
            // Soft: keep the last good token, only record the failure.
            tracing::warn!(error = code, "premium entitlement check failed");
            c.error = Some(code.into());
            save_cached(s, &c).await?;
        }
    }
    status(s).await
}

/// Admin status payload.
pub async fn status(s: &AppState) -> AppResult<Value> {
    let enabled = feature_modules::is_enabled(&s.pool, MODULE).await?;
    let (c, claims) = cached_claims(s).await?;
    let eff = effective(s).await?;
    let link = crate::addons::mythic_link(s).await?;
    let p = prefs(s).await?;
    let cl = claims.unwrap_or_default();
    let exp = (cl.exp > 0)
        .then(|| {
            OffsetDateTime::from_unix_timestamp(cl.exp)
                .ok()
                .map(rfc3339)
        })
        .flatten();
    Ok(json!({
        "enabled": enabled,
        "install_id": install_id(s).await?,
        "linked": link.is_some(),
        "team_name": link.as_ref().map(|l| l.team_name.clone()).filter(|n| !n.is_empty()).unwrap_or(cl.team_name.clone()),
        "active": eff.active,
        "warning": eff.warning,
        "entitlements": eff.entitlements,
        "plan": cl.plan,
        "reason": if cl.reason.is_empty() { None } else { Some(cl.reason.clone()) },
        "seats": cl.seats,
        "seats_used": cl.seats_used,
        "expires_at": exp,
        "checked_at": c.checked_at,
        "last_ok_at": c.ok_at,
        "error": c.error,
        "buy_url": if cl.manage_url.starts_with("https://") { cl.manage_url.clone() } else { DEFAULT_BUY_URL.to_string() },
        "support_url": if eff.entitlements.priority_support && cl.support_url.starts_with("https://") { Some(cl.support_url.clone()) } else { None },
        "premium_addons": premium_addons(s).await?,
        "prefs": p,
    }))
}

pub async fn schedule(s: &AppState) -> AppResult<Option<i64>> {
    if !feature_modules::is_enabled(&s.pool, MODULE).await? {
        return Ok(None);
    }
    let key = format!("{JOB_KIND}:{}", now().date());
    jobs::enqueue(&s.pool, JOB_KIND, json!({}), None, Some(&key), 2).await
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/premium", get(get_status))
        .route("/admin/premium/check", post(check_now))
        .route("/admin/premium/settings", put(put_prefs))
}

#[utoipa::path(get, path = "/api/v1/admin/premium", responses((status = 200), (status = 403), (status = 404)))]
pub async fn get_status(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(status(&s).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/premium/check", responses((status = 200), (status = 403), (status = 404)))]
pub async fn check_now(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(check(&s).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/premium/settings", request_body = Prefs, responses((status = 200), (status = 403), (status = 404), (status = 409), (status = 422)))]
pub async fn put_prefs(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<Prefs>,
) -> ApiResult<Json<Value>> {
    let source_url = b.source_url.trim().to_string();
    if !source_url.is_empty()
        && (!(source_url.starts_with("https://") || source_url.starts_with("http://"))
            || source_url.len() > 500
            || source_url.contains(char::is_whitespace))
    {
        return Err(AppError::Validation("error.premium_source_url").into());
    }
    if b.hide_powered_by && !effective(&s).await?.entitlements.white_label {
        return Err(AppError::Conflict("error.premium_required").into());
    }
    let p = Prefs {
        hide_powered_by: b.hide_powered_by,
        source_url,
    };
    settings::set(
        &s.pool,
        PREFS,
        serde_json::to_value(&p).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "premium.settings",
        None,
        u.ip.as_deref(),
        json!({ "hide_powered_by": p.hide_powered_by, "source_url": p.source_url }),
    )
    .await?;
    Ok(Json(status(&s).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(claims: &Claims, seed: &[u8; 32]) -> String {
        let kp = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed).unwrap();
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        let sig = kp.sign(payload.as_bytes());
        format!("{payload}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
    }

    fn key(seed: &[u8; 32]) -> (String, String) {
        use ring::signature::KeyPair;
        let kp = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed).unwrap();
        ("test".into(), STANDARD.encode(kp.public_key().as_ref()))
    }

    fn claims(active: bool, exp: i64) -> Claims {
        Claims {
            v: 1,
            kid: "test".into(),
            product: "peltycms_premium".into(),
            install_id: "i".into(),
            active,
            exp,
            entitlements: Entitlements {
                white_label: true,
                premium_addons: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn verifies_signature_and_rejects_tampering() {
        let seed = [7u8; 32];
        let t = sign(&claims(true, 10), &seed);
        assert!(verify_token(&t, &[key(&seed)]).unwrap().active);
        // Wrong key.
        assert_eq!(
            verify_token(&t, &[key(&[8u8; 32])]),
            Err("error.premium_signature")
        );
        // Tampered claims.
        let (_, sig) = t.split_once('.').unwrap();
        let mut c = claims(true, 10);
        c.entitlements.higher_limits = true;
        let forged = format!(
            "{}.{sig}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&c).unwrap())
        );
        assert_eq!(
            verify_token(&forged, &[key(&seed)]),
            Err("error.premium_signature")
        );
        assert!(verify_token("nope", &[key(&seed)]).is_err());
    }

    #[test]
    fn soft_verification() {
        let ok = Cached::default();
        let failed = Cached {
            error: Some("error.premium_unavailable".into()),
            ..Default::default()
        };
        // Valid and fresh: active, no warning.
        let e = effective_from(true, &ok, Some(&claims(true, 100)), 50);
        assert!(e.active && e.warning.is_none() && e.entitlements.white_label);
        // Expired: still active, warning only.
        let e = effective_from(true, &ok, Some(&claims(true, 100)), 200);
        assert!(e.active);
        assert_eq!(e.warning.as_deref(), Some("expired"));
        // Check failed: still active, warning only.
        let e = effective_from(true, &failed, Some(&claims(true, 100)), 50);
        assert!(e.active);
        assert_eq!(e.warning.as_deref(), Some("check_failed"));
        // Server said inactive: off.
        assert!(!effective_from(true, &ok, Some(&claims(false, 100)), 50).active);
        // Never verified: off.
        assert!(!effective_from(true, &failed, None, 50).active);
        // Module off: off.
        assert!(!effective_from(false, &ok, Some(&claims(true, 100)), 50).active);
    }

    /// Token signed by MyMythical's PHP signer (libsodium, test seed 32 x 0x2a), so
    /// both sides agree on the wire format.
    #[test]
    fn verifies_token_from_mymythical() {
        let keys = vec![(
            "it-test".to_string(),
            "GX9rI+FshTLGq8g4+s1ep4m+DHaykgM0A5v6iz02jWE=".to_string(),
        )];
        let tok = "eyJ2IjoxLCJraWQiOiJpdC10ZXN0IiwiaXNzIjoibXl0aGljYWxzeXN0ZW1zIiwicHJvZHVjdCI6InBlbHR5Y21zX3ByZW1pdW0iLCJpbnN0YWxsX2lkIjoiNmYxYzFlN2EtM2M1NS00YzJhLTlkM2UtMWQ4ZjViN2E5YzAxIiwidGVhbV91dWlkIjoiMGI2ZTlmN2UtMTExMS00MjIyLTgzMzMtOTQ0NDU1NTU2NjY2IiwidGVhbV9uYW1lIjoiw5xuw69jb2RlIFRlYW0iLCJhY3RpdmUiOnRydWUsInBsYW4iOiJwcmVtaXVtIiwicmVhc29uIjoiIiwiZW50aXRsZW1lbnRzIjp7IndoaXRlX2xhYmVsIjp0cnVlLCJwcmVtaXVtX2FkZG9ucyI6dHJ1ZSwicHJpb3JpdHlfc3VwcG9ydCI6dHJ1ZSwiaGlnaGVyX2xpbWl0cyI6dHJ1ZX0sInNlYXRzIjoyLCJzZWF0c191c2VkIjoxLCJtYW5hZ2VfdXJsIjoiaHR0cHM6Ly9teS5teXRoaWNhbHN5c3RlbXMub3JnL3RlYW0vcGVsdHljbXMtcHJlbWl1bSIsInN1cHBvcnRfdXJsIjoiaHR0cHM6Ly9teS5teXRoaWNhbHN5c3RlbXMub3JnL3RlYW0vc3VwcG9ydC9jcmVhdGUiLCJpYXQiOjE3OTU2MDAwMDAsImV4cCI6MTc5NjIwNDgwMH0.GVWrvNOBsJjhjVaQl4XDW-jBzrtXqjKPw2Ig1gb4iQ9xZZtsx99_fvuTaBVY-BuHJo_NCSRbH_bOHcqBu9vHAA";
        let cl = verify_token(tok, &keys).expect("valid MyMythical token");
        assert!(cl.active);
        assert_eq!(cl.install_id, "6f1c1e7a-3c55-4c2a-9d3e-1d8f5b7a9c01");
        assert_eq!(cl.team_name, "Ünïcode Team");
        assert_eq!(cl.seats, 2);
        assert!(cl.entitlements.white_label && cl.entitlements.premium_addons);
        let mut bad = tok.to_string();
        bad.replace_range(10..11, if &tok[10..11] == "A" { "B" } else { "A" });
        assert!(verify_token(&bad, &keys).is_err());
    }

    #[test]
    fn official_key_decodes() {
        for (_, k) in OFFICIAL_KEYS {
            assert_eq!(STANDARD.decode(k).unwrap().len(), 32);
        }
    }
}
