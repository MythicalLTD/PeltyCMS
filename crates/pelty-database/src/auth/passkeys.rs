//! Passkey storage and single-use WebAuthn ceremony state. The crypto lives
//! in pelty-api (webauthn-rs); this module stores opaque JSON.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const MAX_PER_USER: i64 = 10;

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct PasskeyRow {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub last_used_at: Option<OffsetDateTime>,
}

pub async fn list(pool: &PgPool, user: Uuid) -> AppResult<Vec<PasskeyRow>> {
    sqlx::query_as("SELECT id, name, created_at, last_used_at FROM passkeys WHERE user_id = $1 ORDER BY created_at")
        .bind(user)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn count(pool: &PgPool, user: Uuid) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM passkeys WHERE user_id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// (row id, credential JSON) for a user's passkeys.
pub async fn credentials(pool: &PgPool, user: Uuid) -> AppResult<Vec<(Uuid, Value)>> {
    sqlx::query_as("SELECT id, credential FROM passkeys WHERE user_id = $1")
        .bind(user)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn insert(
    pool: &PgPool,
    user: Uuid,
    name: &str,
    cred_id: &str,
    credential: &Value,
) -> AppResult<Uuid> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::Validation("error.passkey_name"));
    }
    if count(pool, user).await? >= MAX_PER_USER {
        return Err(AppError::Conflict("error.passkey_limit"));
    }
    let id = Uuid::new_v4();
    let r = sqlx::query("INSERT INTO passkeys (id, user_id, name, cred_id, credential) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (cred_id) DO NOTHING")
        .bind(id)
        .bind(user)
        .bind(name)
        .bind(cred_id)
        .bind(credential)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.passkey_exists"));
    }
    Ok(id)
}

/// Store the updated credential (sign counter / backup state) after use.
pub async fn touch(pool: &PgPool, id: Uuid, credential: Option<&Value>) -> AppResult<()> {
    sqlx::query("UPDATE passkeys SET last_used_at = now(), credential = COALESCE($2, credential) WHERE id = $1")
        .bind(id)
        .bind(credential)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn delete(pool: &PgPool, user: Uuid, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM passkeys WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_all(pool: &PgPool, user: Uuid) -> AppResult<u64> {
    Ok(sqlx::query("DELETE FROM passkeys WHERE user_id = $1")
        .bind(user)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected())
}

pub async fn put_challenge(
    pool: &PgPool,
    user: Option<Uuid>,
    kind: &str,
    state: &Value,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("DELETE FROM webauthn_challenges WHERE expires_at < now()")
        .execute(pool)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO webauthn_challenges (id, user_id, kind, state, expires_at) VALUES ($1,$2,$3,$4, now() + interval '5 minutes')")
        .bind(id)
        .bind(user)
        .bind(kind)
        .bind(state)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(id)
}

/// Consume a challenge (single use). `None` when unknown, expired or of
/// another kind.
pub async fn take_challenge(
    pool: &PgPool,
    id: Uuid,
    kind: &str,
) -> AppResult<Option<(Option<Uuid>, Value)>> {
    sqlx::query_as("DELETE FROM webauthn_challenges WHERE id = $1 AND kind = $2 AND expires_at > now() RETURNING user_id, state")
        .bind(id)
        .bind(kind)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Record that a passkey proved usable without an e-mail (resident key).
pub async fn mark_discoverable(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE passkeys SET discoverable = true WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
