//! Plugin webhook endpoints (secret + URL override) and the delivery log.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Delivery {
    pub id: Uuid,
    pub plugin_id: String,
    pub event: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub response_status: Option<i32>,
    pub last_error: Option<String>,
    pub url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub delivered_at: Option<OffsetDateTime>,
}

const COLS: &str = "id, plugin_id, event, payload, status, attempts, response_status, last_error, url, created_at, delivered_at";

/// (url_override, secret_sealed)
pub async fn endpoint(pool: &PgPool, plugin: &str) -> AppResult<(Option<String>, Option<Vec<u8>>)> {
    let r: Option<(Option<String>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT url_override, secret_sealed FROM plugin_webhooks WHERE plugin_id = $1").bind(plugin).fetch_optional(pool).await.map_err(db_err)?;
    Ok(r.unwrap_or((None, None)))
}

pub async fn set_url(pool: &PgPool, plugin: &str, url: Option<&str>) -> AppResult<()> {
    sqlx::query("INSERT INTO plugin_webhooks (plugin_id, url_override) VALUES ($1,$2) ON CONFLICT (plugin_id) DO UPDATE SET url_override = $2, updated_at = now()")
        .bind(plugin)
        .bind(url)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_secret(pool: &PgPool, plugin: &str, sealed: &[u8]) -> AppResult<()> {
    sqlx::query("INSERT INTO plugin_webhooks (plugin_id, secret_sealed) VALUES ($1,$2) ON CONFLICT (plugin_id) DO UPDATE SET secret_sealed = $2, updated_at = now()")
        .bind(plugin)
        .bind(sealed)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Store a secret only if none exists yet; returns the one in effect.
pub async fn ensure_secret(pool: &PgPool, plugin: &str, sealed: &[u8]) -> AppResult<Vec<u8>> {
    sqlx::query("INSERT INTO plugin_webhooks (plugin_id, secret_sealed) VALUES ($1,$2) ON CONFLICT (plugin_id) DO UPDATE SET secret_sealed = COALESCE(plugin_webhooks.secret_sealed, EXCLUDED.secret_sealed)")
        .bind(plugin)
        .bind(sealed)
        .execute(pool)
        .await
        .map_err(db_err)?;
    let s: Option<Vec<u8>> = sqlx::query_scalar("SELECT secret_sealed FROM plugin_webhooks WHERE plugin_id = $1").bind(plugin).fetch_one(pool).await.map_err(db_err)?;
    Ok(s.unwrap_or_default())
}

pub async fn create(pool: &PgPool, plugin: &str, event: &str, payload: &serde_json::Value) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO plugin_deliveries (id, plugin_id, event, payload) VALUES ($1,$2,$3,$4)")
        .bind(id)
        .bind(plugin)
        .bind(event)
        .bind(payload)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(id)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Delivery>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM plugin_deliveries WHERE id = $1")).bind(id).fetch_optional(pool).await.map_err(db_err)
}

/// Record one attempt. `ok` marks it delivered; `final_fail` marks it failed.
pub async fn record(pool: &PgPool, id: Uuid, url: &str, status: Option<u16>, error: Option<&str>, ok: bool, final_fail: bool) -> AppResult<()> {
    let st = if ok { "delivered" } else if final_fail { "failed" } else { "pending" };
    sqlx::query(
        "UPDATE plugin_deliveries SET attempts = attempts + 1, url = $2, response_status = $3, last_error = $4, status = $5,
         delivered_at = CASE WHEN $5 = 'delivered' THEN now() ELSE delivered_at END WHERE id = $1",
    )
    .bind(id)
    .bind(url)
    .bind(status.map(i32::from))
    .bind(error)
    .bind(st)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn reset(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE plugin_deliveries SET status = 'pending', delivered_at = NULL WHERE id = $1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn list(pool: &PgPool, plugin: &str, status: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Delivery>, i64)> {
    let rows = sqlx::query_as(&format!(
        "SELECT {COLS} FROM plugin_deliveries WHERE plugin_id = $1 AND ($2::text IS NULL OR status = $2) ORDER BY created_at DESC, id LIMIT $3 OFFSET $4"
    ))
    .bind(plugin)
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM plugin_deliveries WHERE plugin_id = $1 AND ($2::text IS NULL OR status = $2)")
        .bind(plugin)
        .bind(status)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok((rows, total))
}
