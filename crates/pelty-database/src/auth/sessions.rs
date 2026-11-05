//! Session repository: create, look up and revoke login sessions.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    #[serde(skip)]
    pub csrf_token: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen_at: OffsetDateTime,
}

const COLS: &str = "id, user_id, csrf_token, ip, user_agent, created_at, last_seen_at";

pub struct NewSession<'a> {
    pub user_id: Uuid,
    pub token_hash: &'a [u8],
    pub csrf_token: &'a str,
    pub ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub idle: Duration,
    pub absolute: Duration,
}

pub async fn create(pool: &PgPool, s: NewSession<'_>) -> AppResult<Session> {
    let now = OffsetDateTime::now_utc();
    sqlx::query_as(&format!(
        "INSERT INTO sessions (id, user_id, token_hash, csrf_token, ip, user_agent, idle_expires_at, absolute_expires_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {COLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(s.user_id)
    .bind(s.token_hash)
    .bind(s.csrf_token)
    .bind(s.ip)
    .bind(s.user_agent.map(|u| u.chars().take(300).collect::<String>()))
    .bind(now + s.idle)
    .bind(now + s.absolute)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Look up an active session by token hash and slide its idle expiry.
pub async fn touch(pool: &PgPool, token_hash: &[u8], idle: Duration) -> AppResult<Option<Session>> {
    let now = OffsetDateTime::now_utc();
    sqlx::query_as(&format!(
        "UPDATE sessions SET last_seen_at = $2, idle_expires_at = LEAST($2 + $3::interval, absolute_expires_at)
         WHERE token_hash = $1 AND revoked_at IS NULL AND idle_expires_at > $2 AND absolute_expires_at > $2
         RETURNING {COLS}"
    ))
    .bind(token_hash)
    .bind(now)
    .bind(format!("{} seconds", idle.whole_seconds()))
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn revoke(pool: &PgPool, user_id: Uuid, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn list_active(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<Session>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM sessions WHERE user_id = $1 AND revoked_at IS NULL
         AND idle_expires_at > now() AND absolute_expires_at > now() ORDER BY last_seen_at DESC"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Everything a request needs to authorize, in one round trip.
#[derive(Debug, Clone, FromRow)]
pub struct AuthContext {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub csrf_token: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

/// Resolve a session token hash to the active user with roles and
/// permissions in a single query. The sliding idle expiry is refreshed at most
/// once per `touch_every` (a data-modifying CTE), so steady traffic is
/// read-only instead of writing a row on every request.
pub async fn resolve(
    pool: &PgPool,
    token_hash: &[u8],
    idle: Duration,
    touch_every: Duration,
) -> AppResult<Option<AuthContext>> {
    let now = OffsetDateTime::now_utc();
    sqlx::query_as(
        "WITH s AS (
             SELECT id, user_id, csrf_token, last_seen_at FROM sessions
             WHERE token_hash = $1 AND revoked_at IS NULL AND idle_expires_at > $2 AND absolute_expires_at > $2
         ), touched AS (
             UPDATE sessions SET last_seen_at = $2, idle_expires_at = LEAST($2 + $3::interval, absolute_expires_at)
             WHERE id = (SELECT id FROM s) AND last_seen_at < $2 - $4::interval
             RETURNING id
         )
         SELECT s.id AS session_id, s.user_id, s.csrf_token,
                ARRAY(SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id = r.id
                      WHERE ur.user_id = s.user_id ORDER BY r.name) AS roles,
                ARRAY(SELECT DISTINCT p.name FROM permissions p
                      JOIN role_permissions rp ON rp.permission_id = p.id
                      JOIN user_roles ur ON ur.role_id = rp.role_id
                      WHERE ur.user_id = s.user_id ORDER BY p.name) AS permissions
         FROM s JOIN users u ON u.id = s.user_id AND u.is_active",
    )
    .bind(token_hash)
    .bind(now)
    .bind(format!("{} seconds", idle.whole_seconds()))
    .bind(format!("{} seconds", touch_every.whole_seconds()))
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Revoke every active session of a user (password reset, 2FA changes).
pub async fn revoke_all(pool: &PgPool, user_id: Uuid) -> AppResult<u64> {
    let r = sqlx::query(
        "UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Revoke every active session of a user except `keep` ("sign out others").
pub async fn revoke_others(pool: &PgPool, user_id: Uuid, keep: Uuid) -> AppResult<u64> {
    let r = sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL")
        .bind(user_id)
        .bind(keep)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Last full authentication of a session: the step-up re-auth time or, if
/// none, the login itself.
pub async fn last_auth_at(pool: &PgPool, id: Uuid) -> AppResult<Option<OffsetDateTime>> {
    sqlx::query_scalar(
        "SELECT COALESCE(reauth_at, created_at) FROM sessions WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Record a successful step-up re-authentication.
pub async fn mark_reauth(pool: &PgPool, user_id: Uuid, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("UPDATE sessions SET reauth_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL").bind(id).bind(user_id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Has this user signed in from `ip` before (any session other than `except`)?
pub async fn seen_ip(pool: &PgPool, user_id: Uuid, ip: &str, except: Uuid) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE user_id = $1 AND ip = $2 AND id <> $3)",
    )
    .bind(user_id)
    .bind(ip)
    .bind(except)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}
