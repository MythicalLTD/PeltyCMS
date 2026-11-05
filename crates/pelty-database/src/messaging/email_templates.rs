//! Operator overrides for email templates. Defaults live in locale files
//! (`email.<name>.subject` / `email.<name>.body`). Optional `html` replaces
//! the auto-generated branded wrapper when set.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct EmailTemplate {
    pub name: String,
    pub locale: String,
    pub subject: String,
    pub body: String,
    /// Custom HTML multipart part; `None` = derive HTML from `body`.
    pub html: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub async fn get(pool: &PgPool, name: &str, locale: &str) -> AppResult<Option<EmailTemplate>> {
    sqlx::query_as(
        "SELECT name, locale, subject, body, html, updated_at FROM email_templates WHERE name = $1 AND locale = $2",
    )
    .bind(name)
    .bind(locale)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<EmailTemplate>> {
    sqlx::query_as(
        "SELECT name, locale, subject, body, html, updated_at FROM email_templates ORDER BY name, locale",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn upsert(
    pool: &PgPool,
    name: &str,
    locale: &str,
    subject: &str,
    body: &str,
    html: Option<&str>,
) -> AppResult<EmailTemplate> {
    sqlx::query_as(
        "INSERT INTO email_templates (name, locale, subject, body, html) VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (name, locale) DO UPDATE
           SET subject = EXCLUDED.subject, body = EXCLUDED.body, html = EXCLUDED.html, updated_at = now()
         RETURNING name, locale, subject, body, html, updated_at",
    )
    .bind(name)
    .bind(locale)
    .bind(subject)
    .bind(body)
    .bind(html)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn delete(pool: &PgPool, name: &str, locale: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM email_templates WHERE name = $1 AND locale = $2")
        .bind(name)
        .bind(locale)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}
