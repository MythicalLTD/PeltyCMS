//! Social / SSO login (OAuth 2.0 authorization code + PKCE, OpenID Connect).
//!
//! Providers are addons (`kind: sso`): generic OIDC/OAuth2, Google, GitHub,
//! Discord, Microsoft and Facebook. Each addon's settings carry the client
//! id/secret and the behaviour switches (`allow_register`,
//! `link_by_verified_email`, `allow_staff`). Endpoint URLs can be overridden
//! per addon (required for `oidc`; optional for the others, e.g. a proxy or a
//! local mock in tests). All provider HTTP goes through the shared transport.
//! SAML 2.0 is not implemented (see `make docs`).
use crate::addons::Manifest;
use crate::error::ApiResult;
use crate::extract::{CurrentUser, SESSION_COOKIE};
use crate::routes::auth::AuthGuard;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use pelty_core::{AppError, AppResult};
use pelty_database::{addon_registry as reg, audit, security, users};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const ENGINES: &[&str] = &[
    "oidc",
    "google",
    "github",
    "discord",
    "microsoft",
    "facebook",
];

/// Endpoints and display name from the provider addon's `describe()`.
struct Endpoints {
    name: String,
    authorize: String,
    token: String,
    userinfo: String,
    scope: String,
}

fn endpoints(
    s: &AppState,
    engine: &str,
) -> Option<(Endpoints, std::sync::Arc<pelty_wasm::WasmAddon>)> {
    let a = crate::wasm_addons::loaded(s, "sso", engine)?;
    let g = |k: &str| a.descriptor[k].as_str().unwrap_or("").to_string();
    let ep = Endpoints {
        name: g("name"),
        authorize: g("authorize_url"),
        token: g("token_url"),
        userinfo: g("userinfo_url"),
        scope: g("scope"),
    };
    Some((ep, a))
}

/// OAuth / OIDC providers are WASM addons (extensions/addons/sso-*); none is built in.
pub fn builtin_manifests() -> Vec<Manifest> {
    vec![]
}

struct Provider {
    addon: std::sync::Arc<pelty_wasm::WasmAddon>,
    name: String,
    id: String,
    engine: String,
    cfg: Value,
    authorize: String,
    token: String,
    userinfo: String,
    scope: String,
}

fn pick(cfg: &Value, key: &str, default: &str) -> String {
    match cfg[key].as_str() {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}
fn flag(cfg: &Value, key: &str, default: bool) -> bool {
    cfg[key].as_bool().unwrap_or(default)
}

async fn provider(s: &AppState, id: &str) -> AppResult<Provider> {
    let mut a = reg::get(&s.pool, id)
        .await?
        .filter(|a| a.enabled && a.kind == "sso")
        .ok_or(AppError::NotFound)?;
    a.settings = crate::addon_secrets::open(s, &a.settings);
    let (ep, addon) = endpoints(s, &a.engine).ok_or(AppError::NotFound)?;
    let p = Provider {
        addon,
        name: ep.name.clone(),
        id: a.id.clone(),
        engine: a.engine.clone(),
        authorize: pick(&a.settings, "authorize_url", &ep.authorize),
        token: pick(&a.settings, "token_url", &ep.token),
        userinfo: pick(&a.settings, "userinfo_url", &ep.userinfo),
        scope: pick(&a.settings, "scope", &ep.scope),
        cfg: a.settings,
    };
    if p.cfg["client_id"].as_str().unwrap_or("").is_empty()
        || p.authorize.is_empty()
        || p.token.is_empty()
        || p.userinfo.is_empty()
    {
        return Err(AppError::NotFound);
    }
    Ok(p)
}

fn redirect_uri(s: &AppState, id: &str) -> String {
    format!(
        "{}/api/v1/auth/oauth/{id}/callback",
        s.cfg.public_url.trim_end_matches('/')
    )
}

fn see_other(to: &str) -> Response {
    let mut r = StatusCode::SEE_OTHER.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/oauth/providers", get(providers))
        .route("/auth/oauth/{id}/start", get(start))
        .route("/auth/oauth/{id}/callback", get(callback))
        .route("/account/identities", get(list_identities))
        .route("/account/identities/{id}", delete(unlink))
}

/// Public: enabled providers for "Login with …" buttons.
#[utoipa::path(get, path = "/api/v1/auth/oauth/providers", responses((status = 200)))]
pub async fn providers(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let mut out = vec![];
    for a in reg::list(&s.pool).await? {
        if a.kind != "sso" || !a.enabled {
            continue;
        }
        if let Ok(p) = provider(&s, &a.id).await {
            // Unconfigured providers stay hidden.
            if p.cfg["client_id"].as_str().unwrap_or("").is_empty()
                || p.authorize.is_empty()
                || p.token.is_empty()
            {
                continue;
            }
            let name = if p.name.is_empty() {
                "SSO"
            } else {
                p.name.as_str()
            };
            out.push(json!({
                "id": p.id,
                "engine": p.engine,
                "label": pick(&p.cfg, "button_label", name),
                "allow_register": flag(&p.cfg, "allow_register", true),
                "staff": flag(&p.cfg, "allow_staff", false),
            }));
        }
    }
    Ok(Json(json!({ "providers": out })))
}

#[derive(Deserialize)]
pub struct StartQuery {
    #[serde(default)]
    pub mode: Option<String>,
    /// `json` returns `{url}` instead of redirecting.
    #[serde(default)]
    pub format: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/auth/oauth/{id}/start", params(("id" = String, Path)), responses((status = 303), (status = 404)))]
pub async fn start(
    State(s): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<StartQuery>,
) -> ApiResult<Response> {
    let p = provider(&s, &id).await?;
    let mode = q.mode.as_deref().unwrap_or("login");
    let user_id = if mode == "link" {
        let tok =
            crate::extract::cookie(&headers, SESSION_COOKIE).ok_or(AppError::Unauthenticated)?;
        Some(
            s.auth
                .authenticate(&tok)
                .await?
                .ok_or(AppError::Unauthenticated)?
                .user_id,
        )
    } else if mode == "login" {
        None
    } else {
        return Err(AppError::Validation("error.oauth_mode").into());
    };
    let state = pelty_auth::token::generate();
    let verifier = pelty_auth::token::generate();
    let nonce = pelty_auth::token::generate();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    security::put_oauth_state(
        &s.pool,
        &pelty_auth::token::hash(&state),
        &p.id,
        &verifier,
        &nonce,
        mode,
        user_id,
    )
    .await?;
    let mut url = form_urlencoded::Serializer::new(String::new());
    url.append_pair("response_type", "code")
        .append_pair("client_id", p.cfg["client_id"].as_str().unwrap_or(""))
        .append_pair("redirect_uri", &redirect_uri(&s, &p.id))
        .append_pair("scope", &p.scope)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    if p.scope.split(' ').any(|x| x == "openid") {
        url.append_pair("nonce", &nonce);
    }
    let sep = if p.authorize.contains('?') { '&' } else { '?' };
    let full = format!("{}{sep}{}", p.authorize, url.finish());
    if q.format.as_deref() == Some("json") {
        return Ok(Json(json!({ "url": full })).into_response());
    }
    Ok(see_other(&full))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Normalized provider profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
    pub name: String,
}

/// Code exchange + userinfo through the provider addon (op `exchange`).
async fn fetch_profile(
    s: &AppState,
    p: &Provider,
    code: &str,
    verifier: &str,
) -> AppResult<Profile> {
    let fail = |e: pelty_wasm::WasmError| {
        tracing::warn!(error = %e, provider = %p.id, "oauth exchange failed");
        AppError::Validation("error.oauth_failed")
    };
    let ctx = pelty_wasm::CallCtx {
        settings: p.cfg.clone(),
        ..Default::default()
    };
    let input = json!({
        "code": code, "code_verifier": verifier, "redirect_uri": redirect_uri(s, &p.id),
        "token_url": p.token, "userinfo_url": p.userinfo, "settings": p.cfg,
    });
    let v = p
        .addon
        .call_async(ctx, "exchange".into(), input)
        .await
        .map_err(fail)?;
    let g = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let prof = Profile {
        subject: g("subject"),
        email: g("email"),
        email_verified: v["email_verified"].as_bool().unwrap_or(false),
        name: g("name"),
    };
    if prof.subject.is_empty() {
        return Err(AppError::Validation("error.oauth_failed"));
    }
    Ok(prof)
}

async fn is_staff(s: &AppState, user: Uuid) -> AppResult<bool> {
    Ok(users::roles(&s.pool, user)
        .await?
        .iter()
        .any(|r| r != "customer"))
}

#[utoipa::path(get, path = "/api/v1/auth/oauth/{id}/callback", params(("id" = String, Path)), responses((status = 303)))]
pub async fn callback(
    State(s): State<AppState>,
    g: AuthGuard,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> ApiResult<Response> {
    match finish(&s, &g, &id, &headers, q).await {
        Ok(r) => Ok(r),
        Err(e) => {
            let code = match &e {
                AppError::Validation(c) | AppError::Conflict(c) | AppError::Denied(c) => {
                    c.trim_start_matches("error.")
                }
                _ => "oauth_failed",
            };
            Ok(see_other(&format!("/login?oauth_error={code}")))
        }
    }
}

async fn finish(
    s: &AppState,
    g: &AuthGuard,
    id: &str,
    headers: &HeaderMap,
    q: CallbackQuery,
) -> AppResult<Response> {
    if q.error.is_some() {
        return Err(AppError::Validation("error.oauth_cancelled"));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return Err(AppError::Validation("error.oauth_failed"));
    };
    let st = security::take_oauth_state(&s.pool, &pelty_auth::token::hash(&state))
        .await?
        .ok_or(AppError::Validation("error.oauth_state"))?;
    if st.provider != id {
        return Err(AppError::Validation("error.oauth_state"));
    }
    let p = provider(s, id).await?;
    let prof = fetch_profile(s, &p, &code, &st.verifier).await?;
    let ip = g.ip();

    if st.mode == "link" {
        let uid = st.user_id.ok_or(AppError::Unauthenticated)?;
        security::link_identity(&s.pool, uid, &p.id, &prof.subject, &prof.email).await?;
        audit::record(
            &s.pool,
            Some(uid),
            "auth.identity_linked",
            Some(&p.id),
            ip,
            json!({ "provider": p.id }),
        )
        .await?;
        return Ok(see_other(&format!("/sessions?linked={}", p.id)));
    }

    let user = match security::find_identity(&s.pool, &p.id, &prof.subject).await? {
        Some(i) => {
            security::touch_identity(&s.pool, i.id).await?;
            users::find(&s.pool, i.user_id)
                .await?
                .ok_or(AppError::Unauthenticated)?
        }
        None => {
            let existing = if prof.email.is_empty() {
                None
            } else {
                users::find_by_email(&s.pool, &prof.email).await?
            };
            match existing {
                Some(u)
                    if prof.email_verified
                        && flag(&p.cfg, "link_by_verified_email", false)
                        && !is_staff(s, u.id).await? =>
                {
                    security::link_identity(&s.pool, u.id, &p.id, &prof.subject, &prof.email)
                        .await?;
                    audit::record(
                        &s.pool,
                        Some(u.id),
                        "auth.identity_linked",
                        Some(&p.id),
                        ip,
                        json!({ "provider": p.id, "by": "verified_email" }),
                    )
                    .await?;
                    u
                }
                Some(_) => return Err(AppError::Conflict("error.oauth_email_in_use")),
                None if flag(&p.cfg, "allow_register", true) && !prof.email.is_empty() => {
                    let pw = format!("{}Aa1!", pelty_auth::token::generate());
                    let name: String = if prof.name.is_empty() {
                        prof.email.split('@').next().unwrap_or("").to_string()
                    } else {
                        prof.name.clone()
                    };
                    let name: String = name.chars().take(100).collect();
                    let u = s
                        .auth
                        .register(
                            pelty_auth::RegisterInput {
                                email: &prof.email,
                                password: &pw,
                                display_name: &name,
                            },
                            "customer",
                            ip,
                        )
                        .await?;
                    if prof.email_verified {
                        users::mark_verified(&s.pool, u.id).await?;
                    }
                    security::link_identity(&s.pool, u.id, &p.id, &prof.subject, &prof.email)
                        .await?;
                    audit::record(
                        &s.pool,
                        Some(u.id),
                        "auth.registered_via_sso",
                        Some(&p.id),
                        ip,
                        json!({ "provider": p.id }),
                    )
                    .await?;
                    s.emit(pelty_extensions::events::DomainEvent::new(
                        "user.registered",
                        json!({ "user_id": u.id, "via": p.id }),
                    ));
                    users::find(&s.pool, u.id)
                        .await?
                        .ok_or(AppError::Unauthenticated)?
                }
                None => return Err(AppError::Validation("error.oauth_not_linked")),
            }
        }
    };
    if !user.is_active {
        return Err(AppError::Validation("error.oauth_failed"));
    }
    let staff = is_staff(s, user.id).await?;
    if staff && !flag(&p.cfg, "allow_staff", false) {
        return Err(AppError::Denied("error.oauth_staff_disabled"));
    }
    if users::is_locked(&s.pool, user.id).await? {
        return Err(AppError::Validation("error.account_locked"));
    }
    // A second factor is never skipped by social login.
    if crate::duo::applies(s, user.id).await? {
        let ch = crate::routes::mfa::start_challenge(s, user.id)
            .await
            .map_err(|_| AppError::Validation("error.oauth_failed"))?;
        return Ok(see_other(&format!("/login?mfa_challenge={ch}&duo=1")));
    }
    if pelty_database::mfa::is_enabled(&s.pool, user.id).await? {
        let ch = crate::routes::mfa::start_challenge(s, user.id)
            .await
            .map_err(|_| AppError::Validation("error.oauth_failed"))?;
        return Ok(see_other(&format!("/login?mfa_challenge={ch}")));
    }
    audit::record(
        &s.pool,
        Some(user.id),
        "auth.login_sso",
        Some(&p.id),
        ip,
        json!({ "provider": p.id }),
    )
    .await?;
    let r = crate::routes::auth::session_response(s, g, headers, user)
        .await
        .map_err(|_| AppError::Validation("error.oauth_failed"))?;
    let mut out = see_other(if staff { "/admin" } else { "/dashboard" });
    for c in r.headers().get_all(header::SET_COOKIE) {
        out.headers_mut().append(header::SET_COOKIE, c.clone());
    }
    Ok(out)
}

#[utoipa::path(get, path = "/api/v1/account/identities", responses((status = 200), (status = 401)))]
pub async fn list_identities(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "identities": security::list_identities(&s.pool, u.user_id).await? }),
    ))
}

#[utoipa::path(delete, path = "/api/v1/account/identities/{id}", params(("id" = Uuid, Path)), responses((status = 204), (status = 404)))]
pub async fn unlink(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let provider = security::unlink_identity(&s.pool, u.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.identity_unlinked",
        Some(&provider),
        u.ip.as_deref(),
        json!({ "provider": provider }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
