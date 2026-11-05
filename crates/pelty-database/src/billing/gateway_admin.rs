//! Gateway administration: config upserts, custom gateways, webhook
//! delivery log, payment lookups for return-time completion.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub async fn upsert(
    pool: &PgPool,
    id: &str,
    enabled: bool,
    config: &serde_json::Value,
    secret_refs: &serde_json::Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO payment_gateways (id, enabled, config, secret_refs) VALUES ($1,$2,$3,$4)
         ON CONFLICT (id) DO UPDATE SET enabled = EXCLUDED.enabled, config = EXCLUDED.config,
         secret_refs = EXCLUDED.secret_refs, updated_at = now()",
    )
    .bind(id)
    .bind(enabled)
    .bind(config)
    .bind(secret_refs)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn delete(pool: &PgPool, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM payment_gateways WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

#[derive(Debug, Default)]
pub struct NewDelivery<'a> {
    pub gateway: &'a str,
    pub is_test: bool,
    pub status_code: i32,
    pub signature_ok: bool,
    pub duplicate: bool,
    pub event_id: Option<&'a str>,
    pub event_type: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub provider_ref: Option<&'a str>,
    pub error: Option<&'a str>,
    pub body_bytes: i32,
}

pub async fn log_delivery(pool: &PgPool, d: &NewDelivery<'_>) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO gateway_webhook_deliveries
         (gateway, is_test, status_code, signature_ok, duplicate, event_id, event_type, kind, provider_ref, error, body_bytes)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(d.gateway)
    .bind(d.is_test)
    .bind(d.status_code)
    .bind(d.signature_ok)
    .bind(d.duplicate)
    .bind(d.event_id)
    .bind(d.event_type)
    .bind(d.kind)
    .bind(d.provider_ref)
    .bind(d.error)
    .bind(d.body_bytes)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Delivery {
    pub id: Uuid,
    pub gateway: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub received_at: time::OffsetDateTime,
    pub is_test: bool,
    pub status_code: i32,
    pub signature_ok: bool,
    pub duplicate: bool,
    pub event_id: Option<String>,
    pub event_type: Option<String>,
    pub kind: Option<String>,
    pub provider_ref: Option<String>,
    pub error: Option<String>,
    pub body_bytes: i32,
}

pub async fn deliveries(
    pool: &PgPool,
    gateway: Option<&str>,
    limit: i64,
) -> AppResult<Vec<Delivery>> {
    sqlx::query_as(
        "SELECT id, gateway, received_at, is_test, status_code, signature_ok, duplicate, event_id, event_type,
                kind, provider_ref, error, body_bytes
         FROM gateway_webhook_deliveries WHERE ($1::text IS NULL OR gateway = $1)
         ORDER BY received_at DESC, id DESC LIMIT $2",
    )
    .bind(gateway)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Stored provider_ref of a payment (fallback when a webhook only echoes our id).
pub async fn provider_ref_for(
    pool: &PgPool,
    gateway: &str,
    payment_id: &str,
) -> AppResult<Option<String>> {
    let Ok(id) = Uuid::parse_str(payment_id) else {
        return Ok(None);
    };
    let r: Option<Option<String>> =
        sqlx::query_scalar("SELECT provider_ref FROM payments WHERE id = $1 AND gateway = $2")
            .bind(id)
            .bind(gateway)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(r.flatten())
}

#[derive(Debug, Clone, FromRow)]
pub struct OwnedPayment {
    pub gateway: String,
    pub provider_ref: Option<String>,
    pub invoice_id: Option<Uuid>,
    pub status: String,
}

pub async fn owned_payment(
    pool: &PgPool,
    id: Uuid,
    customer_id: Uuid,
) -> AppResult<Option<OwnedPayment>> {
    sqlx::query_as("SELECT gateway, provider_ref, invoice_id, status FROM payments WHERE id = $1 AND customer_id = $2")
        .bind(id)
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn payment_exists_by_ref(
    pool: &PgPool,
    gateway: &str,
    provider_ref: &str,
) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM payments WHERE gateway = $1 AND provider_ref = $2)",
    )
    .bind(gateway)
    .bind(provider_ref)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}
