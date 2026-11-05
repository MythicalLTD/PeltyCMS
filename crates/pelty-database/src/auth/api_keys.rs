//! Personal API keys (hashed, scoped, revocable, optional expiry).
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    /// Addresses / CIDR ranges the key may be used from (empty = any).
    pub allowed_ips: Vec<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    pub last_used_ip: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, name, prefix, scopes, allowed_ips, expires_at, last_used_at, last_used_ip, revoked_at, created_at";

pub async fn list(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<ApiKey>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLS} FROM api_keys WHERE user_id=$1 ORDER BY revoked_at IS NOT NULL, created_at DESC")))
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn count_active(pool: &PgPool, user_id: Uuid) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM api_keys WHERE user_id=$1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    prefix: &str,
    hash: &[u8],
    scopes: &[String],
    allowed_ips: &[String],
    expires_at: Option<OffsetDateTime>,
) -> AppResult<ApiKey> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO api_keys (user_id, name, prefix, key_hash, scopes, allowed_ips, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {COLS}"
    )))
    .bind(user_id)
    .bind(name)
    .bind(prefix)
    .bind(hash)
    .bind(scopes)
    .bind(allowed_ips)
    .bind(expires_at)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Change a live key's IP allowlist and/or expiry. `user_id = None` lets
/// staff edit any key. `expires_at`: `None` keeps it, `Some(None)` clears it.
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    user_id: Option<Uuid>,
    allowed_ips: Option<&[String]>,
    expires_at: Option<Option<OffsetDateTime>>,
) -> AppResult<Option<ApiKey>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE api_keys SET allowed_ips = COALESCE($3, allowed_ips),
                             expires_at = CASE WHEN $4 THEN $5 ELSE expires_at END
         WHERE id = $1 AND ($2::uuid IS NULL OR user_id = $2) AND revoked_at IS NULL
         RETURNING {COLS}"
    )))
    .bind(id)
    .bind(user_id)
    .bind(allowed_ips)
    .bind(expires_at.is_some())
    .bind(expires_at.flatten())
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Revoke one of the user's keys. Admins may revoke any key (`any_user`).
pub async fn revoke(pool: &PgPool, id: Uuid, user_id: Uuid, any_user: bool) -> AppResult<bool> {
    let r = sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id=$1 AND (user_id=$2 OR $3) AND revoked_at IS NULL")
        .bind(id)
        .bind(user_id)
        .bind(any_user)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn revoke_all(pool: &PgPool, user_id: Uuid) -> AppResult<u64> {
    let r = sqlx::query(
        "UPDATE api_keys SET revoked_at = now() WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// A resolved key: owner roles and *all* owner permissions plus the key's
/// scopes. The caller expands implied read levels and intersects.
#[derive(Debug, Clone, FromRow)]
pub struct KeyContext {
    pub key_id: Uuid,
    pub user_id: Uuid,
    pub roles: Vec<String>,
    pub owner_permissions: Vec<String>,
    pub scopes: Vec<String>,
    /// False when the key has an allowlist and the caller IP is not on it
    /// (then `last_used_*` is not touched).
    pub ip_allowed: bool,
}

/// Resolve a key hash: active owner, key not revoked or expired. Touches
/// `last_used_*` at most once a minute.
pub async fn resolve(
    pool: &PgPool,
    hash: &[u8],
    ip: Option<&str>,
) -> AppResult<Option<KeyContext>> {
    sqlx::query_as(
        "WITH k AS (
             SELECT id, user_id, scopes,
                    (cardinality(allowed_ips) = 0 OR ($2::text IS NOT NULL AND EXISTS (
                        SELECT 1 FROM unnest(allowed_ips) a WHERE $2::text::inet <<= a::inet))) AS ip_allowed
             FROM api_keys
             WHERE key_hash = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())
         ), touched AS (
             UPDATE api_keys SET last_used_at = now(), last_used_ip = $2
             WHERE id = (SELECT id FROM k WHERE ip_allowed) AND (last_used_at IS NULL OR last_used_at < now() - interval '60 seconds')
             RETURNING id
         )
         SELECT k.id AS key_id, k.user_id, k.scopes, k.ip_allowed,
                ARRAY(SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = k.user_id ORDER BY r.name) AS roles,
                ARRAY(SELECT DISTINCT p.name FROM permissions p
                      JOIN role_permissions rp ON rp.permission_id = p.id
                      JOIN user_roles ur ON ur.role_id = rp.role_id
                      WHERE ur.user_id = k.user_id ORDER BY p.name) AS owner_permissions
         FROM k JOIN users u ON u.id = k.user_id AND u.is_active",
    )
    .bind(hash)
    .bind(ip)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Admin view: a key with its owner.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct OwnedKey {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub key: ApiKey,
    pub user_id: Uuid,
    pub user_email: String,
}

/// All keys (newest first), optionally only active ones or one owner's.
pub async fn list_all(
    pool: &PgPool,
    active_only: bool,
    user_id: Option<Uuid>,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<OwnedKey>, i64)> {
    let filter = "($1 = false OR (k.revoked_at IS NULL AND (k.expires_at IS NULL OR k.expires_at > now())))
         AND ($2::uuid IS NULL OR k.user_id = $2)
         AND ($3::text IS NULL OR u.email ILIKE '%' || $3 || '%' OR k.name ILIKE '%' || $3 || '%' OR k.prefix = $3)";
    let cols = COLS
        .split(", ")
        .map(|c| format!("k.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let items = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {cols}, k.user_id, u.email AS user_email FROM api_keys k JOIN users u ON u.id = k.user_id
         WHERE {filter} ORDER BY k.created_at DESC, k.id LIMIT $4 OFFSET $5"
    )))
    .bind(active_only)
    .bind(user_id)
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM api_keys k JOIN users u ON u.id = k.user_id WHERE {filter}"
    )))
    .bind(active_only)
    .bind(user_id)
    .bind(q)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((items, total))
}

/// Owner of a key, if it exists.
pub async fn owner(pool: &PgPool, id: Uuid) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT user_id FROM api_keys WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}
