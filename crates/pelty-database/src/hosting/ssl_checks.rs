//! Stored results of TLS certificate probes for client domains.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct SslCheck {
    pub domain_id: Uuid,
    pub domain: String,
    pub customer_id: Uuid,
    pub status: String,
    pub issuer: String,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub valid_from: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<OffsetDateTime>,
    pub error: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub checked_at: OffsetDateTime,
}

pub struct Probe {
    pub status: &'static str,
    pub issuer: String,
    pub valid_from: Option<OffsetDateTime>,
    pub expires_at: Option<OffsetDateTime>,
    pub error: String,
}

const SEL: &str = "SELECT c.domain_id, d.name AS domain, d.customer_id, c.status, c.issuer, c.valid_from, c.expires_at, c.error, c.checked_at \
    FROM ssl_checks c JOIN domains d ON d.id = c.domain_id";

pub async fn list(
    pool: &PgPool,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<SslCheck>, i64)> {
    let rows = sqlx::query_as(sqlx::AssertSqlSafe(format!("{SEL} WHERE ($1::text IS NULL OR c.status = $1) ORDER BY c.expires_at NULLS FIRST, d.name LIMIT $2 OFFSET $3")))
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let total = sqlx::query_scalar(
        "SELECT count(*) FROM ssl_checks WHERE ($1::text IS NULL OR status = $1)",
    )
    .bind(status)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((rows, total))
}

pub async fn for_customer(pool: &PgPool, customer: Uuid) -> AppResult<Vec<SslCheck>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SEL} WHERE d.customer_id = $1 ORDER BY d.name"
    )))
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, domain: Uuid) -> AppResult<Option<SslCheck>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{SEL} WHERE c.domain_id = $1")))
        .bind(domain)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Active domains never checked or last checked more than `hours` ago.
pub async fn due(pool: &PgPool, hours: i32, limit: i64) -> AppResult<Vec<(Uuid, String)>> {
    sqlx::query_as(
        "SELECT d.id, d.name FROM domains d LEFT JOIN ssl_checks c ON c.domain_id = d.id
         WHERE d.status = 'active' AND (c.checked_at IS NULL OR c.checked_at < now() - make_interval(hours => $1))
         ORDER BY c.checked_at NULLS FIRST LIMIT $2",
    )
    .bind(hours)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn domain_name(pool: &PgPool, domain: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar("SELECT name FROM domains WHERE id = $1")
        .bind(domain)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn save(pool: &PgPool, domain: Uuid, p: &Probe) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO ssl_checks (domain_id, status, issuer, valid_from, expires_at, error, checked_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (domain_id) DO UPDATE SET status = EXCLUDED.status, issuer = EXCLUDED.issuer, valid_from = EXCLUDED.valid_from,
           expires_at = EXCLUDED.expires_at, error = EXCLUDED.error, checked_at = now()",
    )
    .bind(domain)
    .bind(p.status)
    .bind(p.issuer.chars().take(500).collect::<String>())
    .bind(p.valid_from)
    .bind(p.expires_at)
    .bind(p.error.chars().take(500).collect::<String>())
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}
