//! Retention-based erasure of inactive clients (WHMCS Data Retention
//! "delete inactive clients"). Clients are anonymised with the same
//! routine as a GDPR erasure (invoices stay for tax law), never hard
//! deleted. Safeguards: off by default, at least 365 days, never staff,
//! never anyone with live services/domains, unpaid invoices, open tickets
//! or a pending GDPR request, at most `max_per_run` per run, and every
//! erasure leaves a completed GDPR request as the audit trail.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const KEY: &str = "system.inactive_client_erasure";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    /// Days without any activity (0 when disabled, else 365..=3650).
    pub inactive_days: i32,
    /// Upper bound per run (1..=500).
    pub max_per_run: i32,
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        let days_ok = if self.enabled { (365..=3650).contains(&self.inactive_days) } else { self.inactive_days == 0 || (365..=3650).contains(&self.inactive_days) };
        if days_ok && (self.max_per_run == 0 && !self.enabled || (1..=500).contains(&self.max_per_run)) { Ok(()) } else { Err(AppError::Validation("error.validation")) }
    }
}

pub async fn get(pool: &PgPool) -> AppResult<Settings> {
    Ok(crate::settings::get(pool, KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn put(pool: &PgPool, s: &Settings) -> AppResult<()> {
    s.validate()?;
    crate::settings::set(pool, KEY, serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?).await
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Candidate {
    pub customer_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub last_activity: OffsetDateTime,
}

const CANDIDATES: &str = "
WITH act AS (
  SELECT c.id AS customer_id, c.owner_user_id AS user_id, c.email::text AS email,
    GREATEST(c.created_at, c.updated_at,
      (SELECT max(i.created_at) FROM invoices i WHERE i.customer_id = c.id),
      (SELECT max(t.last_reply_at) FROM tickets t WHERE t.customer_id = c.id),
      (SELECT max(se.created_at) FROM sessions se WHERE se.user_id = c.owner_user_id),
      (SELECT u.last_login_at FROM users u WHERE u.id = c.owner_user_id)) AS last_activity
  FROM customers c
  WHERE c.owner_user_id IS NOT NULL AND c.status <> 'closed' AND c.merged_into IS NULL
    AND NOT EXISTS (SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE ur.user_id = c.owner_user_id AND r.name <> 'customer')
    AND NOT EXISTS (SELECT 1 FROM services s WHERE s.customer_id = c.id AND s.status IN ('pending','active','suspended'))
    AND NOT EXISTS (SELECT 1 FROM domains d WHERE d.customer_id = c.id AND d.status IN ('pending','active','grace','redemption','transferring'))
    AND NOT EXISTS (SELECT 1 FROM invoices i WHERE i.customer_id = c.id AND i.status = 'unpaid')
    AND NOT EXISTS (SELECT 1 FROM tickets t WHERE t.customer_id = c.id AND t.status <> 'closed')
    AND NOT EXISTS (SELECT 1 FROM gdpr_requests g WHERE g.user_id = c.owner_user_id AND g.status = 'pending')
)
SELECT customer_id, user_id, email, last_activity FROM act
WHERE last_activity < now() - make_interval(days => $1)
ORDER BY last_activity, customer_id LIMIT $2";

/// Dry run: who would be erased with `days` of inactivity.
pub async fn candidates(pool: &PgPool, days: i32, limit: i64) -> AppResult<Vec<Candidate>> {
    if !(365..=3650).contains(&days) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as(CANDIDATES).bind(days).bind(limit.clamp(1, 500)).fetch_all(pool).await.map_err(db_err)
}

/// Erase up to `max_per_run` candidates when enabled; returns how many.
pub async fn run(pool: &PgPool) -> AppResult<u64> {
    let cfg = get(pool).await?;
    if !cfg.enabled || cfg.validate().is_err() {
        return Ok(0);
    }
    let mut n = 0;
    for c in candidates(pool, cfg.inactive_days, i64::from(cfg.max_per_run)).await? {
        let rid: Uuid = sqlx::query_scalar(
            "INSERT INTO gdpr_requests (user_id, subject_ref, reason) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(c.user_id)
        .bind(c.email.chars().take(254).collect::<String>())
        .bind(format!("automatic: inactive for {} days", cfg.inactive_days))
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .unwrap_or_default();
        if rid.is_nil() {
            continue;
        }
        crate::gdpr::erase_system(pool, rid).await?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::Settings;
    #[test]
    fn validation() {
        assert!(Settings::default().validate().is_ok());
        assert!(Settings { enabled: true, inactive_days: 364, max_per_run: 10 }.validate().is_err());
        assert!(Settings { enabled: true, inactive_days: 730, max_per_run: 0 }.validate().is_err());
        assert!(Settings { enabled: true, inactive_days: 730, max_per_run: 50 }.validate().is_ok());
        assert!(Settings { enabled: false, inactive_days: 730, max_per_run: 50 }.validate().is_ok());
    }
}
