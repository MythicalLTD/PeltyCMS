//! Social / SSO login (OAuth 2.0 authorization code + PKCE, OpenID Connect).
//!
//! Providers are addons (`kind: sso`): generic OIDC/OAuth2, Google, GitHub,
//! Discord, Microsoft and Facebook. Each addon's settings carry the client
//! id/secret and the behaviour switches (`allow_register`,
//! `link_by_verified_email`, `allow_staff`). Endpoint URLs can be overridden
//! per addon (required for `oidc`; optional for the others, e.g. a proxy or a
//! local mock in tests). All provider HTTP goes through the shared transport.
//! SAML 2.0 lives in `saml`. Game-store sign-in that is not OAuth (Cfx.re
//! forum keys for FiveM, Steam OpenID 2.0) shares the account side here and
//! its protocols in [`crate::game_sso`].
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
    "cfx",
    "steam",
];

/// Endpoints and display name from the provider addon's `describe()`.
struct Endpoints {
    name: String,
    authorize: String,
    token: String,
    userinfo: String,
    scope: String,
    /// Empty for OAuth 2.0 / OIDC; see [`crate::game_sso`].
    protocol: String,
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
        protocol: g("protocol"),
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
    protocol: String,
}

impl Provider {
    fn game(&self) -> bool {
        crate::game_sso::is_game(&self.protocol)
    }
    /// Shown and usable: OAuth needs a client id and endpoints; game
    /// sign-in has no client registration, so the admin opts in with
    /// `show_button`.
    fn configured(&self) -> bool {
        if self.game() {
            return flag(&self.cfg, "show_button", false) && !self.authorize.is_empty();
        }
        !self.cfg["client_id"].as_str().unwrap_or("").is_empty()
            && !self.authorize.is_empty()
            && !self.token.is_empty()
            && !self.userinfo.is_empty()
    }
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
        protocol: ep.protocol,
        cfg: a.settings,
    };
    if !p.configured() {
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
        .route("/auth/oauth/signup/{token}", get(signup_info))
        .route("/auth/oauth/signup", axum::routing::post(signup))
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
        // Unconfigured providers stay hidden (`provider` fails).
        if let Ok(p) = provider(&s, &a.id).await {
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
                // Game-store sign-in (FiveM, Steam): shown first and larger.
                "featured": p.game(),
                "custom_label": !pick(&p.cfg, "button_label", "").is_empty(),
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
    /// Same-site path to land on afterwards (e.g. `/checkout`).
    #[serde(default)]
    pub return_to: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/auth/oauth/{id}/start", params(("id" = String, Path)), responses((status = 303), (status = 404)))]
pub async fn start(
    State(s): State<AppState>,
    _g: AuthGuard,
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
    let return_to = crate::game_sso::safe_return_to(q.return_to.as_deref());
    if p.game() {
        return start_game(&s, &p, &state, &nonce, mode, user_id, return_to, q.format).await;
    }
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    security::put_oauth_state_ext(
        &s.pool,
        &pelty_auth::token::hash(&state),
        &p.id,
        &verifier,
        &nonce,
        mode,
        user_id,
        security::StateExtra {
            bind_hash: None,
            return_to: return_to.as_deref(),
        },
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

/// Callback URL of a game sign-in attempt; the provider must send the
/// browser back to exactly this.
fn game_callback(s: &AppState, id: &str, state: &str) -> String {
    let mut q = form_urlencoded::Serializer::new(String::new());
    q.append_pair("state", state);
    format!("{}?{}", redirect_uri(s, id), q.finish())
}

#[allow(clippy::too_many_arguments)]
async fn start_game(
    s: &AppState,
    p: &Provider,
    state: &str,
    nonce: &str,
    mode: &str,
    user_id: Option<Uuid>,
    return_to: Option<String>,
    format: Option<String>,
) -> ApiResult<Response> {
    let bind = pelty_auth::token::generate();
    security::put_oauth_state_ext(
        &s.pool,
        &pelty_auth::token::hash(state),
        &p.id,
        "",
        nonce,
        mode,
        user_id,
        security::StateExtra {
            bind_hash: Some(&pelty_auth::token::hash(&bind)),
            return_to: return_to.as_deref(),
        },
    )
    .await?;
    let callback = game_callback(s, &p.id, state);
    let input = crate::game_sso::StartInput {
        addon_id: &p.id,
        authorize: &p.authorize,
        cfg: &p.cfg,
        callback: &callback,
        nonce,
    };
    let url = if p.protocol == crate::game_sso::DISCOURSE {
        crate::game_sso::cfx_url(s, &input).await?
    } else {
        crate::game_sso::steam_url(s, &input)
    };
    let mut r = if format.as_deref() == Some("json") {
        Json(json!({ "url": url })).into_response()
    } else {
        see_other(&url)
    };
    r.headers_mut().append(
        header::SET_COOKIE,
        crate::routes::auth::cookie_header(s, crate::game_sso::BIND_COOKIE, &bind, 600, true),
    );
    Ok(r)
}

#[derive(Deserialize, Default)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Normalized provider profile.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
    pub name: String,
    /// Display handle at the provider (Cfx.re username, Steam persona).
    pub username: String,
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
        username: String::new(),
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
    Query(raw): Query<Vec<(String, String)>>,
) -> ApiResult<Response> {
    let res = finish(&s, &g, &id, &headers, &raw).await;
    let mut out = match res {
        Ok(r) => r,
        Err(e) => {
            let code = match &e {
                AppError::Validation(c) | AppError::Conflict(c) | AppError::Denied(c) => {
                    c.trim_start_matches("error.")
                }
                _ => "oauth_failed",
            };
            see_other(&format!("/login?oauth_error={code}"))
        }
    };
    // The browser-binding cookie of a game sign-in is single use.
    if crate::extract::cookie(&headers, crate::game_sso::BIND_COOKIE).is_some() {
        out.headers_mut().append(
            header::SET_COOKIE,
            crate::routes::auth::cookie_header(&s, crate::game_sso::BIND_COOKIE, "", 0, true),
        );
    }
    Ok(out)
}

async fn finish(
    s: &AppState,
    g: &AuthGuard,
    id: &str,
    headers: &HeaderMap,
    raw: &[(String, String)],
) -> AppResult<Response> {
    let get = |k: &str| raw.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let q = CallbackQuery {
        code: get("code"),
        state: get("state"),
        error: get("error"),
    };
    if q.error.is_some() {
        return Err(AppError::Validation("error.oauth_cancelled"));
    }
    let Some(state) = q.state else {
        return Err(AppError::Validation("error.oauth_failed"));
    };
    let st = security::take_oauth_state(&s.pool, &pelty_auth::token::hash(&state))
        .await?
        .ok_or(AppError::Validation("error.oauth_state"))?;
    if st.provider != id {
        return Err(AppError::Validation("error.oauth_state"));
    }
    let p = provider(s, id).await?;
    let prof = if p.game() {
        // Bound to the browser that started the attempt.
        let cookie = crate::extract::cookie(headers, crate::game_sso::BIND_COOKIE)
            .map(|c| pelty_auth::token::hash(&c));
        match (&st.bind_hash, cookie) {
            (Some(want), Some(got)) if *want == got => {}
            _ => return Err(AppError::Validation("error.oauth_state")),
        }
        crate::game_sso::profile(
            s,
            &p.protocol,
            &p.id,
            &p.addon,
            &p.cfg,
            raw,
            &game_callback(s, &p.id, &state),
            &st.nonce,
        )
        .await?
    } else {
        let Some(code) = q.code else {
            return Err(AppError::Validation("error.oauth_failed"));
        };
        fetch_profile(s, &p, &code, &st.verifier).await?
    };
    complete(
        s,
        g,
        headers,
        &p.id,
        &p.engine,
        &p.cfg,
        prof,
        st.mode == "link",
        st.user_id,
        st.return_to,
    )
    .await
}

/// Sign in, link or register from a verified provider profile.
#[allow(clippy::too_many_arguments)]
async fn complete(
    s: &AppState,
    g: &AuthGuard,
    headers: &HeaderMap,
    pid: &str,
    engine: &str,
    cfg: &Value,
    prof: Profile,
    link: bool,
    link_user: Option<Uuid>,
    return_to: Option<String>,
) -> AppResult<Response> {
    struct P<'a> {
        id: &'a str,
        cfg: &'a Value,
    }
    let p = P { id: pid, cfg };
    let ip = g.ip();

    if link {
        let uid = link_user.ok_or(AppError::Unauthenticated)?;
        security::link_identity(&s.pool, uid, p.id, &prof.subject, &prof.email).await?;
        remember_username(s, p.id, &prof).await?;
        link_game_ids(s, engine, uid, &prof.subject).await;
        audit::record(
            &s.pool,
            Some(uid),
            "auth.identity_linked",
            Some(p.id),
            ip,
            json!({ "provider": p.id }),
        )
        .await?;
        return Ok(see_other(&format!("/sessions?linked={}", p.id)));
    }

    let user = match security::find_identity(&s.pool, p.id, &prof.subject).await? {
        Some(i) => {
            security::touch_identity(&s.pool, i.id).await?;
            remember_username(s, p.id, &prof).await?;
            link_game_ids(s, engine, i.user_id, &prof.subject).await;
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
                        && flag(p.cfg, "link_by_verified_email", false)
                        && !is_staff(s, u.id).await? =>
                {
                    security::link_identity(&s.pool, u.id, p.id, &prof.subject, &prof.email)
                        .await?;
                    remember_username(s, p.id, &prof).await?;
                    link_game_ids(s, engine, u.id, &prof.subject).await;
                    audit::record(
                        &s.pool,
                        Some(u.id),
                        "auth.identity_linked",
                        Some(p.id),
                        ip,
                        json!({ "provider": p.id, "by": "verified_email" }),
                    )
                    .await?;
                    u
                }
                Some(_) => return Err(AppError::Conflict("error.oauth_email_in_use")),
                // Steam / Cfx.re never share an e-mail: ask for one first.
                None if flag(p.cfg, "allow_register", true) && prof.email.is_empty() => {
                    let token = pelty_auth::token::generate();
                    security::put_sso_signup(
                        &s.pool,
                        &pelty_auth::token::hash(&token),
                        &security::SsoSignup {
                            provider: p.id.to_string(),
                            subject: prof.subject.clone(),
                            name: if prof.name.is_empty() {
                                prof.username.clone()
                            } else {
                                prof.name.clone()
                            },
                            username: prof.username.clone(),
                            return_to,
                        },
                    )
                    .await?;
                    return Ok(see_other(&format!("/login?sso_signup={token}")));
                }
                None if flag(p.cfg, "allow_register", true) => {
                    if pelty_database::banned_emails::is_banned(&s.pool, &prof.email).await? {
                        return Err(AppError::Validation("error.email_banned"));
                    }
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
                    security::link_identity(&s.pool, u.id, p.id, &prof.subject, &prof.email)
                        .await?;
                    remember_username(s, p.id, &prof).await?;
                    link_game_ids(s, engine, u.id, &prof.subject).await;
                    audit::record(
                        &s.pool,
                        Some(u.id),
                        "auth.registered_via_sso",
                        Some(p.id),
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
    sign_in(s, g, headers, p.id, p.cfg, user, return_to).await
}

/// Write the id a game-store sign-in proves onto the customer's game
/// accounts (module `game_profiles`): Steam → `steamid`, Discord →
/// `discord_id`, Cfx.re → `fivem`, on every game with an enabled store that
/// has the field. Only empty fields are filled; a different value already
/// there is left alone. Never fails the sign-in. Cfx.re's
/// `session/current.json` has no Rockstar `license:`, so none is invented:
/// checkout asks for it.
pub(crate) async fn link_game_ids(s: &AppState, engine: &str, user: Uuid, subject: &str) {
    if let Err(e) = try_link_game_ids(s, engine, user, subject).await {
        tracing::warn!(error = %e, engine, "could not link game ids from sign-in");
    }
}

async fn try_link_game_ids(s: &AppState, engine: &str, user: Uuid, subject: &str) -> AppResult<()> {
    use pelty_database::{feature_modules, games};
    let Some(field) = games::sso_field(engine) else {
        return Ok(());
    };
    if !feature_modules::is_enabled(&s.pool, "game_profiles").await?
        || games::normalize_identity(field, subject).is_none()
        || is_staff(s, user).await?
        || pelty_database::storefront::contact_for_user(&s.pool, user)
            .await?
            .is_some()
    {
        return Ok(());
    }
    let list = games::store_games_with(&s.pool, field).await?;
    if list.is_empty() {
        return Ok(());
    }
    let cid = s.clients().id_for_user(user).await?;
    let wrote = games::fill_missing(&s.pool, cid, &list, field, subject).await?;
    if wrote.is_empty() {
        return Ok(());
    }
    audit::record(
        &s.pool,
        Some(user),
        "game_account.linked",
        Some(&cid.to_string()),
        None,
        json!({ "games": wrote, "field": field, "via": engine }),
    )
    .await?;
    if feature_modules::is_enabled(&s.pool, crate::routes::gaming::delivery::MODULE).await?
        && pelty_database::game_delivery::release_waiting(&s.pool, cid).await? > 0
    {
        crate::routes::gaming::delivery::kick(s);
    }
    Ok(())
}

/// Keep the provider handle on the identity.
async fn remember_username(s: &AppState, pid: &str, prof: &Profile) -> AppResult<()> {
    if !prof.username.is_empty() {
        security::set_identity_username(&s.pool, pid, &prof.subject, &prof.username).await?;
    }
    Ok(())
}

/// Session for `user` signed in through provider `pid` (or the MFA step).
async fn sign_in(
    s: &AppState,
    g: &AuthGuard,
    headers: &HeaderMap,
    pid: &str,
    cfg: &Value,
    user: users::User,
    return_to: Option<String>,
) -> AppResult<Response> {
    let ip = g.ip();
    if !user.is_active {
        return Err(AppError::Validation("error.oauth_failed"));
    }
    let staff = is_staff(s, user.id).await?;
    if staff && !flag(cfg, "allow_staff", false) {
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
        Some(pid),
        ip,
        json!({ "provider": pid }),
    )
    .await?;
    let r = crate::routes::auth::session_response(s, g, headers, user)
        .await
        .map_err(|_| AppError::Validation("error.oauth_failed"))?;
    let to = match return_to {
        Some(t) if !staff => t,
        _ if staff => "/admin".to_string(),
        _ => "/dashboard".to_string(),
    };
    let mut out = see_other(&to);
    for c in r.headers().get_all(header::SET_COOKIE) {
        out.headers_mut().append(header::SET_COOKIE, c.clone());
    }
    Ok(out)
}

fn signup_hash(token: &str) -> Vec<u8> {
    pelty_auth::token::hash(token.trim()).to_vec()
}

/// Public: what the "finish signing up" form shows for a pending sign-up.
#[utoipa::path(get, path = "/api/v1/auth/oauth/signup/{token}", params(("token" = String, Path)), responses((status = 200), (status = 404)))]
pub async fn signup_info(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Json<Value>> {
    let su = security::peek_sso_signup(&s.pool, &signup_hash(&token))
        .await?
        .ok_or(AppError::NotFound)?;
    let label = endpoints_name(&s, &su.provider).await;
    Ok(Json(json!({
        "provider": su.provider, "provider_name": label, "name": su.name,
    })))
}

async fn endpoints_name(s: &AppState, id: &str) -> String {
    match provider(s, id).await {
        Ok(p) => pick(&p.cfg, "button_label", &p.name),
        Err(_) => String::new(),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SignupBody {
    pub token: String,
    pub email: String,
}

/// Finish a game sign-in that had no e-mail (Steam, Cfx.re): create the
/// customer with the entered e-mail (unverified, a verification mail goes
/// out), link the identity and sign in. An e-mail that already has an
/// account is refused: sign in there and link from the account page.
#[utoipa::path(post, path = "/api/v1/auth/oauth/signup", request_body = SignupBody,
    responses((status = 200, description = "`{redirect}` with the session cookie"), (status = 404), (status = 409), (status = 422), (status = 429)))]
pub async fn signup(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<SignupBody>,
) -> ApiResult<Response> {
    let email = b.email.trim().to_string();
    if !crate::routes::guest::email_ok(&email) {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    let hash = signup_hash(&b.token);
    let su = security::peek_sso_signup(&s.pool, &hash)
        .await?
        .ok_or(AppError::Validation("error.oauth_state"))?;
    let p = provider(&s, &su.provider).await?;
    if !flag(&p.cfg, "allow_register", true) {
        return Err(AppError::Validation("error.oauth_not_linked").into());
    }
    if pelty_database::banned_emails::is_banned(&s.pool, &email).await? {
        return Err(AppError::Validation("error.email_banned").into());
    }
    if users::find_by_email(&s.pool, &email).await?.is_some() {
        return Err(AppError::Conflict("error.oauth_email_in_use").into());
    }
    if security::find_identity(&s.pool, &p.id, &su.subject)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict("error.identity_linked").into());
    }
    // One use from here on.
    let su = security::take_sso_signup(&s.pool, &hash)
        .await?
        .ok_or(AppError::Validation("error.oauth_state"))?;
    let pre = crate::addon_ext::pre_event(
        &s,
        "client.before_register",
        json!({ "email": email, "display_name": su.name, "ip": g.ip() }),
    )
    .await?;
    let name: String = pre
        .get("display_name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            if su.name.trim().is_empty() {
                email.split('@').next().unwrap_or("").to_string()
            } else {
                su.name.clone()
            }
        })
        .chars()
        .take(100)
        .collect();
    let pw = format!("{}Aa1!", pelty_auth::token::generate());
    let u = s
        .auth
        .register(
            pelty_auth::RegisterInput {
                email: &email,
                password: &pw,
                display_name: &name,
            },
            "customer",
            g.ip(),
        )
        .await?;
    security::link_identity(&s.pool, u.id, &p.id, &su.subject, "").await?;
    if !su.username.is_empty() {
        security::set_identity_username(&s.pool, &p.id, &su.subject, &su.username).await?;
    }
    link_game_ids(&s, &p.engine, u.id, &su.subject).await;
    crate::routes::affiliates::attribute_signup(&s, &headers, u.id).await;
    if let Err(e) = crate::routes::account::send_verification(&s, &u).await {
        tracing::error!(error = %e, "could not queue verification email");
    }
    audit::record(
        &s.pool,
        Some(u.id),
        "auth.registered_via_sso",
        Some(&p.id),
        g.ip(),
        json!({ "provider": p.id }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.registered",
        json!({ "user_id": u.id, "via": p.id }),
    ));
    let user = users::find(&s.pool, u.id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    let r = sign_in(&s, &g, &headers, &p.id, &p.cfg, user, su.return_to).await?;
    // JSON for the form: where the redirect would have gone, same cookies.
    let to = r
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/dashboard")
        .to_string();
    let mut out = Json(json!({ "redirect": to })).into_response();
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
