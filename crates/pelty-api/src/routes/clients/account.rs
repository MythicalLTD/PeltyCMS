//! Password reset and email verification. Every public endpoint here is
//! rate-limited (AuthGuard) and never reveals whether an account exists.
use crate::routes::auth::AuthGuard;
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::mail;
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use pelty_auth::{password, token};
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, sessions, user_tokens, users};
use serde::Deserialize;
use serde_json::json;
use time::Duration;
use utoipa::ToSchema;

pub const RESET_TTL_MINUTES: i64 = 60;
pub const VERIFY_TTL_HOURS: i64 = 48;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/password/forgot", post(forgot))
        .route("/auth/password/reset", post(reset))
        .route("/auth/email/verify", post(verify))
        .route("/auth/email/resend", post(resend))
}

#[derive(Deserialize, ToSchema)]
pub struct ForgotBody {
    pub email: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ResetBody {
    pub token: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct TokenBody {
    pub token: String,
}

/// Issue a verification token for `u` and queue the email.
pub async fn send_verification(s: &AppState, u: &users::User) -> AppResult<()> {
    let tok = token::generate();
    user_tokens::issue(
        &s.pool,
        u.id,
        user_tokens::EMAIL_VERIFY,
        &token::hash(&tok),
        Duration::hours(VERIFY_TTL_HOURS),
    )
    .await?;
    let link = format!("{}/verify-email?token={tok}", s.cfg.public_url);
    let locale = mail::user_locale(s, u);
    mail::enqueue(
        s,
        "email_verify",
        &locale,
        &u.email,
        mail::vars(&[("name", display(u)), ("link", &link)]),
    )
    .await
}

fn display(u: &users::User) -> &str {
    if u.display_name.trim().is_empty() {
        &u.email
    } else {
        &u.display_name
    }
}

#[utoipa::path(post, path = "/api/v1/auth/password/forgot", request_body = ForgotBody,
    responses((status = 204, description = "Always, whether or not the account exists"), (status = 429)))]
pub async fn forgot(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: axum::http::HeaderMap,
    Json(b): Json<ForgotBody>,
) -> ApiResult<StatusCode> {
    crate::captcha::require(&s, &headers, "password_reset", g.ip()).await?;
    let email = b.email.trim().to_lowercase();
    if let Some(u) = users::find_by_email(&s.pool, &email)
        .await?
        .filter(|u| u.is_active)
    {
        // Cancel-only; a cancelled reset still answers 204 (no account probing).
        if crate::addon_ext::pre_event(&s, "user.before_password_reset", json!({ "user_id": u.id, "email": u.email, "ip": g.ip() })).await.is_err() {
            return Ok(StatusCode::NO_CONTENT);
        }
        let tok = token::generate();
        user_tokens::issue(
            &s.pool,
            u.id,
            user_tokens::PASSWORD_RESET,
            &token::hash(&tok),
            Duration::minutes(RESET_TTL_MINUTES),
        )
        .await?;
        let link = format!("{}/reset-password?token={tok}", s.cfg.public_url);
        let locale = mail::user_locale(&s, &u);
        let mins = RESET_TTL_MINUTES.to_string();
        mail::enqueue(
            &s,
            "password_reset",
            &locale,
            &u.email,
            mail::vars(&[
                ("name", display(&u)),
                ("link", &link),
                ("expires_minutes", &mins),
            ]),
        )
        .await?;
        audit::record(
            &s.pool,
            Some(u.id),
            "auth.password_reset_requested",
            None,
            g.ip(),
            json!({}),
        )
        .await?;
        s.emit(pelty_extensions::events::DomainEvent::new("user.password_reset_requested", json!({ "user_id": u.id })));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/auth/password/reset", request_body = ResetBody,
    responses((status = 204), (status = 422, description = "Invalid/expired token or weak password"), (status = 429)))]
pub async fn reset(
    State(s): State<AppState>,
    g: AuthGuard,
    Json(b): Json<ResetBody>,
) -> ApiResult<StatusCode> {
    // Validate the password before burning the token, so a weak choice can be corrected.
    let hash = token::hash(b.token.trim());
    let peek = user_tokens::peek(&s.pool, user_tokens::PASSWORD_RESET, &hash).await?;
    let (_, email) = peek.ok_or(AppError::Validation("error.invalid_token"))?;
    pelty_auth::policy::load(&s.pool).await?.check_password(&b.password, &email)?;
    let uid = user_tokens::consume(&s.pool, user_tokens::PASSWORD_RESET, &hash)
        .await?
        .ok_or(AppError::Validation("error.invalid_token"))?;
    users::set_password(&s.pool, uid, &password::hash(&b.password)?).await?;
    // Possession of the mailbox proves the address too.
    users::mark_verified(&s.pool, uid).await?;
    let revoked = sessions::revoke_all(&s.pool, uid).await?;
    pelty_database::api_keys::revoke_all(&s.pool, uid).await?;
    audit::record(
        &s.pool,
        Some(uid),
        "auth.password_reset",
        None,
        g.ip(),
        json!({ "sessions_revoked": revoked }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new("user.password_reset", json!({ "user_id": uid, "sessions_revoked": revoked })));
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/auth/email/verify", request_body = TokenBody,
    responses((status = 204), (status = 422), (status = 429)))]
pub async fn verify(
    State(s): State<AppState>,
    g: AuthGuard,
    Json(b): Json<TokenBody>,
) -> ApiResult<StatusCode> {
    let uid = user_tokens::consume(
        &s.pool,
        user_tokens::EMAIL_VERIFY,
        &token::hash(b.token.trim()),
    )
    .await?
    .ok_or(AppError::Validation("error.invalid_token"))?;
    users::mark_verified(&s.pool, uid).await?;
    audit::record(
        &s.pool,
        Some(uid),
        "auth.email_verified",
        None,
        g.ip(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new("user.email_verified", json!({ "user_id": uid })));
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/auth/email/resend", responses((status = 204), (status = 401), (status = 429)))]
pub async fn resend(
    State(s): State<AppState>,
    _g: AuthGuard,
    u: CurrentUser,
) -> ApiResult<StatusCode> {
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    if user.email_verified_at.is_none() {
        send_verification(&s, &user).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
