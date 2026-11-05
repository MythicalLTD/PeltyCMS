//! Admin account security: step-up re-authentication ("sudo mode") and an
//! authenticated password change (needed once a password has expired).
//! The enforcement itself lives in `RequirePermission` (see `extract.rs`).
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use pelty_auth::password;
use pelty_core::AppError;
use pelty_database::{audit, mfa, sessions, users};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/reauth", post(reauth))
        .route("/auth/password/change", post(change_password))
}

#[derive(Deserialize, ToSchema)]
pub struct ReauthBody {
    pub password: String,
    /// TOTP or backup code; required when 2FA is enabled.
    #[serde(default)]
    pub code: String,
}

pub(crate) async fn verify_factors(
    s: &AppState,
    u: &CurrentUser,
    pw: &str,
    code: &str,
) -> ApiResult<()> {
    let key = format!("reauth:{}", u.user_id);
    if s.auth_limiter.exceeded(&key) {
        return Err(AppError::RateLimited.into());
    }
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    if !password::verify(pw, &user.password_hash) {
        s.auth_limiter.check(&key);
        audit::record(
            &s.pool,
            Some(u.user_id),
            "auth.reauth_failed",
            None,
            u.ip.as_deref(),
            json!({}),
        )
        .await?;
        return Err(AppError::Validation("error.invalid_password").into());
    }
    if mfa::is_enabled(&s.pool, u.user_id).await?
        && crate::routes::mfa::check_code(s, u.user_id, code)
            .await?
            .is_none()
    {
        s.auth_limiter.check(&key);
        return Err(AppError::Validation("error.mfa_invalid_code").into());
    }
    Ok(())
}

/// Confirm the password (and 2FA code) again; unlocks sensitive admin
/// changes for `admin_reauth_minutes`.
#[utoipa::path(post, path = "/api/v1/auth/reauth", request_body = ReauthBody, responses((status = 204), (status = 422), (status = 429)))]
pub async fn reauth(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<ReauthBody>,
) -> ApiResult<StatusCode> {
    if u.api_key {
        return Err(AppError::Forbidden.into());
    }
    verify_factors(&s, &u, &b.password, &b.code).await?;
    sessions::mark_reauth(&s.pool, u.user_id, u.session_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.reauth",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePasswordBody {
    pub current_password: String,
    pub new_password: String,
    #[serde(default)]
    pub code: String,
}

/// Change the own password (current password + 2FA code); signs out every
/// other session.
#[utoipa::path(post, path = "/api/v1/auth/password/change", request_body = ChangePasswordBody, responses((status = 204), (status = 422), (status = 429)))]
pub async fn change_password(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<ChangePasswordBody>,
) -> ApiResult<StatusCode> {
    if u.api_key {
        return Err(AppError::Forbidden.into());
    }
    verify_factors(&s, &u, &b.current_password, &b.code).await?;
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    if b.new_password == b.current_password {
        return Err(AppError::Validation("error.password_reused").into());
    }
    pelty_auth::policy::load(&s.pool)
        .await?
        .check_password(&b.new_password, &user.email)?;
    users::set_password(&s.pool, u.user_id, &password::hash(&b.new_password)?).await?;
    let revoked = sessions::revoke_others(&s.pool, u.user_id, u.session_id).await?;
    sessions::mark_reauth(&s.pool, u.user_id, u.session_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.password_changed",
        None,
        u.ip.as_deref(),
        json!({ "sessions_revoked": revoked }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
