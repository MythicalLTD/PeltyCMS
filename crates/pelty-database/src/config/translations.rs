//! Translation editor overrides.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn all(pool: &PgPool) -> AppResult<Vec<(String, String, String)>> {
    sqlx::query_as("SELECT lang, key, value FROM translation_overrides")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn for_lang(pool: &PgPool, lang: &str) -> AppResult<Vec<(String, String)>> {
    sqlx::query_as("SELECT key, value FROM translation_overrides WHERE lang = $1")
        .bind(lang)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn set(pool: &PgPool, lang: &str, key: &str, value: &str, by: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO translation_overrides (lang, key, value, updated_by) VALUES ($1,$2,$3,$4)
         ON CONFLICT (lang, key) DO UPDATE SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(lang).bind(key).bind(value).bind(by).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn delete(pool: &PgPool, lang: &str, key: &str) -> AppResult<bool> {
    Ok(
        sqlx::query("DELETE FROM translation_overrides WHERE lang = $1 AND key = $2")
            .bind(lang)
            .bind(key)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected()
            > 0,
    )
}
