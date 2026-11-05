//! User repository: accounts for clients and staff.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip)]
    pub password_hash: String,
    pub display_name: String,
    pub locale: Option<String>,
    pub is_active: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub email_verified_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Bumped on every avatar change; `None` = no picture (use initials).
    pub avatar_version: Option<i64>,
}

const COLS: &str = "id, email::text AS email, password_hash, display_name, locale, is_active, email_verified_at, created_at, avatar_version";

pub async fn create(
    pool: &PgPool,
    email: &str,
    hash: &str,
    name: &str,
    role: &str,
) -> AppResult<User> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let user: User = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO users (id, email, password_hash, display_name) VALUES ($1,$2,$3,$4) RETURNING {COLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(email)
    .bind(hash)
    .bind(name)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) SELECT $1, id FROM roles WHERE name = $2",
    )
    .bind(user.id)
    .bind(role)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(user)
}

pub async fn find_by_email(pool: &PgPool, email: &str) -> AppResult<Option<User>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM users WHERE email = $1"
    )))
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn find(pool: &PgPool, id: Uuid) -> AppResult<Option<User>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM users WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn list(pool: &PgPool, limit: i64, offset: i64) -> AppResult<(Vec<User>, i64)> {
    // One round trip: page + total via a window function. If the page is past
    // the end, fall back to a plain count so `total` stays correct.
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        user: User,
        total: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS}, count(*) OVER() AS total FROM users ORDER BY created_at DESC, id LIMIT $1 OFFSET $2"
    )))
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = match rows.first() {
        Some(r) => r.total,
        None => sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(pool)
            .await
            .map_err(db_err)?,
    };
    Ok((rows.into_iter().map(|r| r.user).collect(), total))
}

pub async fn set_locale(pool: &PgPool, id: Uuid, locale: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE users SET locale = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(locale)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn roles(pool: &PgPool, id: Uuid) -> AppResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = $1 ORDER BY r.name",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn permissions(pool: &PgPool, id: Uuid) -> AppResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT p.name FROM permissions p
         JOIN role_permissions rp ON rp.permission_id = p.id
         JOIN user_roles ur ON ur.role_id = rp.role_id
         WHERE ur.user_id = $1 ORDER BY p.name",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_password(pool: &PgPool, id: Uuid, hash: &str) -> AppResult<()> {
    sqlx::query("UPDATE users SET password_hash = $2, password_changed_at = now() WHERE id = $1")
        .bind(id)
        .bind(hash)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn mark_verified(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE users SET email_verified_at = COALESCE(email_verified_at, now()) WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Consecutive failures before an account gets locked.
pub const LOCK_AFTER: i32 = 5;

/// True while the account is in a lockout window.
pub async fn is_locked(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT coalesce(locked_until > now(), false) FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map(|v: Option<bool>| v.unwrap_or(false))
        .map_err(db_err)
}

/// Record a failed login. From the LOCK_AFTER-th failure on, lock for
/// 2^(n-LOCK_AFTER) minutes (max 60). Returns (failures, locked_minutes).
pub async fn login_failed(pool: &PgPool, id: Uuid) -> AppResult<(i32, Option<i32>)> {
    let (n, mins): (i32, Option<i32>) = sqlx::query_as(
        "UPDATE users SET failed_logins = failed_logins + 1,
            locked_until = CASE WHEN failed_logins + 1 >= $2
                THEN now() + make_interval(mins => least(60, power(2, failed_logins + 1 - $2)::int)) ELSE locked_until END
         WHERE id = $1
         RETURNING failed_logins, CASE WHEN failed_logins >= $2 THEN least(60, power(2, failed_logins - $2)::int) END",
    )
    .bind(id)
    .bind(LOCK_AFTER)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((n, mins))
}

/// Successful login: clear the failure counter and any lock.
pub async fn login_succeeded(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1 AND (failed_logins <> 0 OR locked_until IS NOT NULL)")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Emails of active admins (for security notifications).
pub async fn admin_emails(pool: &PgPool) -> AppResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT u.email::text FROM users u JOIN user_roles ur ON ur.user_id = u.id JOIN roles r ON r.id = ur.role_id
         WHERE r.name = 'admin' AND u.is_active",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// When the password was last set (password expiry policy).
pub async fn password_changed_at(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<Option<time::OffsetDateTime>> {
    sqlx::query_scalar("SELECT password_changed_at FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}
