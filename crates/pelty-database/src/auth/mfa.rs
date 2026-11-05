//! Storage for TOTP 2FA: sealed secrets, hashed backup codes, login challenges.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use uuid::Uuid;

pub const MAX_ATTEMPTS: i32 = 5;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Mfa {
    pub user_id: Uuid,
    pub secret_enc: Vec<u8>,
    pub enabled: bool,
    pub last_step: Option<i64>,
}

pub async fn get(pool: &PgPool, user_id: Uuid) -> AppResult<Option<Mfa>> {
    sqlx::query_as("SELECT user_id, secret_enc, enabled_at IS NOT NULL AS enabled, last_step FROM user_mfa WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn is_enabled(pool: &PgPool, user_id: Uuid) -> AppResult<bool> {
    Ok(get(pool, user_id).await?.is_some_and(|m| m.enabled))
}

/// Store a pending (not yet enabled) secret. Never replaces an enabled one.
pub async fn set_pending(pool: &PgPool, user_id: Uuid, secret_enc: &[u8]) -> AppResult<bool> {
    let r = sqlx::query(
        "INSERT INTO user_mfa (user_id, secret_enc) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET secret_enc = EXCLUDED.secret_enc, last_step = NULL, created_at = now()
         WHERE user_mfa.enabled_at IS NULL",
    )
    .bind(user_id)
    .bind(secret_enc)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Record a used time step; false if it was not newer (replay / race).
pub async fn use_step(pool: &PgPool, user_id: Uuid, step: i64) -> AppResult<bool> {
    let r = sqlx::query("UPDATE user_mfa SET last_step = $2 WHERE user_id = $1 AND (last_step IS NULL OR last_step < $2)")
        .bind(user_id)
        .bind(step)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Enable 2FA and replace backup codes atomically.
pub async fn enable(
    pool: &PgPool,
    user_id: Uuid,
    step: i64,
    codes: &[[u8; 32]],
) -> AppResult<bool> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("UPDATE user_mfa SET enabled_at = now(), last_step = $2 WHERE user_id = $1 AND enabled_at IS NULL")
        .bind(user_id)
        .bind(step)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    if r.rows_affected() != 1 {
        return Ok(false);
    }
    replace_codes_tx(&mut tx, user_id, codes).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(true)
}

async fn replace_codes_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    codes: &[[u8; 32]],
) -> AppResult<()> {
    sqlx::query("DELETE FROM user_backup_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    for c in codes {
        sqlx::query("INSERT INTO user_backup_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(&c[..])
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

pub async fn replace_codes(pool: &PgPool, user_id: Uuid, codes: &[[u8; 32]]) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    replace_codes_tx(&mut tx, user_id, codes).await?;
    tx.commit().await.map_err(db_err)
}

pub async fn disable(pool: &PgPool, user_id: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM user_backup_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("DELETE FROM user_mfa WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("DELETE FROM mfa_challenges WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Consume a backup code (single use).
pub async fn use_backup(pool: &PgPool, user_id: Uuid, hash: &[u8]) -> AppResult<bool> {
    let r = sqlx::query("UPDATE user_backup_codes SET used_at = now() WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL")
        .bind(user_id)
        .bind(hash)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn backup_remaining(pool: &PgPool, user_id: Uuid) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM user_backup_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn create_challenge(
    pool: &PgPool,
    user_id: Uuid,
    hash: &[u8],
    ttl_secs: i64,
) -> AppResult<()> {
    sqlx::query("INSERT INTO mfa_challenges (token_hash, user_id, expires_at) VALUES ($1, $2, now() + make_interval(secs => $3))")
        .bind(hash)
        .bind(user_id)
        .bind(ttl_secs as f64)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Count an attempt against a live challenge; returns its user if attempts remain.
pub async fn attempt_challenge(pool: &PgPool, hash: &[u8]) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "UPDATE mfa_challenges SET attempts = attempts + 1
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() AND attempts < $2
         RETURNING user_id",
    )
    .bind(hash)
    .bind(MAX_ATTEMPTS)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn finish_challenge(pool: &PgPool, hash: &[u8]) -> AppResult<bool> {
    let r = sqlx::query(
        "UPDATE mfa_challenges SET used_at = now() WHERE token_hash = $1 AND used_at IS NULL",
    )
    .bind(hash)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Opted in to passkeys as second factor *and* still has a passkey.
pub async fn passkey_2fa(pool: &PgPool, user_id: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_passkey_2fa f WHERE f.user_id = $1 AND EXISTS(SELECT 1 FROM passkeys p WHERE p.user_id = $1))")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn set_passkey_2fa(pool: &PgPool, user_id: Uuid, on: bool) -> AppResult<()> {
    let q = if on {
        "INSERT INTO user_passkey_2fa (user_id) VALUES ($1) ON CONFLICT DO NOTHING"
    } else {
        "DELETE FROM user_passkey_2fa WHERE user_id = $1"
    };
    sqlx::query(q).bind(user_id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// Second-factor methods a password login must complete with one of.
/// Empty when no second factor is configured. A registered passkey is
/// accepted alongside TOTP; on its own only after opting in.
pub async fn login_methods(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<&'static str>> {
    let (totp, opted, keys): (bool, bool, bool) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM user_mfa WHERE user_id = $1 AND enabled_at IS NOT NULL), \
         EXISTS(SELECT 1 FROM user_passkey_2fa WHERE user_id = $1), \
         EXISTS(SELECT 1 FROM passkeys WHERE user_id = $1)",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let mut m = vec![];
    if totp {
        m.push("totp");
    }
    if keys && (totp || opted) {
        m.push("passkey");
    }
    Ok(m)
}
