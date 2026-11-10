//! Discord roles (module `discord_roles`): product → role rules and the
//! add/remove queue. HTTP calls live in the API crate.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "discord_roles";
/// Attempts before a job is marked failed.
pub const MAX_ATTEMPTS: i32 = 8;

/// Discord ids (guild, role, user) are 17–20 digit snowflakes.
pub fn is_snowflake(s: &str) -> bool {
    (17..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

pub async fn config(pool: &PgPool) -> AppResult<(String, Option<Vec<u8>>)> {
    let row: Option<(String, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT guild_id, bot_token_enc FROM discord_role_config WHERE id")
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(row.unwrap_or_default())
}

/// Save the guild id and, when given, the sealed token (`Some(None)` clears it).
pub async fn save_config(
    pool: &PgPool,
    guild_id: &str,
    token: Option<Option<Vec<u8>>>,
) -> AppResult<()> {
    let g = guild_id.trim();
    if !(g.is_empty() || is_snowflake(g)) {
        return Err(AppError::Validation("error.discord_id_invalid"));
    }
    sqlx::query(
        "INSERT INTO discord_role_config (id, guild_id, bot_token_enc) VALUES (true, $1, $3)
         ON CONFLICT (id) DO UPDATE SET guild_id = EXCLUDED.guild_id,
             bot_token_enc = CASE WHEN $2 THEN EXCLUDED.bot_token_enc ELSE discord_role_config.bot_token_enc END,
             updated_at = now()",
    )
    .bind(g)
    .bind(token.is_some())
    .bind(token.flatten())
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DiscordRoleRule {
    pub product_id: Uuid,
    pub product_name: String,
    pub role_ids: Vec<String>,
    pub remove_on_expiry: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscordRoleRuleInput {
    pub role_ids: Vec<String>,
    #[serde(default = "yes")]
    pub remove_on_expiry: bool,
}

fn yes() -> bool {
    true
}

pub async fn rules(pool: &PgPool) -> AppResult<Vec<DiscordRoleRule>> {
    sqlx::query_as(
        "SELECT r.product_id, p.name AS product_name, r.role_ids, r.remove_on_expiry
           FROM discord_role_rules r JOIN products p ON p.id = r.product_id ORDER BY p.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_rule(pool: &PgPool, product: Uuid, i: &DiscordRoleRuleInput) -> AppResult<()> {
    let mut roles: Vec<String> = i.role_ids.iter().map(|r| r.trim().to_string()).collect();
    roles.sort();
    roles.dedup();
    if roles.is_empty() || roles.len() > 10 || !roles.iter().all(|r| is_snowflake(r)) {
        return Err(AppError::Validation("error.discord_id_invalid"));
    }
    let r = sqlx::query(
        "INSERT INTO discord_role_rules (product_id, role_ids, remove_on_expiry) VALUES ($1, $2, $3)
         ON CONFLICT (product_id) DO UPDATE SET role_ids = EXCLUDED.role_ids,
             remove_on_expiry = EXCLUDED.remove_on_expiry, updated_at = now()",
    )
    .bind(product)
    .bind(&roles)
    .bind(i.remove_on_expiry)
    .execute(pool)
    .await;
    match r {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.is_foreign_key_violation() => Err(AppError::NotFound),
        Err(e) => Err(db_err(e)),
    }
}

pub async fn delete_rule(pool: &PgPool, product: Uuid) -> AppResult<bool> {
    let n = sqlx::query("DELETE FROM discord_role_rules WHERE product_id = $1")
        .bind(product)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    Ok(n > 0)
}

/// Queue role changes for a service's product rules. `expiry` honours the
/// rule's `remove_on_expiry`. Returns how many jobs were added.
pub async fn enqueue(
    pool: &PgPool,
    service: Uuid,
    action: &str,
    occurrence: &str,
    expiry: bool,
) -> AppResult<u64> {
    let n = sqlx::query(
        "INSERT INTO discord_role_jobs (service_id, user_id, role_id, action, occurrence)
         SELECT s.id, c.owner_user_id, role, $2, $3
           FROM services s
           JOIN customers c ON c.id = s.customer_id AND c.owner_user_id IS NOT NULL
           JOIN discord_role_rules r ON r.product_id = s.product_id
           CROSS JOIN LATERAL unnest(r.role_ids) AS role
          WHERE s.id = $1 AND (NOT $4 OR r.remove_on_expiry)
         ON CONFLICT (service_id, role_id, action, occurrence) DO NOTHING",
    )
    .bind(service)
    .bind(action)
    .bind(occurrence)
    .bind(expiry)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(n)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Claimed {
    pub id: Uuid,
    pub service_id: Option<Uuid>,
    pub user_id: Uuid,
    pub role_id: String,
    pub action: String,
    pub attempts: i32,
    /// Linked Discord account, if any.
    pub discord_user: Option<String>,
}

/// Lease up to `limit` due jobs (pushes `next_at` out so a crash retries).
pub async fn claim(pool: &PgPool, limit: i64) -> AppResult<Vec<Claimed>> {
    sqlx::query_as(
        "WITH due AS (
            SELECT id FROM discord_role_jobs
             WHERE status IN ('pending','waiting') AND next_at <= now()
               AND (status = 'pending' OR EXISTS (
                    SELECT 1 FROM user_identities i WHERE i.user_id = discord_role_jobs.user_id AND i.provider = 'discord'))
             ORDER BY next_at LIMIT $1 FOR UPDATE SKIP LOCKED)
         UPDATE discord_role_jobs j SET attempts = j.attempts + 1, status = 'pending',
                next_at = now() + interval '10 minutes'
           FROM due WHERE j.id = due.id
         RETURNING j.id, j.service_id, j.user_id, j.role_id, j.action, j.attempts,
                   (SELECT i.subject FROM user_identities i WHERE i.user_id = j.user_id AND i.provider = 'discord') AS discord_user",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Another active service of this user still grants `role` (keep it).
pub async fn still_granted(pool: &PgPool, j: &Claimed) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM services s
              JOIN customers c ON c.id = s.customer_id
              JOIN discord_role_rules r ON r.product_id = s.product_id
             WHERE c.owner_user_id = $1 AND s.status = 'active'
               AND s.id IS DISTINCT FROM $2 AND $3 = ANY(r.role_ids))",
    )
    .bind(j.user_id)
    .bind(j.service_id)
    .bind(&j.role_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Record an outcome: `done`, `skipped`, `failed`, `waiting`, or `pending`
/// with a retry delay. Too many attempts turn a retry into `failed`.
pub async fn finish(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    note: &str,
    retry_secs: i64,
) -> AppResult<()> {
    let note: String = note.chars().take(500).collect();
    sqlx::query(
        "UPDATE discord_role_jobs SET
            status = CASE WHEN $2 = 'pending' AND attempts >= $5 THEN 'failed' ELSE $2 END,
            last_error = $3,
            next_at = now() + make_interval(secs => $4),
            done_at = CASE WHEN $2 IN ('done','skipped','failed') OR ($2 = 'pending' AND attempts >= $5) THEN now() END
          WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(note)
    .bind(retry_secs as f64)
    .bind(MAX_ATTEMPTS)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DiscordRoleJob {
    pub id: Uuid,
    pub service_id: Option<Uuid>,
    pub email: String,
    pub product_name: Option<String>,
    pub role_id: String,
    pub action: String,
    pub status: String,
    pub attempts: i32,
    pub last_error: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub done_at: Option<OffsetDateTime>,
}

pub async fn jobs(pool: &PgPool, limit: i64) -> AppResult<Vec<DiscordRoleJob>> {
    sqlx::query_as(
        "SELECT j.id, j.service_id, u.email, p.name AS product_name, j.role_id, j.action, j.status,
                j.attempts, j.last_error, j.created_at, j.done_at
           FROM discord_role_jobs j
           JOIN users u ON u.id = j.user_id
           LEFT JOIN services s ON s.id = j.service_id
           LEFT JOIN products p ON p.id = s.product_id
          ORDER BY j.created_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Retry a failed, skipped or waiting job now.
pub async fn retry(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query(
        "UPDATE discord_role_jobs SET status = 'pending', attempts = 0, next_at = now(), done_at = NULL, last_error = ''
          WHERE id = $1 AND status IN ('failed','skipped','waiting')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("error.discord_job_not_retryable"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_snowflake;

    #[test]
    fn snowflakes() {
        assert!(is_snowflake("123456789012345678"));
        assert!(!is_snowflake("1234"));
        assert!(!is_snowflake("12345678901234567a"));
        assert!(!is_snowflake("123456789012345678901"));
    }
}
