//! TOTP two-factor authentication: enrolment, login challenge, backup codes.
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::routes::auth::{session_response, AuthGuard};
use crate::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_auth::{password, token, totp};
use pelty_core::AppError;
use pelty_database::{audit, mfa, users};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

const CHALLENGE_TTL_SECS: i64 = 300;
const BACKUP_CODES: usize = 10;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/mfa", get(status))
        .route("/auth/mfa/verify", post(verify))
        .route("/auth/mfa/setup", post(setup))
        .route("/auth/mfa/enable", post(enable))
        .route("/auth/mfa/disable", post(disable))
        .route("/auth/mfa/backup-codes", post(regenerate))
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn sbox(s: &AppState) -> ApiResult<&pelty_auth::crypto::SecretBox> {
    s.secret_box
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("error.mfa_unavailable").into())
}

pub(crate) async fn start_challenge(s: &AppState, user_id: Uuid) -> ApiResult<String> {
    let tok = token::generate();
    mfa::create_challenge(&s.pool, user_id, &token::hash(&tok), CHALLENGE_TTL_SECS).await?;
    Ok(tok)
}

fn new_codes() -> (Vec<String>, Vec<[u8; 32]>) {
    let plain: Vec<String> = (0..BACKUP_CODES).map(|_| totp::backup_code()).collect();
    let hashes = plain
        .iter()
        .map(|c| token::hash(&totp::normalize_backup(c)))
        .collect();
    (plain, hashes)
}

/// Check a TOTP code (with replay protection) or a backup code.
/// Returns Some("totp"|"backup") on success.
pub(crate) async fn check_code(
    s: &AppState,
    user_id: Uuid,
    code: &str,
) -> ApiResult<Option<&'static str>> {
    let Some(m) = mfa::get(&s.pool, user_id).await?.filter(|m| m.enabled) else {
        return Ok(None);
    };
    let trimmed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        let secret = sbox(s)?
            .open(&m.secret_enc)
            .ok_or_else(|| AppError::Internal("mfa secret undecryptable".into()))?;
        if let Some(step) = totp::verify(&secret, &trimmed, now(), m.last_step) {
            if mfa::use_step(&s.pool, user_id, step).await? {
                return Ok(Some("totp"));
            }
        }
        return Ok(None);
    }
    let norm = totp::normalize_backup(code);
    if norm.len() == 10 && mfa::use_backup(&s.pool, user_id, &token::hash(&norm)).await? {
        return Ok(Some("backup"));
    }
    Ok(None)
}

#[derive(Serialize, ToSchema)]
pub struct MfaStatus {
    pub enabled: bool,
    pub backup_codes_remaining: i64,
    /// Whether the server can enrol new devices (encryption key configured).
    pub available: bool,
}

#[utoipa::path(get, path = "/api/v1/auth/mfa", responses((status = 200, body = MfaStatus), (status = 401)))]
pub async fn status(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<MfaStatus>> {
    Ok(Json(MfaStatus {
        enabled: mfa::is_enabled(&s.pool, u.user_id).await?,
        backup_codes_remaining: mfa::backup_remaining(&s.pool, u.user_id).await?,
        available: s.secret_box.is_some(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct VerifyBody {
    pub challenge: String,
    pub code: String,
}

/// Second login step. Max 5 attempts per challenge, 5 minute lifetime.
#[utoipa::path(post, path = "/api/v1/auth/mfa/verify", request_body = VerifyBody,
    responses((status = 200, body = crate::routes::auth::MeResponse), (status = 401), (status = 429)))]
pub async fn verify(
    State(s): State<AppState>,
    g: AuthGuard,
    headers: HeaderMap,
    Json(b): Json<VerifyBody>,
) -> ApiResult<Response> {
    let hash = token::hash(&b.challenge);
    let Some(user_id) = mfa::attempt_challenge(&s.pool, &hash).await? else {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    };
    // Duo replaces TOTP for the users it covers; no downgrade via this route.
    if crate::duo::applies(&s, user_id).await? {
        return Err(AppError::Validation("error.duo_required").into());
    }
    let Some(method) = check_code(&s, user_id, &b.code).await? else {
        audit::record(
            &s.pool,
            Some(user_id),
            "auth.mfa_failed",
            None,
            g.ip(),
            json!({}),
        )
        .await?;
        return Err(AppError::Validation("error.mfa_invalid_code").into());
    };
    if !mfa::finish_challenge(&s.pool, &hash).await? {
        return Err(AppError::Validation("error.mfa_challenge_expired").into());
    }
    let user = users::find(&s.pool, user_id)
        .await?
        .filter(|u| u.is_active)
        .ok_or(AppError::InvalidCredentials)?;
    audit::record(
        &s.pool,
        Some(user_id),
        "auth.mfa_verified",
        None,
        g.ip(),
        json!({ "method": method }),
    )
    .await?;
    session_response(&s, &g, &headers, user).await
}

#[derive(Serialize, ToSchema)]
pub struct SetupResponse {
    /// Base32 seed, shown once to the enrolling user only.
    pub secret: String,
    pub otpauth_url: String,
}

/// Start enrolment: a fresh pending secret, replaced on each call until enabled.
#[utoipa::path(post, path = "/api/v1/auth/mfa/setup", responses((status = 200, body = SetupResponse), (status = 400), (status = 409)))]
pub async fn setup(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<SetupResponse>> {
    let sb = sbox(&s)?;
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    let secret = totp::new_secret();
    if !mfa::set_pending(&s.pool, u.user_id, &sb.seal(&secret)).await? {
        return Err(AppError::Conflict("error.mfa_already_enabled").into());
    }
    let issuer = s.i18n.t(&s.cfg.default_locale, "app.name").to_string();
    Ok(Json(SetupResponse {
        secret: totp::base32(&secret),
        otpauth_url: totp::otpauth_url(&issuer, &user.email, &secret),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct CodeBody {
    pub code: String,
}

#[derive(Serialize, ToSchema)]
pub struct BackupCodes {
    /// Plain codes, returned exactly once.
    pub backup_codes: Vec<String>,
}

#[utoipa::path(post, path = "/api/v1/auth/mfa/enable", request_body = CodeBody,
    responses((status = 200, body = BackupCodes), (status = 409), (status = 422)))]
pub async fn enable(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<CodeBody>,
) -> ApiResult<Json<BackupCodes>> {
    let sb = sbox(&s)?;
    let m = mfa::get(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Validation("error.mfa_not_started"))?;
    if m.enabled {
        return Err(AppError::Conflict("error.mfa_already_enabled").into());
    }
    let secret = sb
        .open(&m.secret_enc)
        .ok_or_else(|| AppError::Internal("mfa secret undecryptable".into()))?;
    let step = totp::verify(&secret, &b.code, now(), None)
        .ok_or(AppError::Validation("error.mfa_invalid_code"))?;
    let (plain, hashes) = new_codes();
    if !mfa::enable(&s.pool, u.user_id, step, &hashes).await? {
        return Err(AppError::Conflict("error.mfa_already_enabled").into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.mfa_enabled",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.mfa_enabled",
        json!({ "user_id": u.user_id }),
    ));
    Ok(Json(BackupCodes {
        backup_codes: plain,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct ConfirmBody {
    pub password: String,
    pub code: String,
}

async fn confirm(s: &AppState, u: &CurrentUser, b: &ConfirmBody) -> ApiResult<()> {
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    if !password::verify(&b.password, &user.password_hash) {
        return Err(AppError::Validation("error.invalid_password").into());
    }
    if check_code(s, u.user_id, &b.code).await?.is_none() {
        return Err(AppError::Validation("error.mfa_invalid_code").into());
    }
    Ok(())
}

/// Turn 2FA off; needs the password and a current code (or a backup code).
#[utoipa::path(post, path = "/api/v1/auth/mfa/disable", request_body = ConfirmBody, responses((status = 204), (status = 422)))]
pub async fn disable(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<ConfirmBody>,
) -> ApiResult<StatusCode> {
    if !mfa::is_enabled(&s.pool, u.user_id).await? {
        return Err(AppError::Validation("error.mfa_not_enabled").into());
    }
    confirm(&s, &u, &b).await?;
    mfa::disable(&s.pool, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.mfa_disabled",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "user.mfa_disabled",
        json!({ "user_id": u.user_id }),
    ));
    Ok(StatusCode::NO_CONTENT)
}

/// Replace all backup codes; returned once.
#[utoipa::path(post, path = "/api/v1/auth/mfa/backup-codes", request_body = ConfirmBody, responses((status = 200, body = BackupCodes), (status = 422)))]
pub async fn regenerate(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<ConfirmBody>,
) -> ApiResult<Json<BackupCodes>> {
    if !mfa::is_enabled(&s.pool, u.user_id).await? {
        return Err(AppError::Validation("error.mfa_not_enabled").into());
    }
    confirm(&s, &u, &b).await?;
    let (plain, hashes) = new_codes();
    mfa::replace_codes(&s.pool, u.user_id, &hashes).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "auth.mfa_backup_regenerated",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(BackupCodes {
        backup_codes: plain,
    }))
}
