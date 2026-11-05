//! Duo Security two-factor (Duo Universal Prompt, OIDC auth-code flow).
//!
//! Password login → `{duo_required, challenge}` → `POST /auth/duo/start`
//! returns the Duo authorize URL (signed HS512 request JWT) → Duo redirects
//! the browser to `{public_url}/auth/duo/callback?state&duo_code` → the SPA
//! posts them to `/auth/duo/finish`, which swaps the code at Duo's token
//! endpoint (client-assertion JWT) and validates the HS512 `id_token`.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::routes::auth::{session_response, AuthGuard};
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use pelty_auth::token;
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, mfa, settings, users};
use pelty_payments::HttpRequestSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha512;
use uuid::Uuid;

const KEY: &str = "duo";
const STATE_TTL_SECS: i64 = 600;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuoConfig {
    pub enabled: bool,
    pub client_id: String,
    pub api_host: String,
    /// `staff` or `all`.
    pub scope: String,
    /// Sealed client secret (base64), never returned.
    #[serde(default)]
    pub secret_enc: String,
}

pub async fn config(s: &AppState) -> AppResult<DuoConfig> {
    Ok(settings::get(&s.pool, KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

fn secret(s: &AppState, c: &DuoConfig) -> AppResult<String> {
    let sb = s.secret_box.as_ref().ok_or(AppError::BadRequest("error.mfa_unavailable"))?;
    let raw = STANDARD.decode(&c.secret_enc).map_err(|_| AppError::Conflict("error.duo_not_configured"))?;
    let plain = sb.open(&raw).ok_or(AppError::Conflict("error.duo_not_configured"))?;
    String::from_utf8(plain).map_err(|_| AppError::Conflict("error.duo_not_configured"))
}

pub fn host_ok(h: &str) -> bool {
    h.len() <= 64 && h.starts_with("api-") && h.ends_with(".duosecurity.com")
        && h["api-".len()..h.len() - ".duosecurity.com".len()].chars().all(|c| c.is_ascii_alphanumeric())
        && h.len() > "api-.duosecurity.com".len()
}

/// Does Duo apply to this user?
pub async fn applies(s: &AppState, user: Uuid) -> AppResult<bool> {
    let c = config(s).await?;
    if !c.enabled || c.secret_enc.is_empty() {
        return Ok(false);
    }
    Ok(c.scope == "all" || users::roles(&s.pool, user).await?.iter().any(|r| r != "customer"))
}

// JWT (HS512)

pub fn jwt_sign(claims: &Value, secret: &str) -> String {
    let head = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS512","typ":"JWT"}"#);
    let body = URL_SAFE_NO_PAD.encode(claims.to_string());
    let input = format!("{head}.{body}");
    let mut mac = Hmac::<Sha512>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

/// Verify signature + `alg`; returns the claims.
pub fn jwt_verify(jwt: &str, secret: &str) -> Option<Value> {
    let mut parts = jwt.split('.');
    let (h, b, sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let head: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(h).ok()?).ok()?;
    if head["alg"] != "HS512" {
        return None;
    }
    let mut mac = Hmac::<Sha512>::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(format!("{h}.{b}").as_bytes());
    mac.verify_slice(&URL_SAFE_NO_PAD.decode(sig).ok()?).ok()?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(b).ok()?).ok()
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn redirect_uri(s: &AppState) -> String {
    format!("{}/auth/duo/callback", s.cfg.public_url.trim_end_matches('/'))
}

fn form(pairs: &[(&str, &str)]) -> String {
    form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs.iter()).finish()
}

fn assertion(c: &DuoConfig, secret: &str, aud: &str) -> String {
    jwt_sign(&json!({ "iss": c.client_id, "sub": c.client_id, "aud": aud, "jti": token::generate(), "iat": now(), "exp": now() + 300 }), secret)
}

async fn post_form(s: &AppState, url: String, body: String) -> AppResult<Value> {
    let (status, text) = s.http().send(HttpRequestSpec { method: "POST", url, headers: vec![("content-type".into(), "application/x-www-form-urlencoded".into())], body })
        .await.map_err(|_| AppError::Conflict("error.duo_unavailable"))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        tracing::warn!(status, "duo request failed");
        return Err(AppError::Conflict("error.duo_unavailable"));
    }
    Ok(v)
}

/// Duo `/oauth/v1/health_check`.
pub async fn health_check(s: &AppState, c: &DuoConfig, secret: &str) -> AppResult<()> {
    let url = format!("https://{}/oauth/v1/health_check", c.api_host);
    let body = form(&[("client_id", &c.client_id), ("client_assertion", &assertion(c, secret, &url))]);
    let v = post_form(s, url, body).await?;
    if v["stat"] == "OK" { Ok(()) } else { Err(AppError::Conflict("error.duo_unavailable")) }
}

// login routes

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/duo/start", post(start))
        .route("/auth/duo/finish", post(finish))
        .route("/admin/security/duo", get(admin_get).put(admin_put))
        .route("/admin/security/duo/test", post(admin_test))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StartBody {
    pub challenge: String,
}

/// Swap the password-step challenge for a Duo authorize URL. Health-checks
/// Duo first; if Duo is down the login fails closed.
#[utoipa::path(post, path = "/api/v1/auth/duo/start", request_body = StartBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn start(State(s): State<AppState>, g: AuthGuard, Json(b): Json<StartBody>) -> ApiResult<Json<Value>> {
    let hash = token::hash(&b.challenge);
    let Some(user_id) = mfa::attempt_challenge(&s.pool, &hash).await? else {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    };
    if !applies(&s, user_id).await? {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    }
    let c = config(&s).await?;
    let sec = secret(&s, &c)?;
    health_check(&s, &c, &sec).await?;
    if !mfa::finish_challenge(&s.pool, &hash).await? {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    }
    let user = users::find(&s.pool, user_id).await?.filter(|u| u.is_active).ok_or(AppError::InvalidCredentials)?;
    let _ = sqlx::query("DELETE FROM duo_states WHERE expires_at < now() - interval '1 day'").execute(&s.pool).await;
    let state = token::generate();
    let nonce = token::generate();
    sqlx::query("INSERT INTO duo_states (state_hash, user_id, nonce, expires_at) VALUES ($1,$2,$3, now() + make_interval(secs => $4))")
        .bind(token::hash(&state).as_slice()).bind(user_id).bind(&nonce).bind(STATE_TTL_SECS as f64)
        .execute(&s.pool).await.map_err(pelty_database::db_err)?;
    let aud = format!("https://{}", c.api_host);
    let req = jwt_sign(&json!({
        "response_type": "code", "scope": "openid", "exp": now() + 300, "client_id": c.client_id,
        "redirect_uri": redirect_uri(&s), "state": state, "duo_uname": user.email, "iss": c.client_id, "aud": aud,
        "nonce": nonce, "use_duo_code_attribute": true,
    }), &sec);
    let url = format!("https://{}/oauth/v1/authorize?{}", c.api_host, form(&[("response_type", "code"), ("client_id", &c.client_id), ("request", &req)]));
    audit::record(&s.pool, Some(user_id), "auth.duo_started", None, g.ip(), json!({})).await?;
    Ok(Json(json!({ "url": url })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FinishBody {
    pub state: String,
    pub duo_code: String,
}

/// Validate a Duo `id_token` for this user; Ok(()) only on `allow`.
pub fn check_id_token(claims: &Value, c: &DuoConfig, uname: &str, nonce: &str, now: i64) -> Result<(), &'static str> {
    let iss = format!("https://{}/oauth/v1/token", c.api_host);
    let aud_ok = claims["aud"] == c.client_id.as_str() || claims["aud"].as_array().is_some_and(|a| a.iter().any(|x| x == c.client_id.as_str()));
    if claims["iss"] != iss.as_str() || !aud_ok { return Err("issuer"); }
    if claims["exp"].as_i64().is_none_or(|e| e + 60 < now) { return Err("expired"); }
    if claims["iat"].as_i64().is_some_and(|i| i > now + 60) { return Err("future"); }
    if !claims["preferred_username"].as_str().is_some_and(|u| u.eq_ignore_ascii_case(uname)) { return Err("user"); }
    if claims["nonce"] != nonce { return Err("nonce"); }
    if claims["auth_result"]["result"] != "allow" { return Err("denied"); }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/auth/duo/finish", request_body = FinishBody, responses((status = 200), (status = 401), (status = 422)))]
pub async fn finish(State(s): State<AppState>, g: AuthGuard, headers: HeaderMap, Json(b): Json<FinishBody>) -> ApiResult<Response> {
    // Single use: claim the state before talking to Duo.
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "UPDATE duo_states SET used_at = now() WHERE state_hash = $1 AND used_at IS NULL AND expires_at > now() RETURNING user_id, nonce",
    ).bind(token::hash(&b.state).as_slice()).fetch_optional(&s.pool).await.map_err(pelty_database::db_err)?;
    let Some((user_id, nonce)) = row else {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    };
    if b.duo_code.is_empty() || b.duo_code.len() > 512 {
        return Err(AppError::Validation("error.duo_denied").into());
    }
    let c = config(&s).await?;
    let sec = secret(&s, &c)?;
    let user = users::find(&s.pool, user_id).await?.filter(|u| u.is_active).ok_or(AppError::InvalidCredentials)?;
    let url = format!("https://{}/oauth/v1/token", c.api_host);
    let body = form(&[
        ("grant_type", "authorization_code"), ("code", &b.duo_code), ("redirect_uri", &redirect_uri(&s)),
        ("client_assertion_type", "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"), ("client_assertion", &assertion(&c, &sec, &url)),
    ]);
    let v = post_form(&s, url, body).await?;
    let verdict = v["id_token"].as_str().and_then(|t| jwt_verify(t, &sec)).ok_or("signature")
        .and_then(|claims| check_id_token(&claims, &c, &user.email, &nonce, now()));
    if let Err(why) = verdict {
        audit::record(&s.pool, Some(user_id), "auth.duo_failed", None, g.ip(), json!({ "reason": why })).await?;
        return Err(AppError::Validation("error.duo_denied").into());
    }
    audit::record(&s.pool, Some(user_id), "auth.mfa_verified", None, g.ip(), json!({ "method": "duo" })).await?;
    session_response(&s, &g, &headers, user).await
}

// admin

fn public(c: &DuoConfig) -> Value {
    json!({ "enabled": c.enabled, "client_id": c.client_id, "api_host": c.api_host, "scope": if c.scope.is_empty() { "staff" } else { &c.scope }, "has_secret": !c.secret_enc.is_empty() })
}

#[utoipa::path(get, path = "/api/v1/admin/security/duo", responses((status = 200)))]
pub async fn admin_get(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(public(&config(&s).await?)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DuoBody {
    pub enabled: bool,
    pub client_id: String,
    pub api_host: String,
    pub scope: String,
    /// Empty keeps the stored secret.
    #[serde(default)]
    pub client_secret: String,
}

#[utoipa::path(put, path = "/api/v1/admin/security/duo", request_body = DuoBody, responses((status = 200), (status = 422)))]
pub async fn admin_put(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<DuoBody>) -> ApiResult<Json<Value>> {
    let mut c = config(&s).await?;
    let cid = b.client_id.trim();
    let host = b.api_host.trim().to_ascii_lowercase();
    if !(cid.is_empty() && !b.enabled) && (cid.len() != 20 || !cid.chars().all(|x| x.is_ascii_alphanumeric())) {
        return Err(AppError::Validation("error.duo_client_id").into());
    }
    if !(host.is_empty() && !b.enabled) && !host_ok(&host) {
        return Err(AppError::Validation("error.duo_api_host").into());
    }
    if !matches!(b.scope.as_str(), "staff" | "all") {
        return Err(AppError::Validation("error.duo_scope").into());
    }
    if !b.client_secret.is_empty() {
        if b.client_secret.len() != 40 || !b.client_secret.chars().all(|x| x.is_ascii_alphanumeric()) {
            return Err(AppError::Validation("error.duo_client_secret").into());
        }
        let sb = s.secret_box.as_ref().ok_or(AppError::BadRequest("error.mfa_unavailable"))?;
        c.secret_enc = STANDARD.encode(sb.seal(b.client_secret.as_bytes()));
    }
    if b.enabled && c.secret_enc.is_empty() {
        return Err(AppError::Validation("error.duo_client_secret").into());
    }
    c.enabled = b.enabled;
    c.client_id = cid.to_string();
    c.api_host = host;
    c.scope = b.scope;
    settings::set(&s.pool, KEY, serde_json::to_value(&c).map_err(|e| AppError::Internal(e.to_string()))?).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.duo", None, u.ip.as_deref(), json!({ "enabled": c.enabled, "scope": c.scope, "secret_changed": !b.client_secret.is_empty() })).await?;
    Ok(Json(public(&c)))
}

#[utoipa::path(post, path = "/api/v1/admin/security/duo/test", responses((status = 200), (status = 409)))]
pub async fn admin_test(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    let c = config(&s).await?;
    let sec = secret(&s, &c)?;
    health_check(&s, &c, &sec).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jwt_roundtrip_and_tamper() {
        let t = jwt_sign(&json!({"a": 1}), "s3cret");
        assert_eq!(jwt_verify(&t, "s3cret").unwrap()["a"], 1);
        assert!(jwt_verify(&t, "other").is_none());
        let mut parts: Vec<&str> = t.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(r#"{"a":2}"#);
        parts[1] = &forged;
        assert!(jwt_verify(&parts.join("."), "s3cret").is_none());
        let none_alg = format!("{}.{}.", URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#), URL_SAFE_NO_PAD.encode(r#"{"a":1}"#));
        assert!(jwt_verify(&none_alg, "s3cret").is_none());
    }
    #[test]
    fn hosts() {
        assert!(host_ok("api-1a2b3c4d.duosecurity.com"));
        assert!(!host_ok("api-.duosecurity.com"));
        assert!(!host_ok("evil.com"));
        assert!(!host_ok("api-x.duosecurity.com.evil.com"));
        assert!(!host_ok("api-a/b.duosecurity.com"));
    }
    #[test]
    fn id_token_rules() {
        let c = DuoConfig { enabled: true, client_id: "DIXXXXXXXXXXXXXXXXXX".into(), api_host: "api-abc.duosecurity.com".into(), scope: "staff".into(), secret_enc: String::new() };
        let good = json!({"iss": "https://api-abc.duosecurity.com/oauth/v1/token", "aud": c.client_id, "exp": 1000, "iat": 900, "preferred_username": "Ana@x.test", "nonce": "n", "auth_result": {"result": "allow"}});
        assert_eq!(check_id_token(&good, &c, "ana@x.test", "n", 950), Ok(()));
        let mut b = good.clone(); b["auth_result"]["result"] = json!("deny");
        assert_eq!(check_id_token(&b, &c, "ana@x.test", "n", 950), Err("denied"));
        assert_eq!(check_id_token(&good, &c, "eve@x.test", "n", 950), Err("user"));
        assert_eq!(check_id_token(&good, &c, "ana@x.test", "m", 950), Err("nonce"));
        assert_eq!(check_id_token(&good, &c, "ana@x.test", "n", 2000), Err("expired"));
        let mut b = good.clone(); b["iss"] = json!("https://evil/oauth/v1/token");
        assert_eq!(check_id_token(&b, &c, "ana@x.test", "n", 950), Err("issuer"));
    }
}
