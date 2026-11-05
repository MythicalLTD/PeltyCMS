//! Login, logout, register and session routes for clients and staff.
use crate::error::{ApiError, ApiResult};
use crate::extract::{client_ip, CurrentUser, SESSION_COOKIE};
use crate::AppState;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_auth::{LoginInput, RegisterInput};
use pelty_core::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route(
            "/auth/sessions",
            get(list_sessions).delete(revoke_other_sessions),
        )
        .route("/auth/sessions/{id}", delete(revoke_session))
        .route("/auth/locale", put(set_locale))
}

#[derive(Deserialize, ToSchema)]
pub struct RegisterBody {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct MeResponse {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub locale: Option<String>,
    pub email_verified: bool,
    pub mfa_enabled: bool,
    /// Passkeys are enabled as a second factor (see `/auth/mfa/passkey`).
    pub passkey_2fa: bool,
    pub avatar_version: Option<i64>,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub csrf_token: String,
}

/// Client IP + rate limit check for auth endpoints.
pub struct AuthGuard {
    ip: Option<String>,
    user_agent: Option<String>,
}
impl FromRequestParts<AppState> for AuthGuard {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let ip = client_ip(parts);
        if !state.auth_limiter.check(ip.as_deref().unwrap_or("unknown")) {
            return Err(AppError::RateLimited.into());
        }
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        Ok(AuthGuard { ip, user_agent })
    }
}
impl AuthGuard {
    pub fn ip(&self) -> Option<&str> {
        self.ip.as_deref()
    }
}

pub(crate) fn cookie_header(
    state: &AppState,
    name: &str,
    value: &str,
    max_age: i64,
    http_only: bool,
) -> HeaderValue {
    let secure = if state.cfg.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let ho = if http_only { "; HttpOnly" } else { "" };
    let v = format!("{name}={value}; Path=/; Max-Age={max_age}; SameSite=Lax{ho}{secure}");
    HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static(""))
}

#[utoipa::path(post, path = "/api/v1/auth/register", request_body = RegisterBody,
    responses((status = 201), (status = 409), (status = 422), (status = 429)))]
pub async fn register(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<RegisterBody>,
) -> ApiResult<Response> {
    crate::captcha::require(&s, &headers, "register", g.ip.as_deref()).await?;
    if pelty_database::banned_emails::is_banned(&s.pool, &b.email).await? {
        return Err(pelty_core::AppError::Validation("error.email_banned").into());
    }
    // Pre-event: addons may cancel, or rewrite the display name. The
    // password never leaves the host.
    let pre = crate::addon_ext::pre_event(
        &s,
        "client.before_register",
        json!({ "email": b.email, "display_name": b.display_name, "ip": g.ip }),
    )
    .await?;
    let display_name = pre
        .get("display_name")
        .and_then(|v| v.as_str())
        .filter(|d| d.len() <= 200)
        .map(String::from)
        .unwrap_or_else(|| b.display_name.clone());
    let user = s
        .auth
        .register(
            RegisterInput {
                email: &b.email,
                password: &b.password,
                display_name: &display_name,
            },
            "customer",
            g.ip.as_deref(),
        )
        .await?;
    crate::routes::affiliates::attribute_signup(&s, &headers, user.id).await;
    if let Err(e) = crate::routes::account::send_verification(&s, &user).await {
        tracing::error!(error = %e, "could not queue verification email");
    }
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.registered",
        json!({ "user_id": user.id }),
    ));
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": user.id, "email": user.email })),
    )
        .into_response())
}

#[utoipa::path(post, path = "/api/v1/auth/login", request_body = LoginBody,
    responses((status = 200, description = "MeResponse, or `{mfa_required, challenge}` when 2FA is on"), (status = 401), (status = 429)))]
pub async fn login(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<LoginBody>,
) -> ApiResult<Response> {
    crate::captcha::require(&s, &headers, "login", g.ip.as_deref()).await?;
    crate::addon_ext::pre_event(
        &s,
        "login.before",
        json!({ "email": b.email, "ip": g.ip, "user_agent": g.user_agent }),
    )
    .await?;
    let input = LoginInput {
        email: &b.email,
        password: &b.password,
        ip: g.ip.as_deref(),
        user_agent: g.user_agent.as_deref(),
    };
    let user = match s.auth.check_credentials(&input).await {
        Ok(u) => u,
        Err(e) => {
            match crate::addon_ext::external_login(&s, &b.email, &b.password, g.ip.as_deref())
                .await?
            {
                Some(u) => u,
                None => return Err(e.into()),
            }
        }
    };
    if crate::duo::applies(&s, user.id).await? {
        let challenge = crate::routes::mfa::start_challenge(&s, user.id).await?;
        return Ok(Json(
            json!({ "mfa_required": true, "duo_required": true, "challenge": challenge }),
        )
        .into_response());
    }
    let methods = pelty_database::mfa::login_methods(&s.pool, user.id).await?;
    if !methods.is_empty() {
        let challenge = crate::routes::mfa::start_challenge(&s, user.id).await?;
        return Ok(Json(
            json!({ "mfa_required": true, "challenge": challenge, "methods": methods }),
        )
        .into_response());
    }
    session_response(&s, &g, &headers, user).await
}

/// Issue a session after all factors passed and set the cookies.
pub(crate) async fn session_response(
    s: &AppState,
    g: &AuthGuard,
    headers: &HeaderMap,
    user: pelty_database::users::User,
) -> ApiResult<Response> {
    let issued = s
        .auth
        .issue_session(user, g.ip.as_deref(), g.user_agent.as_deref())
        .await?;
    // Rotation: revoke the session the browser came in with, if any.
    if let Some(old) = crate::extract::cookie(headers, SESSION_COOKIE) {
        if let Ok(Some(prev)) = s.auth.authenticate(&old).await {
            let _ = s
                .auth
                .revoke(
                    prev.user_id,
                    prev.id,
                    g.ip.as_deref(),
                    "auth.session_rotated",
                )
                .await;
        }
    }
    login_alert(
        s,
        issued.user.id,
        issued.session.id,
        g.ip.as_deref(),
        g.user_agent.as_deref(),
    )
    .await;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.logged_in",
        json!({ "user_id": issued.user.id }),
    ));
    let max_age = s.cfg.session_absolute_hours * 3600;
    let body = me_response(s, issued.user.id, issued.csrf_token.clone()).await?;
    let mut r = Json(body).into_response();
    r.headers_mut().append(
        header::SET_COOKIE,
        cookie_header(s, SESSION_COOKIE, &issued.token, max_age, true),
    );
    if let Some(l) = &issued.user.locale {
        r.headers_mut().append(
            header::SET_COOKIE,
            cookie_header(s, "pelty_lang", l, 31_536_000, false),
        );
    }
    Ok(r)
}

async fn me_response(s: &AppState, id: Uuid, csrf: String) -> ApiResult<MeResponse> {
    let u = pelty_database::users::find(&s.pool, id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    Ok(MeResponse {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        locale: u.locale,
        email_verified: u.email_verified_at.is_some(),
        mfa_enabled: pelty_database::mfa::is_enabled(&s.pool, id).await?,
        passkey_2fa: pelty_database::mfa::passkey_2fa(&s.pool, id).await?,
        avatar_version: u.avatar_version,
        roles: pelty_database::users::roles(&s.pool, id).await?,
        permissions: pelty_database::users::permissions(&s.pool, id).await?,
        csrf_token: csrf,
    })
}

#[utoipa::path(post, path = "/api/v1/auth/logout", responses((status = 204), (status = 401), (status = 403)))]
pub async fn logout(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Response> {
    s.auth
        .revoke(u.user_id, u.session_id, u.ip.as_deref(), "auth.logout")
        .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.logged_out",
        json!({ "user_id": u.user_id }),
    ));
    let mut r = StatusCode::NO_CONTENT.into_response();
    r.headers_mut().append(
        header::SET_COOKIE,
        cookie_header(&s, SESSION_COOKIE, "", 0, true),
    );
    Ok(r)
}

#[utoipa::path(get, path = "/api/v1/auth/me", responses((status = 200, body = MeResponse), (status = 401)))]
pub async fn me(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<MeResponse>> {
    Ok(Json(me_response(&s, u.user_id, u.csrf_token).await?))
}

#[utoipa::path(get, path = "/api/v1/auth/sessions", responses((status = 200), (status = 401)))]
pub async fn list_sessions(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let list = pelty_database::sessions::list_active(&s.pool, u.user_id).await?;
    let items: Vec<Value> = list
        .into_iter()
        .map(|x| {
            let current = x.id == u.session_id;
            let mut v = serde_json::to_value(x).unwrap_or_default();
            v["current"] = json!(current);
            v
        })
        .collect();
    Ok(Json(json!({ "items": items })))
}

/// Sign out every other device; the calling session stays valid.
#[utoipa::path(delete, path = "/api/v1/auth/sessions", responses((status = 200), (status = 401), (status = 403)))]
pub async fn revoke_other_sessions(
    State(s): State<AppState>,
    u: CurrentUser,
) -> ApiResult<Json<Value>> {
    if u.api_key {
        return Err(AppError::Forbidden.into());
    }
    let n = pelty_database::sessions::revoke_others(&s.pool, u.user_id, u.session_id).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.sessions_revoked_others",
        None,
        u.ip.as_deref(),
        json!({ "count": n }),
    )
    .await?;
    Ok(Json(json!({ "revoked": n })))
}

#[utoipa::path(delete, path = "/api/v1/auth/sessions/{id}", responses((status = 204), (status = 404)))]
pub async fn revoke_session(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if s.auth
        .revoke(u.user_id, id, u.ip.as_deref(), "auth.session_revoked")
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound.into())
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LocaleBody {
    pub locale: Option<String>,
}

#[utoipa::path(put, path = "/api/v1/auth/locale", request_body = LocaleBody, responses((status = 204), (status = 422)))]
pub async fn set_locale(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<LocaleBody>,
) -> ApiResult<Response> {
    if let Some(l) = &b.locale {
        if !s.i18n.has(l) {
            return Err(AppError::Validation("error.unknown_locale").into());
        }
    }
    pelty_database::users::set_locale(&s.pool, u.user_id, b.locale.as_deref()).await?;
    let mut r = StatusCode::NO_CONTENT.into_response();
    if let Some(l) = &b.locale {
        r.headers_mut().append(
            header::SET_COOKIE,
            cookie_header(&s, "pelty_lang", l, 31_536_000, false),
        );
    }
    Ok(r)
}

/// Staff sign-in from an IP never used before: audit + `user.login_alert`
/// event (mail templates / addons can react). Best effort, never blocks login.
async fn login_alert(s: &AppState, user: Uuid, session: Uuid, ip: Option<&str>, ua: Option<&str>) {
    let Some(ip) = ip else { return };
    let Ok(pol) = pelty_auth::policy::load(&s.pool).await else {
        return;
    };
    if !pol.login_alerts {
        return;
    }
    let staff = pelty_database::users::permissions(&s.pool, user)
        .await
        .map(|p| p.iter().any(|x| x == "admin.access"))
        .unwrap_or(false);
    if !staff
        || pelty_database::sessions::seen_ip(&s.pool, user, ip, session)
            .await
            .unwrap_or(true)
    {
        return;
    }
    let geo = s.geoip.lookup(ip);
    let _ = pelty_database::audit::record(
        &s.pool,
        Some(user),
        "auth.login_new_ip",
        None,
        Some(ip),
        json!({ "user_agent": ua, "location": geo.location, "country": geo.country, "city": geo.city, "region": geo.region }),
    )
    .await;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.login_alert",
        json!({
            "user_id": user,
            "ip": ip,
            "user_agent": ua,
            "location": geo.location,
            "country": geo.country,
            "city": geo.city,
            "region": geo.region,
        }),
    ));
    // Email the account owner (the template can be switched off in Email templates).
    if let Ok(Some(u)) = pelty_database::users::find(&s.pool, user).await {
        let mut v = crate::mail::Vars::new();
        v.insert("email".into(), u.email.clone());
        v.insert("ip".into(), ip.to_string());
        v.insert(
            "user_agent".into(),
            ua.unwrap_or("unknown").chars().take(200).collect(),
        );
        v.insert(
            "time".into(),
            time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
        );
        v.insert("location".into(), geo.location.clone());
        v.insert(
            "city".into(),
            geo.city.clone().unwrap_or_else(|| "-".into()),
        );
        v.insert(
            "region".into(),
            geo.region.clone().unwrap_or_else(|| "-".into()),
        );
        v.insert(
            "country".into(),
            geo.country.clone().unwrap_or_else(|| "-".into()),
        );
        v.insert(
            "country_name".into(),
            geo.country_name.clone().unwrap_or_else(|| "-".into()),
        );
        if let Err(e) = crate::mail::enqueue(
            s,
            "admin_login_alert",
            u.locale.as_deref().unwrap_or("en"),
            &u.email,
            v,
        )
        .await
        {
            tracing::error!(error = %e, "could not queue login alert");
        }
    }
}
