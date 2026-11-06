//! One-time user tokens (password reset, email verification). Only the
//! SHA-256 of the token is stored; consuming is atomic and single-use.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use time::Duration;
use uuid::Uuid;

pub const PASSWORD_RESET: &str = "password_reset";
pub const EMAIL_VERIFY: &str = "email_verify";
/// One-click sign-in after `pelty-server setup` (short TTL, single use).
pub const SETUP_LOGIN: &str = "setup_login";

/// Store a new token, invalidating earlier unused ones of the same purpose.
pub async fn issue(
    pool: &PgPool,
    user_id: Uuid,
    purpose: &str,
    hash: &[u8],
    ttl: Duration,
) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("UPDATE user_tokens SET used_at = now() WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL")
        .bind(user_id)
        .bind(purpose)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO user_tokens (token_hash, user_id, purpose, expires_at) VALUES ($1,$2,$3, now() + make_interval(secs => $4))")
        .bind(hash)
        .bind(user_id)
        .bind(purpose)
        .bind(ttl.whole_seconds() as f64)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Mark a valid token used and return its user. `None` if unknown,
/// expired, already used or for another purpose.
pub async fn consume(pool: &PgPool, purpose: &str, hash: &[u8]) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "UPDATE user_tokens SET used_at = now()
         WHERE token_hash = $1 AND purpose = $2 AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
    )
    .bind(hash)
    .bind(purpose)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Look up (user id, email) for a valid token without consuming it.
pub async fn peek(pool: &PgPool, purpose: &str, hash: &[u8]) -> AppResult<Option<(Uuid, String)>> {
    sqlx::query_as(
        "SELECT t.user_id, u.email::text FROM user_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1 AND t.purpose = $2 AND t.used_at IS NULL AND t.expires_at > now() AND u.is_active",
    )
    .bind(hash)
    .bind(purpose)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}
