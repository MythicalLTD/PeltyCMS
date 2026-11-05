//! Sent-mail log (WHMCS "Email Message Log" / client "Email History").
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EmailLogEntry {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub recipient: String,
    pub template: String,
    pub locale: String,
    pub subject: String,
    pub status: String,
    pub error: Option<String>,
    pub mailer: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EmailLogDetail {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub entry: EmailLogEntry,
    pub body_text: String,
}

pub struct NewEntry<'a> {
    pub recipient: &'a str,
    pub template: &'a str,
    pub locale: &'a str,
    pub subject: &'a str,
    pub body_text: &'a str,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub mailer: &'a str,
}

/// Record one delivery attempt; the user is matched by e-mail address.
pub async fn record(pool: &PgPool, e: &NewEntry<'_>) -> AppResult<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO email_log (user_id, recipient, template, locale, subject, body_text, status, error, mailer)
         VALUES ((SELECT id FROM users WHERE lower(email) = lower($1) LIMIT 1), $1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id",
    )
    .bind(e.recipient)
    .bind(e.template)
    .bind(e.locale)
    .bind(e.subject)
    .bind(e.body_text)
    .bind(e.status)
    .bind(e.error.map(|x| x.chars().take(1000).collect::<String>()))
    .bind(e.mailer)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok(id)
}

const COLS: &str =
    "id, user_id, recipient, template, locale, subject, status, error, mailer, created_at";

/// Newest first; optional user filter and free-text search on recipient/subject.
pub async fn list(
    pool: &PgPool,
    user_id: Option<Uuid>,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<EmailLogEntry>, i64)> {
    let q = q.map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let filter = "($1::uuid IS NULL OR user_id = $1) AND ($2::text IS NULL OR recipient ILIKE $2 OR subject ILIKE $2)";
    let rows = sqlx::query_as::<_, EmailLogEntry>(&format!("SELECT {COLS} FROM email_log WHERE {filter} ORDER BY created_at DESC, id LIMIT $3 OFFSET $4"))
        .bind(user_id)
        .bind(q.as_deref())
        .bind(limit.clamp(1, 200))
        .bind(offset.max(0))
        .fetch_all(pool)
        .await
    .map_err(db_err)?;
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM email_log WHERE {filter}"))
        .bind(user_id)
        .bind(q.as_deref())
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok((rows, total))
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<EmailLogDetail>> {
    sqlx::query_as::<_, EmailLogDetail>(&format!(
        "SELECT {COLS}, body_text FROM email_log WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Remove entries older than `days` (retention).
pub async fn prune(pool: &PgPool, days: i32) -> AppResult<u64> {
    Ok(
        sqlx::query("DELETE FROM email_log WHERE created_at < now() - make_interval(days => $1)")
            .bind(days)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected(),
    )
}
