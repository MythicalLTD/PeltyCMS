//! PeltyCMS as an OAuth 2.0 / OpenID Connect provider ("Sign in with Pelty").
//!
//! * Authorization code flow only, PKCE (`S256`) mandatory for every client
//!   (OAuth 2.1); implicit and password grants are not offered.
//! * The authorize step is a SPA consent screen at `/oauth/authorize` that
//!   talks to `GET|POST /api/v1/oauth/authorize` with the user's session.
//! * `POST /oauth/token` (authorization_code, refresh_token with rotation),
//!   `GET|POST /oauth/userinfo`, `POST /oauth/revoke`, `GET /oauth/jwks`,
//!   `GET /.well-known/openid-configuration`.
//! * ID tokens are ES256 JWTs; the P-256 key is generated on first use and
//!   stored PKCS#8 (sealed with the secret box when an encryption key is set).
//! * Codes, access and refresh tokens are random and stored only as SHA-256.
//!   A replayed code revokes every token minted from it (RFC 6749 §4.1.2).
use crate::error::ApiResult;
use crate::extract::{perms::SettingsManage, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Form, Json, Router};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use pelty_core::AppError;
use pelty_database::{audit, users};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const SCOPES: &[&str] = &["openid", "profile", "email", "offline_access"];
const CODE_TTL: i64 = 60;
const ACCESS_TTL: i64 = 3600;
const REFRESH_TTL: i64 = 30 * 86400;
const ACCESS_PREFIX: &str = "pat_";
const REFRESH_PREFIX: &str = "prt_";

pub fn root_router() -> Router<AppState> {
    Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/oauth/jwks", get(jwks))
        .route("/oauth/token", post(token))
        .route("/oauth/userinfo", get(userinfo).post(userinfo))
        .route("/oauth/revoke", post(revoke))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth/authorize", get(authorize_info).post(authorize_decide))
        .route("/admin/oauth-clients", get(admin_list).post(admin_create))
        .route("/admin/oauth-clients/{id}", patch(admin_update).delete(admin_delete))
        .route("/admin/oauth-clients/{id}/secret", post(admin_rotate))
        .route("/account/oauth-apps", get(my_apps))
        .route("/account/oauth-apps/{id}", axum::routing::delete(my_revoke))
}

fn sha(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn issuer(s: &AppState) -> String {
    s.cfg.public_url.trim_end_matches('/').to_string()
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

// keys / JWT

struct SigningKey {
    kid: String,
    pair: EcdsaKeyPair,
}

async fn signing_key(s: &AppState) -> ApiResult<SigningKey> {
    let rng = SystemRandom::new();
    let row = oauth_provider::active_key(&s.pool).await?;
    let (kid, stored) = match row {
        Some(r) => r,
        None => {
            let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                .map_err(|_| AppError::Internal("oidc keygen".into()))?;
            let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, doc.as_ref(), &rng)
                .map_err(|_| AppError::Internal("oidc key".into()))?;
            let (x, y) = xy(&pair);
            let kid = sha(&format!("{x}.{y}"))[..16].to_string();
            let stored = match &s.secret_box {
                Some(b) => b.seal(doc.as_ref()),
                None => doc.as_ref().to_vec(),
            };
            // Two concurrent first requests: the loser's key is simply ignored.
            oauth_provider::insert_key(&s.pool, &kid, &stored, &x, &y).await?;
            return Ok(SigningKey { kid, pair });
        }
    };
    let plain = match &s.secret_box {
        Some(b) => b.open(&stored).unwrap_or(stored),
        None => stored,
    };
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &plain, &rng)
        .map_err(|_| AppError::Internal("oidc key unreadable".into()))?;
    Ok(SigningKey { kid, pair })
}

/// Uncompressed P-256 point `04 || X || Y` → base64url X, Y.
fn xy(pair: &EcdsaKeyPair) -> (String, String) {
    let p = pair.public_key().as_ref();
    (URL_SAFE_NO_PAD.encode(&p[1..33]), URL_SAFE_NO_PAD.encode(&p[33..65]))
}

fn jwt(key: &SigningKey, claims: &Value) -> ApiResult<String> {
    let header = json!({ "alg": "ES256", "typ": "JWT", "kid": key.kid });
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let sig = key
        .pair
        .sign(&SystemRandom::new(), input.as_bytes())
        .map_err(|_| AppError::Internal("oidc sign".into()))?;
    Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())))
}

// discovery

pub async fn discovery(State(s): State<AppState>) -> Json<Value> {
    let i = issuer(&s);
    Json(json!({
        "issuer": i,
        "authorization_endpoint": format!("{i}/oauth/authorize"),
        "token_endpoint": format!("{i}/oauth/token"),
        "userinfo_endpoint": format!("{i}/oauth/userinfo"),
        "jwks_uri": format!("{i}/oauth/jwks"),
        "revocation_endpoint": format!("{i}/oauth/revoke"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
        "scopes_supported": SCOPES,
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "none"],
        "code_challenge_methods_supported": ["S256"],
        "claims_supported": ["sub", "iss", "aud", "exp", "iat", "auth_time", "nonce", "email", "email_verified", "name", "locale"],
    }))
}

pub async fn jwks(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    signing_key(&s).await?; // ensure one exists
    let rows = oauth_provider::public_keys(&s.pool).await?;
    let keys: Vec<Value> = rows
        .into_iter()
        .map(|(kid, x, y)| json!({ "kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": kid, "x": x, "y": y }))
        .collect();
    Ok(Json(json!({ "keys": keys })))
}

// authorize

#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    pub response_type: Option<String>,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scope: String,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    /// `none` = never show UI (error `consent_required` / `login_required`).
    pub prompt: Option<String>,
}

use pelty_database::oauth_provider::{self, Client as ClientRow};

async fn client_by_public_id(s: &AppState, cid: &str) -> ApiResult<Option<ClientRow>> {
    Ok(oauth_provider::client_by_public_id(&s.pool, cid).await?)
}

/// Validated request: errors before the redirect URI is trusted are returned
/// to the user (never redirected), afterwards they go back to the client.
struct Validated {
    client: ClientRow,
    scopes: Vec<String>,
}

fn redirect_error(uri: &str, err: &str, state: Option<&str>) -> String {
    let mut q = form_urlencoded::Serializer::new(String::new());
    q.append_pair("error", err);
    if let Some(st) = state {
        q.append_pair("state", st);
    }
    let sep = if uri.contains('?') { '&' } else { '?' };
    format!("{uri}{sep}{}", q.finish())
}

async fn validate(s: &AppState, p: &AuthorizeParams) -> ApiResult<Result<Validated, String>> {
    let client = client_by_public_id(s, &p.client_id)
        .await?
        .filter(|c| c.enabled)
        .ok_or(AppError::Validation("error.oauth_client_unknown"))?;
    if !client.redirect_uris.iter().any(|u| u == &p.redirect_uri) {
        return Err(AppError::Validation("error.oauth_redirect_mismatch").into());
    }
    let st = p.state.as_deref();
    if p.response_type.as_deref().unwrap_or("code") != "code" {
        return Ok(Err(redirect_error(&p.redirect_uri, "unsupported_response_type", st)));
    }
    let chal_ok = p.code_challenge.as_deref().is_some_and(|c| (43..=128).contains(&c.len()));
    if !chal_ok || p.code_challenge_method.as_deref() != Some("S256") {
        return Ok(Err(redirect_error(&p.redirect_uri, "invalid_request", st)));
    }
    let mut scopes: Vec<String> = Vec::new();
    for sc in p.scope.split_whitespace() {
        if !SCOPES.contains(&sc) || !client.scopes.iter().any(|a| a == sc) {
            return Ok(Err(redirect_error(&p.redirect_uri, "invalid_scope", st)));
        }
        if !scopes.iter().any(|x| x == sc) {
            scopes.push(sc.to_string());
        }
    }
    if !scopes.iter().any(|x| x == "openid") && scopes.is_empty() {
        return Ok(Err(redirect_error(&p.redirect_uri, "invalid_scope", st)));
    }
    Ok(Ok(Validated { client, scopes }))
}

async fn has_consent(s: &AppState, user: Uuid, client: Uuid, scopes: &[String]) -> ApiResult<bool> {
    let granted = oauth_provider::consent_scopes(&s.pool, user, client).await?;
    Ok(granted.is_some_and(|g| scopes.iter().all(|x| g.contains(x))))
}

/// The consent screen asks this first. When the user already consented (or
/// the client is trusted) the SPA posts `approve` right away.
pub async fn authorize_info(State(s): State<AppState>, u: CurrentUser, Query(p): Query<AuthorizeParams>) -> ApiResult<Json<Value>> {
    let v = match validate(&s, &p).await? {
        Ok(v) => v,
        Err(redirect) => return Ok(Json(json!({ "redirect_to": redirect }))),
    };
    let consented = v.client.trusted || has_consent(&s, u.user_id, v.client.id, &v.scopes).await?;
    if p.prompt.as_deref() == Some("none") && !consented {
        return Ok(Json(json!({ "redirect_to": redirect_error(&p.redirect_uri, "consent_required", p.state.as_deref()) })));
    }
    let me = users::find(&s.pool, u.user_id).await?.ok_or(AppError::Unauthenticated)?;
    Ok(Json(json!({
        "client": { "name": v.client.name, "client_id": v.client.client_id, "trusted": v.client.trusted },
        "redirect_host": p.redirect_uri.split('/').nth(2).unwrap_or(""),
        "scopes": v.scopes,
        "consent_required": !consented,
        "user": { "email": me.email, "name": me.display_name },
    })))
}

#[derive(Debug, Deserialize)]
pub struct Decide {
    #[serde(flatten)]
    pub params: AuthorizeParams,
    pub approve: bool,
}

pub async fn authorize_decide(State(s): State<AppState>, u: CurrentUser, Json(b): Json<Decide>) -> ApiResult<Json<Value>> {
    if u.api_key {
        return Err(AppError::Forbidden.into());
    }
    let p = &b.params;
    let v = match validate(&s, p).await? {
        Ok(v) => v,
        Err(redirect) => return Ok(Json(json!({ "redirect_to": redirect }))),
    };
    if !b.approve {
        audit::record(&s.pool, Some(u.user_id), "oauth.consent_denied", Some(&v.client.client_id), u.ip.as_deref(), json!({})).await?;
        return Ok(Json(json!({ "redirect_to": redirect_error(&p.redirect_uri, "access_denied", p.state.as_deref()) })));
    }
    if !v.client.trusted {
        oauth_provider::grant_consent(&s.pool, u.user_id, v.client.id, &v.scopes).await?;
    }
    let code = pelty_auth::token::generate();
    let scope = v.scopes.join(" ");
    oauth_provider::insert_code(&s.pool, &oauth_provider::NewCode {
        code_hash: &sha(&code), client: v.client.id, user: u.user_id, redirect_uri: &p.redirect_uri, scope: &scope,
        nonce: p.nonce.as_deref(), code_challenge: p.code_challenge.as_deref(), expires_at: now() + Duration::seconds(CODE_TTL),
    }).await?;
    audit::record(&s.pool, Some(u.user_id), "oauth.authorized", Some(&v.client.client_id), u.ip.as_deref(), json!({ "scopes": v.scopes })).await?;
    let mut q = form_urlencoded::Serializer::new(String::new());
    q.append_pair("code", &code);
    if let Some(st) = &p.state {
        q.append_pair("state", st);
    }
    q.append_pair("iss", &issuer(&s));
    let sep = if p.redirect_uri.contains('?') { '&' } else { '?' };
    Ok(Json(json!({ "redirect_to": format!("{}{sep}{}", p.redirect_uri, q.finish()) })))
}

// token

/// RFC 6749 §5.2 error body; never cached.
fn oauth_err(status: StatusCode, code: &str) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(json!({ "error": code }))).into_response()
}

#[derive(Debug, Deserialize)]
pub struct TokenForm {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scope: Option<String>,
}

/// Client authentication: HTTP Basic or form fields; public clients send
/// only `client_id` (they are bound by PKCE instead).
async fn authenticate_client(s: &AppState, headers: &HeaderMap, f: &TokenForm) -> Result<ClientRow, (StatusCode, &'static str)> {
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| STANDARD.decode(b.trim()).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| {
            let (a, b) = s.split_once(':')?;
            let dec = |x: &str| form_urlencoded::parse(format!("v={x}").as_bytes()).next().map(|(_, v)| v.into_owned());
            Some((dec(a)?, dec(b)?))
        });
    let (cid, secret) = match basic {
        Some((c, sec)) => (c, Some(sec)),
        None => match &f.client_id {
            Some(c) => (c.clone(), f.client_secret.clone()),
            None => return Err((StatusCode::UNAUTHORIZED, "invalid_client")),
        },
    };
    let client = match client_by_public_id(s, &cid).await {
        Ok(Some(c)) if c.enabled => c,
        Ok(_) => return Err((StatusCode::UNAUTHORIZED, "invalid_client")),
        Err(_) => return Err((StatusCode::INTERNAL_SERVER_ERROR, "server_error")),
    };
    match (&client.secret_hash, secret) {
        (None, None) => Ok(client),
        (Some(h), Some(sec)) if pelty_auth::token::constant_eq(h, &sha(&sec)) => Ok(client),
        _ => Err((StatusCode::UNAUTHORIZED, "invalid_client")),
    }
}

async fn issue(s: &AppState, client: &ClientRow, user: Uuid, scope: &str, nonce: Option<String>, code_hash: Option<&str>, with_id_token: bool) -> ApiResult<Value> {
    let access = format!("{ACCESS_PREFIX}{}", pelty_auth::token::generate());
    oauth_provider::insert_token(&s.pool, &oauth_provider::NewToken {
        token_hash: &sha(&access), kind: "access", client: client.id, user, scope, code_hash, expires_at: now() + Duration::seconds(ACCESS_TTL),
    }).await?;
    let mut out = json!({ "access_token": access, "token_type": "Bearer", "expires_in": ACCESS_TTL, "scope": scope });
    let scopes: Vec<&str> = scope.split_whitespace().collect();
    if scopes.contains(&"offline_access") {
        let refresh = format!("{REFRESH_PREFIX}{}", pelty_auth::token::generate());
        oauth_provider::insert_token(&s.pool, &oauth_provider::NewToken {
            token_hash: &sha(&refresh), kind: "refresh", client: client.id, user, scope, code_hash, expires_at: now() + Duration::seconds(REFRESH_TTL),
        }).await?;
        out["refresh_token"] = json!(refresh);
    }
    if with_id_token && scopes.contains(&"openid") {
        let me = users::find(&s.pool, user).await?.ok_or(AppError::NotFound)?;
        let t = now().unix_timestamp();
        let mut c = json!({ "iss": issuer(s), "sub": user.to_string(), "aud": client.client_id, "iat": t, "exp": t + ACCESS_TTL, "auth_time": t });
        if let Some(n) = nonce {
            c["nonce"] = json!(n);
        }
        // at_hash: left half of SHA-256(access_token), base64url (OIDC core §3.1.3.6)
        let h = Sha256::digest(access.as_bytes());
        c["at_hash"] = json!(URL_SAFE_NO_PAD.encode(&h[..16]));
        claims(&mut c, &me, &scopes);
        out["id_token"] = json!(jwt(&signing_key(s).await?, &c)?);
    }
    Ok(out)
}

fn claims(c: &mut Value, u: &users::User, scopes: &[&str]) {
    if scopes.contains(&"email") {
        c["email"] = json!(u.email);
        c["email_verified"] = json!(u.email_verified_at.is_some());
    }
    if scopes.contains(&"profile") {
        c["name"] = json!(u.display_name);
        if let Some(l) = &u.locale {
            c["locale"] = json!(l);
        }
    }
}

fn ok_json(v: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store"), (header::PRAGMA, "no-cache")], Json(v)).into_response()
}

pub async fn token(State(s): State<AppState>, headers: HeaderMap, Form(f): Form<TokenForm>) -> Response {
    match token_inner(&s, &headers, f).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = ?e.0, "oauth token endpoint failed");
            oauth_err(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
        }
    }
}

async fn token_inner(s: &AppState, headers: &HeaderMap, f: TokenForm) -> ApiResult<Response> {
    let client = match authenticate_client(s, headers, &f).await {
        Ok(c) => c,
        Err((st, e)) => return Ok(oauth_err(st, e)),
    };
    match f.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(redirect), Some(verifier)) = (&f.code, &f.redirect_uri, &f.code_verifier) else {
                return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_request"));
            };
            let ch = sha(code);
            let c = match oauth_provider::redeem_code(&s.pool, &ch).await? {
                oauth_provider::Redeem::Unknown => return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_grant")),
                oauth_provider::Redeem::Replayed { user } => {
                    audit::record(&s.pool, Some(user), "oauth.code_replay", Some(&client.client_id), None, json!({})).await?;
                    return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_grant"));
                }
                oauth_provider::Redeem::Fresh(c) => c,
            };
            let pkce = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            let pkce_ok = c.code_challenge.as_deref().is_some_and(|c| pelty_auth::token::constant_eq(c, &pkce));
            if c.client_id != client.id || &c.redirect_uri != redirect || c.expires_at < now() || !pkce_ok {
                return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_grant"));
            }
            Ok(ok_json(issue(s, &client, c.user_id, &c.scope, c.nonce, Some(&ch), true).await?))
        }
        "refresh_token" => {
            let Some(rt) = &f.refresh_token else {
                return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_request"));
            };
            let h = sha(rt);
            let row = oauth_provider::take_refresh(&s.pool, &h, client.id).await?;
            let Some((uid, scope, code_hash)) = row else {
                return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_grant"));
            };
            let scope = match &f.scope {
                Some(req) => {
                    let had: Vec<&str> = scope.split_whitespace().collect();
                    if !req.split_whitespace().all(|x| had.contains(&x)) {
                        return Ok(oauth_err(StatusCode::BAD_REQUEST, "invalid_scope"));
                    }
                    req.clone()
                }
                None => scope,
            };
            Ok(ok_json(issue(s, &client, uid, &scope, None, code_hash.as_deref(), true).await?))
        }
        _ => Ok(oauth_err(StatusCode::BAD_REQUEST, "unsupported_grant_type")),
    }
}

// userinfo / revoke

pub async fn userinfo(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| t.starts_with(ACCESS_PREFIX));
    let unauth = || {
        (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer error=\"invalid_token\"")], Json(json!({ "error": "invalid_token" }))).into_response()
    };
    let Some(t) = bearer else { return Ok(unauth()) };
    let row = oauth_provider::access_token(&s.pool, &sha(t)).await?;
    let Some((uid, scope)) = row else { return Ok(unauth()) };
    let scopes: Vec<&str> = scope.split_whitespace().collect();
    if !scopes.contains(&"openid") {
        return Ok((StatusCode::FORBIDDEN, Json(json!({ "error": "insufficient_scope" }))).into_response());
    }
    let Some(me) = users::find(&s.pool, uid).await?.filter(|u| u.is_active) else { return Ok(unauth()) };
    let mut c = json!({ "sub": uid.to_string() });
    claims(&mut c, &me, &scopes);
    Ok(ok_json(c))
}

#[derive(Debug, Deserialize)]
pub struct RevokeForm {
    pub token: String,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// RFC 7009: always 200, even for unknown tokens.
pub async fn revoke(State(s): State<AppState>, headers: HeaderMap, Form(f): Form<RevokeForm>) -> Response {
    let tf = TokenForm { grant_type: String::new(), code: None, redirect_uri: None, code_verifier: None, refresh_token: None, client_id: f.client_id, client_secret: f.client_secret, scope: None };
    let client = match authenticate_client(&s, &headers, &tf).await {
        Ok(c) => c,
        Err((st, e)) => return oauth_err(st, e),
    };
    let r = oauth_provider::revoke_token(&s.pool, &sha(&f.token), client.id).await;
    match r {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => oauth_err(StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
    }
}

// admin

#[derive(Debug, Deserialize)]
pub struct ClientBody {
    pub name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
    pub scopes: Option<Vec<String>>,
    pub confidential: Option<bool>,
    pub trusted: Option<bool>,
    pub enabled: Option<bool>,
}

fn check_uris(uris: &[String]) -> ApiResult<()> {
    if uris.is_empty() || uris.len() > 20 {
        return Err(AppError::Validation("error.oauth_redirect_invalid").into());
    }
    for u in uris {
        let https = u.starts_with("https://");
        let loopback = u.starts_with("http://localhost") || u.starts_with("http://127.0.0.1") || u.starts_with("http://[::1]");
        // custom schemes for native apps (RFC 8252 §7.1): `com.example.app:/cb`
        let native = u.split_once(':').is_some_and(|(sch, _)| sch.contains('.') && sch.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
        if u.len() > 500 || u.contains('#') || u.contains(char::is_whitespace) || !(https || loopback || native) {
            return Err(AppError::Validation("error.oauth_redirect_invalid").into());
        }
    }
    Ok(())
}

fn check_scopes(sc: &[String]) -> ApiResult<()> {
    if sc.is_empty() || sc.iter().any(|x| !SCOPES.contains(&x.as_str())) {
        return Err(AppError::Validation("error.oauth_scope_invalid").into());
    }
    Ok(())
}

fn client_json(c: &ClientRow) -> Value {
    json!({ "id": c.id, "client_id": c.client_id, "name": c.name, "redirect_uris": c.redirect_uris, "scopes": c.scopes, "confidential": c.secret_hash.is_some(), "trusted": c.trusted, "enabled": c.enabled })
}

pub async fn admin_list(State(s): State<AppState>, _p: RequirePermission<SettingsManage>) -> ApiResult<Json<Value>> {
    let rows = oauth_provider::list_clients(&s.pool).await?;
    Ok(Json(json!({ "clients": rows.iter().map(client_json).collect::<Vec<_>>(), "scopes": SCOPES, "issuer": issuer(&s) })))
}

pub async fn admin_create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<SettingsManage>, Json(b): Json<ClientBody>) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = b.name.as_deref().map(str::trim).filter(|n| !n.is_empty() && n.len() <= 120).ok_or(AppError::Validation("error.name_required"))?;
    let uris = b.redirect_uris.unwrap_or_default();
    check_uris(&uris)?;
    let scopes = b.scopes.unwrap_or_else(|| vec!["openid".into(), "profile".into(), "email".into()]);
    check_scopes(&scopes)?;
    let id = Uuid::new_v4();
    let client_id = format!("pc_{}", &pelty_auth::token::generate()[..24]);
    let secret = b.confidential.unwrap_or(true).then(|| format!("pcs_{}", pelty_auth::token::generate()));
    let secret_hash = secret.as_deref().map(sha);
    oauth_provider::create_client(&s.pool, &oauth_provider::NewClient {
        id, client_id: &client_id, secret_hash: secret_hash.as_deref(), name, redirect_uris: &uris, scopes: &scopes, trusted: b.trusted.unwrap_or(false),
    }).await?;
    audit::record(&s.pool, Some(u.user_id), "oauth.client_created", Some(&client_id), u.ip.as_deref(), json!({ "name": name })).await?;
    // The secret is shown exactly once.
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "client_id": client_id, "client_secret": secret }))))
}

pub async fn admin_update(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<SettingsManage>, Path(id): Path<Uuid>, Json(b): Json<ClientBody>) -> ApiResult<Json<Value>> {
    if let Some(x) = &b.redirect_uris {
        check_uris(x)?;
    }
    if let Some(x) = &b.scopes {
        check_scopes(x)?;
    }
    let name = b.name.as_deref().map(str::trim);
    if name.is_some_and(|n| n.is_empty() || n.len() > 120) {
        return Err(AppError::Validation("error.name_required").into());
    }
    let patch = oauth_provider::ClientPatch {
        name, redirect_uris: b.redirect_uris.as_deref(), scopes: b.scopes.as_deref(), trusted: b.trusted, enabled: b.enabled,
    };
    let row = oauth_provider::update_client(&s.pool, id, &patch).await?;
    let c = row.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "oauth.client_updated", Some(&c.client_id), u.ip.as_deref(), json!({})).await?;
    Ok(Json(client_json(&c)))
}

pub async fn admin_delete(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<SettingsManage>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let cid = oauth_provider::delete_client(&s.pool, id).await?;
    let cid = cid.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "oauth.client_deleted", Some(&cid), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn admin_rotate(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<SettingsManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let secret = format!("pcs_{}", pelty_auth::token::generate());
    let cid = oauth_provider::set_secret(&s.pool, id, &sha(&secret)).await?;
    let cid = cid.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "oauth.client_secret_rotated", Some(&cid), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "client_id": cid, "client_secret": secret })))
}

// account

pub async fn my_apps(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let rows = oauth_provider::consents_of(&s.pool, u.user_id).await?;
    let apps: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, scopes, at)| json!({ "id": id, "name": name, "scopes": scopes, "granted_at": at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default() }))
        .collect();
    Ok(Json(json!({ "apps": apps })))
}

pub async fn my_revoke(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    if !oauth_provider::revoke_consent(&s.pool, u.user_id, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "oauth.consent_revoked", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uri_rules() {
        assert!(check_uris(&["https://app.example.com/cb".into()]).is_ok());
        assert!(check_uris(&["http://127.0.0.1:8765/cb".into()]).is_ok());
        assert!(check_uris(&["com.example.app:/cb".into()]).is_ok());
        assert!(check_uris(&["http://evil.example.com/cb".into()]).is_err(), "plain http only for loopback");
        assert!(check_uris(&["https://a.example/cb#frag".into()]).is_err());
        assert!(check_uris(&["javascript:alert(1)".into()]).is_err());
        assert!(check_uris(&[]).is_err());
    }

    #[test]
    fn error_redirect_keeps_state_and_query() {
        assert_eq!(redirect_error("https://a/cb?x=1", "access_denied", Some("s 1")), "https://a/cb?x=1&error=access_denied&state=s+1");
    }

    #[test]
    fn es256_signature_verifies() {
        let rng = SystemRandom::new();
        let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, doc.as_ref(), &rng).unwrap();
        let pubkey = pair.public_key().as_ref().to_vec();
        let k = SigningKey { kid: "k".into(), pair };
        let t = jwt(&k, &json!({"sub": "x"})).map_err(|e| e.0).unwrap();
        let (input, sig) = t.rsplit_once('.').unwrap();
        let sig = URL_SAFE_NO_PAD.decode(sig).unwrap();
        ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &pubkey).verify(input.as_bytes(), &sig).unwrap();
    }
}
