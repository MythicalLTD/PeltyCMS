//! ICANN registrant e-mail verification (WHOIS accuracy).
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Verification {
    pub domain_id: Uuid,
    pub email: String,
    pub status: String,
    #[schema(value_type = String)]
    pub deadline: Date,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub sent_at: OffsetDateTime,
    pub reminders: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub verified_at: Option<OffsetDateTime>,
}

const COLS: &str = "domain_id, email, status, deadline, sent_at, reminders, verified_at";

pub async fn get(pool: &PgPool, domain: Uuid) -> AppResult<Option<Verification>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM registrant_verifications WHERE domain_id = $1"
    ))
    .bind(domain)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Has this address been verified before (for any domain)?
pub async fn email_verified(pool: &PgPool, email: &str) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM registrant_verifications WHERE status = 'verified' AND lower(email) = lower($1))")
        .bind(email).fetch_one(pool).await.map_err(db_err)
}

/// Start (or restart) a pending verification with a fresh token.
pub async fn start(
    pool: &PgPool,
    domain: Uuid,
    email: &str,
    token_hash: &[u8],
    deadline: Date,
) -> AppResult<Verification> {
    sqlx::query_as(&format!(
        "INSERT INTO registrant_verifications (domain_id, email, token_hash, deadline) VALUES ($1, $2, $3, $4)
         ON CONFLICT (domain_id) DO UPDATE SET email = EXCLUDED.email, token_hash = EXCLUDED.token_hash, deadline = EXCLUDED.deadline,
           status = 'pending', sent_at = now(), reminders = 0, verified_at = NULL
         RETURNING {COLS}"
    ))
    .bind(domain).bind(email).bind(token_hash).bind(deadline).fetch_one(pool).await.map_err(db_err)
}

/// Already verified without a mail (address known good).
pub async fn mark_verified_direct(
    pool: &PgPool,
    domain: Uuid,
    email: &str,
    token_hash: &[u8],
    today: Date,
) -> AppResult<Verification> {
    sqlx::query_as(&format!(
        "INSERT INTO registrant_verifications (domain_id, email, token_hash, deadline, status, verified_at) VALUES ($1, $2, $3, $4, 'verified', now())
         ON CONFLICT (domain_id) DO UPDATE SET email = EXCLUDED.email, token_hash = EXCLUDED.token_hash, status = 'verified', verified_at = now()
         RETURNING {COLS}"
    ))
    .bind(domain).bind(email).bind(token_hash).bind(today).fetch_one(pool).await.map_err(db_err)
}

/// New token on resend; keeps the deadline.
pub async fn rotate(
    pool: &PgPool,
    domain: Uuid,
    token_hash: &[u8],
    reminder: bool,
) -> AppResult<Option<Verification>> {
    sqlx::query_as(&format!(
        "UPDATE registrant_verifications SET token_hash = $2, sent_at = now(), reminders = reminders + CASE WHEN $3 THEN 1 ELSE 0 END
         WHERE domain_id = $1 AND status <> 'verified' RETURNING {COLS}"
    ))
    .bind(domain).bind(token_hash).bind(reminder).fetch_optional(pool).await.map_err(db_err)
}

/// Consume a token: verifies its domain and every other open request for
/// the same address. Returns the verified domain ids.
pub async fn verify_token(pool: &PgPool, token_hash: &[u8]) -> AppResult<Vec<Uuid>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM registrant_verifications WHERE token_hash = $1 AND status <> 'verified' FOR UPDATE")
        .bind(token_hash).fetch_optional(&mut *tx).await.map_err(db_err)?;
    let Some(email) = email else {
        return Ok(vec![]);
    };
    let ids: Vec<Uuid> = sqlx::query_scalar("UPDATE registrant_verifications SET status = 'verified', verified_at = now() WHERE lower(email) = lower($1) AND status <> 'verified' RETURNING domain_id")
        .bind(&email).fetch_all(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(ids)
}

pub async fn admin_verify(pool: &PgPool, domain: Uuid) -> AppResult<bool> {
    Ok(sqlx::query("UPDATE registrant_verifications SET status = 'verified', verified_at = now() WHERE domain_id = $1 AND status <> 'verified'")
        .bind(domain).execute(pool).await.map_err(db_err)?.rows_affected() > 0)
}

/// Open requests: (row, days left until deadline).
pub async fn open(pool: &PgPool) -> AppResult<Vec<Verification>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM registrant_verifications WHERE status = 'pending' ORDER BY deadline"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Pending past the deadline → overdue. Returns those domain ids.
pub async fn mark_overdue(pool: &PgPool, today: Date) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar("UPDATE registrant_verifications SET status = 'overdue' WHERE status = 'pending' AND deadline < $1 RETURNING domain_id")
        .bind(today).fetch_all(pool).await.map_err(db_err)
}

/// Counts for the admin overview.
pub async fn counts(pool: &PgPool) -> AppResult<(i64, i64, i64)> {
    sqlx::query_as("SELECT count(*) FILTER (WHERE status = 'pending'), count(*) FILTER (WHERE status = 'overdue'), count(*) FILTER (WHERE status = 'verified') FROM registrant_verifications")
        .fetch_one(pool).await.map_err(db_err)
}
