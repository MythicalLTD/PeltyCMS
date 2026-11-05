//! Plugin state, gateway config, webhook dedup, payments.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub async fn plugin_states(pool: &PgPool) -> AppResult<Vec<(String, bool)>> {
    sqlx::query_as("SELECT id, enabled FROM plugins")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn set_plugin_enabled(pool: &PgPool, id: &str, enabled: bool) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO plugins (id, enabled) VALUES ($1,$2)
         ON CONFLICT (id) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now()",
    )
    .bind(id)
    .bind(enabled)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct GatewayRow {
    pub id: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub secret_refs: serde_json::Value,
}

pub async fn gateways(pool: &PgPool) -> AppResult<Vec<GatewayRow>> {
    sqlx::query_as("SELECT id, enabled, config, secret_refs FROM payment_gateways ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn gateway(pool: &PgPool, id: &str) -> AppResult<Option<GatewayRow>> {
    sqlx::query_as("SELECT id, enabled, config, secret_refs FROM payment_gateways WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Insert a webhook event; returns false if it was already seen (duplicate).
pub async fn record_webhook(
    pool: &PgPool,
    gateway: &str,
    event_id: &str,
    ty: &str,
) -> AppResult<bool> {
    let r = sqlx::query(
        "INSERT INTO webhook_events (id, gateway, provider_event_id, event_type) VALUES ($1,$2,$3,$4)
         ON CONFLICT (gateway, provider_event_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(gateway)
    .bind(event_id)
    .bind(ty)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn mark_webhook_processed(pool: &PgPool, gateway: &str, event_id: &str) -> AppResult<()> {
    sqlx::query("UPDATE webhook_events SET processed_at = now() WHERE gateway = $1 AND provider_event_id = $2")
        .bind(gateway)
        .bind(event_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_payment_status_by_ref(
    pool: &PgPool,
    gateway: &str,
    provider_ref: &str,
    status: &str,
) -> AppResult<u64> {
    let r = sqlx::query("UPDATE payments SET status = $3, updated_at = now() WHERE gateway = $1 AND provider_ref = $2 AND (applied_at IS NULL OR $3 = 'refunded')")
        .bind(gateway)
        .bind(provider_ref)
        .bind(status)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected())
}

pub async fn plugin_settings(pool: &PgPool, id: &str) -> AppResult<serde_json::Value> {
    let r: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT settings FROM plugins WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(r.map(|r| r.0).unwrap_or_else(|| serde_json::json!({})))
}

pub async fn set_plugin_settings(
    pool: &PgPool,
    id: &str,
    settings: &serde_json::Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO plugins (id, settings) VALUES ($1,$2)
         ON CONFLICT (id) DO UPDATE SET settings = EXCLUDED.settings, updated_at = now()",
    )
    .bind(id)
    .bind(settings)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}
