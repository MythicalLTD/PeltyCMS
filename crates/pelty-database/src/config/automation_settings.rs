//! Per-task automation settings: enable flag, legacy start hour and an
//! optional cron expression (company time zone).
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use time::OffsetDateTime;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct AutomationSetting {
    pub kind: String,
    pub enabled: bool,
    pub run_hour: Option<i16>,
    pub schedule: Option<String>,
    #[serde(skip)]
    pub updated_at: OffsetDateTime,
    /// Consecutive failed runs (reset by a success).
    pub failures: i32,
    /// Set when the circuit breaker disabled the task.
    #[serde(with = "time::serde::timestamp::option")]
    pub tripped_at: Option<OffsetDateTime>,
    pub tripped_error: Option<String>,
    /// Missed-run policy: `skip` or `run_once`.
    pub missed: String,
}

pub const MISSED: &[&str] = &["skip", "run_once"];

pub async fn all(pool: &PgPool) -> AppResult<Vec<AutomationSetting>> {
    sqlx::query_as("SELECT kind, enabled, run_hour, schedule, updated_at, failures, tripped_at, tripped_error, missed FROM automation_settings ORDER BY kind").fetch_all(pool).await.map_err(db_err)
}

pub async fn upsert(
    pool: &PgPool,
    kind: &str,
    enabled: bool,
    run_hour: Option<i16>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO automation_settings (kind, enabled, run_hour) VALUES ($1, $2, $3)
         ON CONFLICT (kind) DO UPDATE SET enabled = $2, run_hour = $3, updated_at = now(),
           failures = CASE WHEN $2 THEN 0 ELSE automation_settings.failures END,
           tripped_at = CASE WHEN $2 THEN NULL ELSE automation_settings.tripped_at END,
           tripped_error = CASE WHEN $2 THEN NULL ELSE automation_settings.tripped_error END",
    )
    .bind(kind)
    .bind(enabled)
    .bind(run_hour)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Set (or clear with `None`) the cron expression; the enable flag is kept.
pub async fn set_schedule(pool: &PgPool, kind: &str, schedule: Option<&str>) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO automation_settings (kind, enabled, schedule) VALUES ($1, true, $2)
         ON CONFLICT (kind) DO UPDATE SET schedule = $2, updated_at = now()",
    )
    .bind(kind)
    .bind(schedule)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Record a finished run for the circuit breaker. A success clears the
/// failure count; a failure increments it and, at `limit`, disables the
/// task. Returns the failure count when this call tripped the breaker.
/// `updated_at` is left alone (it anchors the cron schedule).
pub async fn record_outcome(
    pool: &PgPool,
    kind: &str,
    err: Option<&str>,
    limit: i32,
) -> AppResult<Option<i32>> {
    let Some(err) = err else {
        sqlx::query(
            "UPDATE automation_settings SET failures = 0 WHERE kind = $1 AND failures <> 0",
        )
        .bind(kind)
        .execute(pool)
        .await
        .map_err(db_err)?;
        return Ok(None);
    };
    let err: String = err.chars().take(500).collect();
    let tripped: Option<i32> = sqlx::query_scalar(
        "INSERT INTO automation_settings AS a (kind, enabled, failures) VALUES ($1, true, 1)
         ON CONFLICT (kind) DO UPDATE SET failures = a.failures + 1
         RETURNING CASE WHEN failures >= $2 AND enabled AND tripped_at IS NULL THEN failures END",
    )
    .bind(kind)
    .bind(limit.max(1))
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if tripped.is_some() {
        sqlx::query("UPDATE automation_settings SET enabled = false, tripped_at = now(), tripped_error = $2 WHERE kind = $1").bind(kind).bind(&err).execute(pool).await.map_err(db_err)?;
    }
    Ok(tripped)
}

/// Clear a tripped breaker: failures 0 and the task enabled again.
pub async fn reset(pool: &PgPool, kind: &str) -> AppResult<bool> {
    let r = sqlx::query("UPDATE automation_settings SET enabled = true, failures = 0, tripped_at = NULL, tripped_error = NULL WHERE kind = $1").bind(kind).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Set the missed-run policy (`skip` or `run_once`).
pub async fn set_missed(pool: &PgPool, kind: &str, missed: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO automation_settings (kind, enabled, missed) VALUES ($1, true, $2)
         ON CONFLICT (kind) DO UPDATE SET missed = $2",
    )
    .bind(kind)
    .bind(missed)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}
