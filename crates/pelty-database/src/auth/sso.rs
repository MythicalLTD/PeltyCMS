//! One-time SSO nonces (see `pelty_provisioning::sso`).
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub async fn issue(pool: &PgPool, nonce: &str, service: Uuid, user: Uuid, expires: OffsetDateTime) -> AppResult<()> {
    sqlx::query("DELETE FROM sso_tokens WHERE expires_at < now() - interval '1 day'").execute(pool).await.map_err(db_err)?;
    sqlx::query("INSERT INTO sso_tokens (nonce, service_id, user_id, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(nonce).bind(service).bind(user).bind(expires)
        .execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// Burn a nonce for `service`; true only the first time, before expiry.
pub async fn consume(pool: &PgPool, nonce: &str, service: Uuid) -> AppResult<bool> {
    let r = sqlx::query("UPDATE sso_tokens SET used_at = now() WHERE nonce = $1 AND service_id = $2 AND used_at IS NULL AND expires_at >= now()")
        .bind(nonce).bind(service)
        .execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}
