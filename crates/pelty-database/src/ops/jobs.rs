//! Postgres job queue: `FOR UPDATE SKIP LOCKED` claiming, exponential
//! backoff retries, idempotency keys and stuck-job recovery.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub run_at: OffsetDateTime,
    pub locked_by: Option<String>,
    pub last_error: Option<String>,
    pub idempotency_key: Option<String>,
    pub result: Option<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
}

/// Email jobs carry one-time links; their payload is never exposed via the
/// API and is wiped once delivered.
const VIEW_COLS: &str = "id, kind, CASE WHEN kind LIKE 'email.%' THEN '{\"redacted\":true}'::jsonb ELSE payload END AS payload, status, attempts, max_attempts, run_at, locked_by, last_error, idempotency_key, result, created_at, finished_at";
const COLS: &str = "id, kind, payload, status, attempts, max_attempts, run_at, locked_by, last_error, idempotency_key, result, created_at, finished_at";

/// Backoff before retry `attempt` (1-based): 10s, 20s, 40s … capped at 1h.
pub fn backoff_secs(attempt: i32) -> i64 {
    let a = attempt.clamp(1, 20) as u32;
    (10i64 << (a - 1)).min(3600)
}

/// Enqueue; with a key, a second enqueue is a no-op returning `None`.
pub async fn enqueue(
    pool: &PgPool,
    kind: &str,
    payload: serde_json::Value,
    run_at: Option<OffsetDateTime>,
    key: Option<&str>,
    max_attempts: i32,
) -> AppResult<Option<i64>> {
    sqlx::query_scalar(
        "INSERT INTO jobs (kind, payload, run_at, idempotency_key, max_attempts) VALUES ($1,$2,COALESCE($3, now()),$4,$5)
         ON CONFLICT (idempotency_key) DO NOTHING RETURNING id",
    )
    .bind(kind)
    .bind(payload)
    .bind(run_at)
    .bind(key)
    .bind(max_attempts.max(1))
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Enqueue an outbox job in the caller's transaction. The job becomes visible
/// to workers only when the associated domain changes commit.
pub async fn enqueue_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: &str,
    payload: serde_json::Value,
    key: &str,
    max_attempts: i32,
) -> AppResult<Option<i64>> {
    sqlx::query_scalar(
        "INSERT INTO jobs (kind, payload, idempotency_key, max_attempts) VALUES ($1,$2,$3,$4)
         ON CONFLICT (idempotency_key) DO NOTHING RETURNING id",
    )
    .bind(kind)
    .bind(payload)
    .bind(key)
    .bind(max_attempts.max(1))
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)
}

/// Claim up to `n` ready jobs for `worker`. Concurrent workers never get the
/// same job (SKIP LOCKED); claiming counts as an attempt.
pub async fn claim(pool: &PgPool, worker: &str, n: i64) -> AppResult<Vec<Job>> {
    sqlx::query_as(&format!(
        "WITH c AS (UPDATE jobs SET status = 'running', locked_at = now(), locked_by = $1, attempts = attempts + 1
         WHERE id IN (SELECT id FROM jobs WHERE status = 'queued' AND run_at <= now()
                      ORDER BY run_at, id FOR UPDATE SKIP LOCKED LIMIT $2)
         RETURNING {COLS}) SELECT * FROM c ORDER BY run_at, id"
    ))
    .bind(worker)
    .bind(n)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn complete(pool: &PgPool, id: i64, result: serde_json::Value) -> AppResult<()> {
    sqlx::query("UPDATE jobs SET status = 'succeeded', result = $2,
            payload = CASE WHEN kind LIKE 'email.%' THEN '{}'::jsonb ELSE payload END, finished_at = now(), locked_at = NULL WHERE id = $1 AND status = 'running'")
        .bind(id)
        .bind(result)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Record a failure: requeue with backoff, or mark `dead` after max attempts.
pub async fn fail(pool: &PgPool, id: i64, error: &str) -> AppResult<String> {
    let err: String = error.chars().take(2000).collect();
    sqlx::query_scalar(
        "UPDATE jobs SET last_error = $2, locked_at = NULL, locked_by = NULL,
            status = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'queued' END,
            finished_at = CASE WHEN attempts >= max_attempts THEN now() END,
            run_at = now() + make_interval(secs => LEAST(3600, 10 * power(2, GREATEST(attempts, 1) - 1)))
         WHERE id = $1 AND status = 'running' RETURNING status",
    )
    .bind(id)
    .bind(err)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Record a permanent failure (bad config, rejected request): no retries.
pub async fn fail_permanent(pool: &PgPool, id: i64, error: &str) -> AppResult<String> {
    let err: String = error.chars().take(2000).collect();
    sqlx::query_scalar(
        "UPDATE jobs SET last_error = $2, locked_at = NULL, locked_by = NULL, status = 'dead', finished_at = now()
         WHERE id = $1 AND status = 'running' RETURNING status",
    )
    .bind(id)
    .bind(err)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Jobs stuck in `running` longer than `secs` (crashed worker) go back to
/// the queue, or to `dead` if out of attempts. Returns how many.
pub async fn recover_stuck(pool: &PgPool, secs: i64) -> AppResult<u64> {
    let r = sqlx::query(
        "UPDATE jobs SET locked_at = NULL, locked_by = NULL, last_error = COALESCE(last_error, 'worker lost'),
            status = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'queued' END,
            finished_at = CASE WHEN attempts >= max_attempts THEN now() END
         WHERE status = 'running' AND locked_at < now() - make_interval(secs => $1::double precision)",
    )
    .bind(secs as f64)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Manually retry a dead job from scratch.
pub async fn retry(pool: &PgPool, id: i64) -> AppResult<Job> {
    sqlx::query_as(&format!(
        "UPDATE jobs SET status = 'queued', attempts = 0, run_at = now(), finished_at = NULL WHERE id = $1 AND status IN ('dead','failed')
         RETURNING {VIEW_COLS}"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::Conflict("error.job_not_retryable"))
}

pub async fn get(pool: &PgPool, id: i64) -> AppResult<Option<Job>> {
    sqlx::query_as(&format!("SELECT {VIEW_COLS} FROM jobs WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn list(
    pool: &PgPool,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Job>, i64)> {
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        j: Job,
        total_count: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {VIEW_COLS}, count(*) OVER() AS total_count FROM jobs WHERE ($1::text IS NULL OR status = $1)
         ORDER BY created_at DESC, id DESC LIMIT $2 OFFSET $3"
    ))
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = rows.first().map(|r| r.total_count).unwrap_or(0);
    Ok((rows.into_iter().map(|r| r.j).collect(), total))
}
