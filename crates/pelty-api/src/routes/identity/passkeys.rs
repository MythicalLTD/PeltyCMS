//! Passkeys (WebAuthn, webauthn-rs): enrolment, passwordless login and
//! step-up re-auth. Recovery when a passkey is lost is the normal password
//! (+ TOTP / backup code) login; staff can revoke a user's passkeys.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::auth::{session_response, AuthGuard};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine;
use pelty_auth::token;
use pelty_core::AppError;
use pelty_database::{audit, passkeys as db, sessions, users};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
use webauthn_rs::prelude::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/passkeys", get(list))
        .route("/auth/passkeys/{id}", delete(remove))
        .route("/auth/passkeys/register/start", post(register_start))
        .route("/auth/passkeys/register/finish", post(register_finish))
        .route("/auth/passkeys/login/start", post(login_start))
        .route("/auth/passkeys/login/finish", post(login_finish))
        .route("/auth/passkeys/discover/start", post(discover_start))
        .route("/auth/passkeys/discover/finish", post(discover_finish))
        .route("/auth/passkeys/reauth/start", post(reauth_start))
        .route("/auth/passkeys/reauth/finish", post(reauth_finish))
        .route(
            "/admin/users/{id}/passkeys",
            get(admin_list).delete(admin_revoke),
        )
        .route(
            "/auth/mfa/passkey",
            get(second_factor_status).put(second_factor_set),
        )
        .route("/auth/mfa/passkey/start", post(mfa_start))
        .route("/auth/mfa/passkey/finish", post(mfa_finish))
}

/// Relying party from the public URL (rp id = host, origin = scheme+host+port).
pub fn webauthn(public_url: &str) -> Result<Webauthn, AppError> {
    let origin =
        Url::parse(public_url).map_err(|_| AppError::Conflict("error.passkeys_unavailable"))?;
    let host = origin
        .host_str()
        .ok_or(AppError::Conflict("error.passkeys_unavailable"))?
        .to_string();
    WebauthnBuilder::new(&host, &origin)
        .and_then(|b| b.rp_name("PeltyCMS").build())
        .map_err(|_| AppError::Conflict("error.passkeys_unavailable"))
}

fn wa(s: &AppState) -> Result<Webauthn, AppError> {
    webauthn(&s.cfg.public_url)
}

fn no_api_key(u: &CurrentUser) -> Result<(), AppError> {
    if u.api_key {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, AppError> {
    serde_json::to_value(v).map_err(|e| AppError::Internal(e.to_string()))
}

async fn user_passkeys(s: &AppState, user: Uuid) -> ApiResult<Vec<(Uuid, Passkey)>> {
    Ok(db::credentials(&s.pool, user)
        .await?
        .into_iter()
        .filter_map(|(id, v)| serde_json::from_value::<Passkey>(v).ok().map(|p| (id, p)))
        .collect())
}

#[utoipa::path(get, path = "/api/v1/auth/passkeys", responses((status = 200)))]
pub async fn list(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    no_api_key(&u)?;
    Ok(Json(
        json!({ "items": db::list(&s.pool, u.user_id).await? }),
    ))
}

#[utoipa::path(delete, path = "/api/v1/auth/passkeys/{id}", responses((status = 204), (status = 404)))]
pub async fn remove(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    no_api_key(&u)?;
    if db::count(&s.pool, u.user_id).await? <= 1
        && pelty_database::mfa::passkey_2fa(&s.pool, u.user_id).await?
        && !pelty_database::mfa::is_enabled(&s.pool, u.user_id).await?
    {
        // Removing the last passkey would silently switch 2FA off.
        return Err(AppError::Conflict("error.passkey_2fa_last").into());
    }
    db::delete(&s.pool, u.user_id, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.passkey_removed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RegisterStart {
    pub password: String,
    #[serde(default)]
    pub code: String,
}

/// Enrolment needs the password (and 2FA code when enabled) again.
#[utoipa::path(post, path = "/api/v1/auth/passkeys/register/start", request_body = RegisterStart, responses((status = 200), (status = 422)))]
pub async fn register_start(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<RegisterStart>,
) -> ApiResult<Json<Value>> {
    no_api_key(&u)?;
    crate::routes::admin_security::verify_factors(&s, &u, &b.password, &b.code).await?;
    if db::count(&s.pool, u.user_id).await? >= db::MAX_PER_USER {
        return Err(AppError::Conflict("error.passkey_limit").into());
    }
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    let exclude: Vec<CredentialID> = user_passkeys(&s, u.user_id)
        .await?
        .into_iter()
        .map(|(_, p)| p.cred_id().clone())
        .collect();
    let name = if user.display_name.trim().is_empty() {
        user.email.clone()
    } else {
        user.display_name.clone()
    };
    let (options, state) = wa(&s)?
        .start_passkey_registration(u.user_id, &user.email, &name, Some(exclude))
        .map_err(|e| AppError::Internal(e.to_string()))?;
    // Ask for a discoverable (resident) credential so sign-in works without
    // typing the e-mail; "preferred" keeps security keys without storage usable.
    let mut options = to_json(&options)?;
    if let Some(sel) = options
        .pointer_mut("/publicKey/authenticatorSelection")
        .and_then(Value::as_object_mut)
    {
        sel.insert("residentKey".into(), json!("preferred"));
        sel.insert("requireResidentKey".into(), json!(false));
    }
    let id = db::put_challenge(&s.pool, Some(u.user_id), "register", &to_json(&state)?).await?;
    Ok(Json(json!({ "challenge_id": id, "options": options })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RegisterFinish {
    pub challenge_id: Uuid,
    pub name: String,
    #[schema(value_type = Object)]
    pub credential: RegisterPublicKeyCredential,
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/register/finish", request_body = RegisterFinish, responses((status = 200), (status = 422)))]
pub async fn register_finish(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<RegisterFinish>,
) -> ApiResult<Json<Value>> {
    no_api_key(&u)?;
    let Some((Some(owner), state)) =
        db::take_challenge(&s.pool, b.challenge_id, "register").await?
    else {
        return Err(AppError::Validation("error.passkey_challenge_expired").into());
    };
    if owner != u.user_id {
        return Err(AppError::Validation("error.passkey_challenge_expired").into());
    }
    let state: PasskeyRegistration =
        serde_json::from_value(state).map_err(|e| AppError::Internal(e.to_string()))?;
    let pk = wa(&s)?
        .finish_passkey_registration(&b.credential, &state)
        .map_err(|e| {
            tracing::info!(error = %e, "passkey registration rejected");
            AppError::Validation("error.passkey_invalid")
        })?;
    let cred_id =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode::<&[u8]>(pk.cred_id().as_ref());
    let id = db::insert(&s.pool, u.user_id, &b.name, &cred_id, &to_json(&pk)?).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.passkey_added",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": b.name.trim() }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginStart {
    pub email: String,
}

/// Stable fake credential id for unknown e-mails, so the response doesn't
/// reveal whether an account (or a passkey) exists.
fn decoy_options(s: &AppState, email: &str) -> Value {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"pelty-passkey-decoy\0");
    h.update(s.cfg.public_url.as_bytes());
    h.update(email.trim().to_lowercase().as_bytes());
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let id = b64.encode(h.finalize());
    let mut ch = Uuid::new_v4().as_bytes().to_vec();
    ch.extend_from_slice(Uuid::new_v4().as_bytes());
    let rp = Url::parse(&s.cfg.public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    json!({ "publicKey": { "challenge": b64.encode(ch), "timeout": 300000, "rpId": rp, "allowCredentials": [{ "type": "public-key", "id": id }], "userVerification": "required" } })
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/login/start", request_body = LoginStart, responses((status = 200)))]
pub async fn login_start(
    State(s): State<AppState>,
    _g: AuthGuard,
    Json(b): Json<LoginStart>,
) -> ApiResult<Json<Value>> {
    let w = wa(&s)?;
    let user = users::find_by_email(&s.pool, b.email.trim())
        .await?
        .filter(|u| u.is_active);
    let keys = match &user {
        Some(u) => user_passkeys(&s, u.id).await?,
        None => vec![],
    };
    if let (Some(u), false) = (&user, keys.is_empty()) {
        let creds: Vec<Passkey> = keys.into_iter().map(|(_, p)| p).collect();
        let (options, state) = w
            .start_passkey_authentication(&creds)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let id = db::put_challenge(&s.pool, Some(u.id), "login", &to_json(&state)?).await?;
        return Ok(Json(json!({ "challenge_id": id, "options": options })));
    }
    let id = db::put_challenge(&s.pool, None, "login", &json!({})).await?;
    Ok(Json(
        json!({ "challenge_id": id, "options": decoy_options(&s, &b.email) }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AssertBody {
    pub challenge_id: Uuid,
    #[schema(value_type = Object)]
    pub credential: PublicKeyCredential,
}

/// Verify an assertion for `user` against the stored ceremony state and
/// persist the updated counter. Returns the passkey row id.
async fn verify_assertion(
    s: &AppState,
    user: Uuid,
    state: Value,
    cred: &PublicKeyCredential,
) -> ApiResult<Uuid> {
    let state: PasskeyAuthentication =
        serde_json::from_value(state).map_err(|_| AppError::InvalidCredentials)?;
    let res = wa(s)?
        .finish_passkey_authentication(cred, &state)
        .map_err(|e| {
            tracing::info!(error = %e, "passkey assertion rejected");
            AppError::InvalidCredentials
        })?;
    if !res.user_verified() {
        return Err(AppError::InvalidCredentials.into());
    }
    for (id, mut pk) in user_passkeys(s, user).await? {
        if pk.cred_id() == res.cred_id() {
            let changed = pk.update_credential(&res).unwrap_or(false);
            db::touch(
                &s.pool,
                id,
                if changed { Some(to_json(&pk)?) } else { None }.as_ref(),
            )
            .await?;
            return Ok(id);
        }
    }
    Err(AppError::InvalidCredentials.into())
}

/// Passwordless sign-in. A user-verified passkey counts as both factors.
#[utoipa::path(post, path = "/api/v1/auth/passkeys/login/finish", request_body = AssertBody, responses((status = 200), (status = 401)))]
pub async fn login_finish(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<AssertBody>,
) -> ApiResult<Response> {
    let Some((Some(uid), state)) = db::take_challenge(&s.pool, b.challenge_id, "login").await?
    else {
        return Err(AppError::InvalidCredentials.into());
    };
    let pid = match verify_assertion(&s, uid, state, &b.credential).await {
        Ok(p) => p,
        Err(e) => {
            audit::record(
                &s.pool,
                Some(uid),
                "auth.passkey_failed",
                None,
                g.ip(),
                json!({}),
            )
            .await?;
            return Err(e);
        }
    };
    let user = users::find(&s.pool, uid)
        .await?
        .filter(|u| u.is_active)
        .ok_or(AppError::InvalidCredentials)?;
    audit::record(
        &s.pool,
        Some(uid),
        "auth.passkey_login",
        Some(&pid.to_string()),
        g.ip(),
        json!({}),
    )
    .await?;
    session_response(&s, &g, &headers, user).await
}

/// Usernameless sign-in: empty `allowCredentials`, the authenticator picks a
/// discoverable credential and returns its user handle. Used by the passkey
/// button with an empty e-mail field and by browser autofill (conditional UI).
#[utoipa::path(post, path = "/api/v1/auth/passkeys/discover/start", responses((status = 200)))]
pub async fn discover_start(State(s): State<AppState>, _g: AuthGuard) -> ApiResult<Json<Value>> {
    let (options, state) = wa(&s)?
        .start_discoverable_authentication()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let id = db::put_challenge(&s.pool, None, "discover", &to_json(&state)?).await?;
    Ok(Json(json!({ "challenge_id": id, "options": options })))
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/discover/finish", request_body = AssertBody, responses((status = 200), (status = 401)))]
pub async fn discover_finish(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<AssertBody>,
) -> ApiResult<Response> {
    let Some((None, state)) = db::take_challenge(&s.pool, b.challenge_id, "discover").await? else {
        return Err(AppError::InvalidCredentials.into());
    };
    let state: DiscoverableAuthentication =
        serde_json::from_value(state).map_err(|_| AppError::InvalidCredentials)?;
    let w = wa(&s)?;
    let (uid, cid) = w
        .identify_discoverable_authentication(&b.credential)
        .map_err(|_| AppError::InvalidCredentials)?;
    let keys = user_passkeys(&s, uid).await?;
    if keys.is_empty() {
        return Err(AppError::InvalidCredentials.into());
    }
    let (_, pk) = keys
        .iter()
        .find(|(_, p)| AsRef::<[u8]>::as_ref(p.cred_id()) == cid)
        .ok_or(AppError::InvalidCredentials)?;
    let res = match w.finish_discoverable_authentication_v2(&b.credential, state, pk) {
        Ok(r) if r.user_verified() => r,
        _ => {
            audit::record(
                &s.pool,
                Some(uid),
                "auth.passkey_failed",
                None,
                g.ip(),
                json!({ "discoverable": true }),
            )
            .await?;
            return Err(AppError::InvalidCredentials.into());
        }
    };
    let user = users::find(&s.pool, uid)
        .await?
        .filter(|u| u.is_active)
        .ok_or(AppError::InvalidCredentials)?;
    let mut pid = None;
    for (id, mut pk) in keys {
        if pk.cred_id() == res.cred_id() {
            let changed = pk.update_credential(&res).unwrap_or(false);
            db::touch(
                &s.pool,
                id,
                if changed { Some(to_json(&pk)?) } else { None }.as_ref(),
            )
            .await?;
            db::mark_discoverable(&s.pool, id).await?;
            pid = Some(id);
        }
    }
    let pid = pid.ok_or(AppError::InvalidCredentials)?;
    audit::record(
        &s.pool,
        Some(uid),
        "auth.passkey_login",
        Some(&pid.to_string()),
        g.ip(),
        json!({ "discoverable": true }),
    )
    .await?;
    session_response(&s, &g, &headers, user).await
}

#[utoipa::path(post, path = "/api/v1/auth/passkeys/reauth/start", responses((status = 200), (status = 409)))]
pub async fn reauth_start(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    no_api_key(&u)?;
    let creds: Vec<Passkey> = user_passkeys(&s, u.user_id)
        .await?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    if creds.is_empty() {
        return Err(AppError::Conflict("error.no_passkeys").into());
    }
    let (options, state) = wa(&s)?
        .start_passkey_authentication(&creds)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let id = db::put_challenge(&s.pool, Some(u.user_id), "reauth", &to_json(&state)?).await?;
    Ok(Json(json!({ "challenge_id": id, "options": options })))
}

/// Step-up with a passkey instead of password + code.
#[utoipa::path(post, path = "/api/v1/auth/passkeys/reauth/finish", request_body = AssertBody, responses((status = 204), (status = 422)))]
pub async fn reauth_finish(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<AssertBody>,
) -> ApiResult<StatusCode> {
    no_api_key(&u)?;
    let Some((Some(owner), state)) = db::take_challenge(&s.pool, b.challenge_id, "reauth").await?
    else {
        return Err(AppError::Validation("error.passkey_challenge_expired").into());
    };
    if owner != u.user_id {
        return Err(AppError::Validation("error.passkey_challenge_expired").into());
    }
    verify_assertion(&s, u.user_id, state, &b.credential)
        .await
        .map_err(|_| AppError::Validation("error.passkey_invalid"))?;
    sessions::mark_reauth(&s.pool, u.user_id, u.session_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.reauth",
        None,
        u.ip.as_deref(),
        json!({ "method": "passkey" }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/users/{id}/passkeys", responses((status = 200)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::UsersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": db::list(&s.pool, id).await? })))
}

/// Revoke every passkey of a user (lost device / compromised account).
#[utoipa::path(delete, path = "/api/v1/admin/users/{id}/passkeys", responses((status = 200)))]
pub async fn admin_revoke(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::UsersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let n = db::delete_all(&s.pool, id).await?;
    // Account recovery: without passkeys the opt-in is meaningless.
    pelty_database::mfa::set_passkey_2fa(&s.pool, id, false).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.passkeys_revoked",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "removed": n }),
    )
    .await?;
    Ok(Json(json!({ "removed": n })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SecondFactorBody {
    pub enabled: bool,
    pub password: String,
    /// Current TOTP or backup code when TOTP is also enabled.
    #[serde(default)]
    pub code: String,
}

#[utoipa::path(get, path = "/api/v1/auth/mfa/passkey", responses((status = 200), (status = 401)))]
pub async fn second_factor_status(
    State(s): State<AppState>,
    u: CurrentUser,
) -> ApiResult<Json<Value>> {
    no_api_key(&u)?;
    Ok(Json(json!({
        "enabled": pelty_database::mfa::passkey_2fa(&s.pool, u.user_id).await?,
        "passkeys": db::count(&s.pool, u.user_id).await?,
        "totp": pelty_database::mfa::is_enabled(&s.pool, u.user_id).await?,
    })))
}

/// Use passkeys as the second factor after the password. Enabling needs a
/// registered passkey; both directions need the password (and TOTP code).
#[utoipa::path(put, path = "/api/v1/auth/mfa/passkey", request_body = SecondFactorBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn second_factor_set(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<SecondFactorBody>,
) -> ApiResult<Json<Value>> {
    no_api_key(&u)?;
    crate::routes::admin_security::verify_factors(&s, &u, &b.password, &b.code).await?;
    if b.enabled && db::count(&s.pool, u.user_id).await? == 0 {
        return Err(AppError::Conflict("error.no_passkeys").into());
    }
    if !b.enabled
        && pelty_database::staff::require_2fa(&s.pool).await?
        && u.has("admin.access")
        && !pelty_database::mfa::is_enabled(&s.pool, u.user_id).await?
    {
        return Err(AppError::Conflict("error.mfa_required_by_policy").into());
    }
    pelty_database::mfa::set_passkey_2fa(&s.pool, u.user_id, b.enabled).await?;
    let action = if b.enabled {
        "auth.passkey_2fa_enabled"
    } else {
        "auth.passkey_2fa_disabled"
    };
    audit::record(
        &s.pool,
        Some(u.user_id),
        action,
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    let ev = if b.enabled {
        "user.mfa_enabled"
    } else {
        "user.mfa_disabled"
    };
    s.emit(pelty_extensions::events::DomainEvent::new(
        ev,
        json!({ "user_id": u.user_id, "method": "passkey" }),
    ));
    Ok(Json(json!({ "enabled": b.enabled })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MfaStart {
    /// The `challenge` returned by `/auth/login`.
    pub challenge: String,
}

/// Second login step with a passkey instead of a TOTP code.
#[utoipa::path(post, path = "/api/v1/auth/mfa/passkey/start", request_body = MfaStart, responses((status = 200), (status = 409), (status = 422)))]
pub async fn mfa_start(
    State(s): State<AppState>,
    _g: AuthGuard,
    Json(b): Json<MfaStart>,
) -> ApiResult<Json<Value>> {
    // Counts as one of the challenge's attempts.
    let Some(uid) =
        pelty_database::mfa::attempt_challenge(&s.pool, &token::hash(&b.challenge)).await?
    else {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    };
    if !pelty_database::mfa::login_methods(&s.pool, uid)
        .await?
        .contains(&"passkey")
    {
        return Err(AppError::Conflict("error.no_passkeys").into());
    }
    let creds: Vec<Passkey> = user_passkeys(&s, uid)
        .await?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    let (options, state) = wa(&s)?
        .start_passkey_authentication(&creds)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let id = db::put_challenge(&s.pool, Some(uid), "mfa", &to_json(&state)?).await?;
    Ok(Json(json!({ "challenge_id": id, "options": options })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MfaFinish {
    pub challenge: String,
    pub challenge_id: Uuid,
    #[schema(value_type = Object)]
    pub credential: PublicKeyCredential,
}

#[utoipa::path(post, path = "/api/v1/auth/mfa/passkey/finish", request_body = MfaFinish, responses((status = 200, body = crate::routes::auth::MeResponse), (status = 422)))]
pub async fn mfa_finish(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<MfaFinish>,
) -> ApiResult<Response> {
    let hash = token::hash(&b.challenge);
    let Some(uid) = pelty_database::mfa::attempt_challenge(&s.pool, &hash).await? else {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    };
    let Some((Some(owner), state)) = db::take_challenge(&s.pool, b.challenge_id, "mfa").await?
    else {
        return Err(AppError::Validation("error.passkey_challenge_expired").into());
    };
    if owner != uid {
        return Err(AppError::Validation("error.passkey_challenge_expired").into());
    }
    // Duo replaces the other factors for the users it covers.
    if crate::duo::applies(&s, uid).await? {
        return Err(AppError::Validation("error.duo_required").into());
    }
    let pid = match verify_assertion(&s, uid, state, &b.credential).await {
        Ok(p) => p,
        Err(_) => {
            audit::record(
                &s.pool,
                Some(uid),
                "auth.mfa_failed",
                None,
                g.ip(),
                json!({ "method": "passkey" }),
            )
            .await?;
            return Err(AppError::Validation("error.passkey_invalid").into());
        }
    };
    if !pelty_database::mfa::finish_challenge(&s.pool, &hash).await? {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    }
    let user = users::find(&s.pool, uid)
        .await?
        .filter(|u| u.is_active)
        .ok_or(AppError::InvalidCredentials)?;
    audit::record(
        &s.pool,
        Some(uid),
        "auth.mfa_verified",
        Some(&pid.to_string()),
        g.ip(),
        json!({ "method": "passkey" }),
    )
    .await?;
    session_response(&s, &g, &headers, user).await
}
