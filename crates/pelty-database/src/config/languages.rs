//! Catalog of UI / mail languages. Bundled `locales/*.json` are seeded as
//! `built_in`; admins can add more without shipping a file.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Language {
    pub code: String,
    pub name: String,
    pub built_in: bool,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<Language>> {
    sqlx::query_as("SELECT code, name, built_in, enabled, created_at FROM languages ORDER BY code")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn list_enabled(pool: &PgPool) -> AppResult<Vec<Language>> {
    sqlx::query_as(
        "SELECT code, name, built_in, enabled, created_at FROM languages WHERE enabled ORDER BY code",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, code: &str) -> AppResult<Option<Language>> {
    sqlx::query_as(
        "SELECT code, name, built_in, enabled, created_at FROM languages WHERE code = $1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn insert(pool: &PgPool, code: &str, name: &str) -> AppResult<Language> {
    sqlx::query_as(
        "INSERT INTO languages (code, name, built_in, enabled) VALUES ($1,$2,false,true)
         RETURNING code, name, built_in, enabled, created_at",
    )
    .bind(code)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn delete(pool: &PgPool, code: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM languages WHERE code = $1 AND built_in = false")
        .bind(code)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Drop all translation overrides for a removed language.
pub async fn wipe_overrides(pool: &PgPool, code: &str) -> AppResult<u64> {
    let r = sqlx::query("DELETE FROM translation_overrides WHERE lang = $1")
        .bind(code)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected())
}
