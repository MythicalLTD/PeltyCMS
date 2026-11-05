//! Outgoing webhook endpoints (admin-managed) and their delivery log.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Endpoint {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub failure_streak: i32,
    pub disabled_reason: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_delivery_at: Option<OffsetDateTime>,
}

const ECOLS: &str =
    "id, name, url, events, enabled, failure_streak, disabled_reason, created_at, last_delivery_at";

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Delivery {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub event: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub response_status: Option<i32>,
    pub last_error: Option<String>,
    pub duration_ms: Option<i32>,
    pub redelivery_of: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub delivered_at: Option<OffsetDateTime>,
}

const DCOLS: &str = "id, endpoint_id, event, payload, status, attempts, response_status, last_error, duration_ms, redelivery_of, created_at, delivered_at";

/// Does an endpoint filter (`*`, `invoice.*`, `invoice.paid`) match `event`?
pub fn matches(filters: &[String], event: &str) -> bool {
    filters.iter().any(|f| {
        f == "*"
            || f == event
            || f.strip_suffix(".*").is_some_and(|p| {
                event
                    .strip_prefix(p)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
    })
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<Endpoint>> {
    sqlx::query_as(&format!(
        "SELECT {ECOLS} FROM webhook_endpoints ORDER BY created_at"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Endpoint>> {
    sqlx::query_as(&format!(
        "SELECT {ECOLS} FROM webhook_endpoints WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Enabled endpoints whose filter matches `event` (with their sealed secret).
pub async fn targets(pool: &PgPool, event: &str) -> AppResult<Vec<Endpoint>> {
    let all: Vec<Endpoint> = sqlx::query_as(&format!(
        "SELECT {ECOLS} FROM webhook_endpoints WHERE enabled"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(all
        .into_iter()
        .filter(|e| matches(&e.events, event))
        .collect())
}

pub async fn secret(pool: &PgPool, id: Uuid) -> AppResult<Option<Vec<u8>>> {
    sqlx::query_scalar("SELECT secret_sealed FROM webhook_endpoints WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn create(
    pool: &PgPool,
    name: &str,
    url: &str,
    events: &[String],
    sealed: &[u8],
    enabled: bool,
    by: Uuid,
) -> AppResult<Endpoint> {
    sqlx::query_as(&format!(
        "INSERT INTO webhook_endpoints (name, url, events, secret_sealed, enabled, created_by) VALUES ($1,$2,$3,$4,$5,$6) RETURNING {ECOLS}"
    ))
    .bind(name)
    .bind(url)
    .bind(events)
    .bind(sealed)
    .bind(enabled)
    .bind(by)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Update; enabling clears the failure streak and the auto-disable reason.
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    url: &str,
    events: &[String],
    enabled: bool,
) -> AppResult<Endpoint> {
    sqlx::query_as(&format!(
        "UPDATE webhook_endpoints SET name = $2, url = $3, events = $4, enabled = $5, updated_at = now(),
                failure_streak = CASE WHEN $5 AND NOT enabled THEN 0 ELSE failure_streak END,
                disabled_reason = CASE WHEN $5 THEN '' ELSE disabled_reason END
         WHERE id = $1 RETURNING {ECOLS}"
    ))
    .bind(id)
    .bind(name)
    .bind(url)
    .bind(events)
    .bind(enabled)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn set_secret(pool: &PgPool, id: Uuid, sealed: &[u8]) -> AppResult<()> {
    let r = sqlx::query(
        "UPDATE webhook_endpoints SET secret_sealed = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(sealed)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM webhook_endpoints WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn create_delivery(
    pool: &PgPool,
    endpoint: Uuid,
    event: &str,
    payload: &serde_json::Value,
    redelivery_of: Option<Uuid>,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO webhook_deliveries (id, endpoint_id, event, payload, redelivery_of) VALUES ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(endpoint)
        .bind(event)
        .bind(payload)
        .bind(redelivery_of)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(id)
}

pub async fn delivery(pool: &PgPool, id: Uuid) -> AppResult<Option<Delivery>> {
    sqlx::query_as(&format!(
        "SELECT {DCOLS} FROM webhook_deliveries WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Delivery log, newest first, optionally per endpoint / status / event.
pub async fn deliveries(
    pool: &PgPool,
    endpoint: Option<Uuid>,
    status: Option<&str>,
    event: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Delivery>, i64)> {
    let filter = "($1::uuid IS NULL OR endpoint_id = $1) AND ($2::text IS NULL OR status = $2) AND ($3::text IS NULL OR event = $3)";
    let items = sqlx::query_as(&format!("SELECT {DCOLS} FROM webhook_deliveries WHERE {filter} ORDER BY created_at DESC, id LIMIT $4 OFFSET $5"))
        .bind(endpoint)
        .bind(status)
        .bind(event)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let total = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM webhook_deliveries WHERE {filter}"
    ))
    .bind(endpoint)
    .bind(status)
    .bind(event)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((items, total))
}

/// Record one attempt. `final_fail` marks the delivery failed (no more
/// retries) and bumps the endpoint's failure streak; success resets it.
/// Returns the endpoint's streak after the update.
pub async fn record(
    pool: &PgPool,
    id: Uuid,
    status: Option<u16>,
    error: Option<&str>,
    ok: bool,
    final_fail: bool,
    duration_ms: i32,
) -> AppResult<i32> {
    let st = if ok {
        "delivered"
    } else if final_fail {
        "failed"
    } else {
        "pending"
    };
    let endpoint: Option<Uuid> = sqlx::query_scalar(
        "UPDATE webhook_deliveries SET attempts = attempts + 1, response_status = $2, last_error = $3, status = $4, duration_ms = $5,
         delivered_at = CASE WHEN $4 = 'delivered' THEN now() ELSE delivered_at END WHERE id = $1 RETURNING endpoint_id",
    )
    .bind(id)
    .bind(status.map(i32::from))
    .bind(error)
    .bind(st)
    .bind(duration_ms)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some(endpoint) = endpoint else {
        return Ok(0);
    };
    let streak: Option<i32> = sqlx::query_scalar(
        "UPDATE webhook_endpoints SET last_delivery_at = now(),
                failure_streak = CASE WHEN $2 THEN 0 WHEN $3 THEN failure_streak + 1 ELSE failure_streak END
         WHERE id = $1 RETURNING failure_streak",
    )
    .bind(endpoint)
    .bind(ok)
    .bind(final_fail)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(streak.unwrap_or(0))
}

/// Auto-disable an endpoint (e.g. after too many failed deliveries).
pub async fn disable(pool: &PgPool, id: Uuid, reason: &str) -> AppResult<bool> {
    let r = sqlx::query("UPDATE webhook_endpoints SET enabled = false, disabled_reason = $2, updated_at = now() WHERE id = $1 AND enabled")
        .bind(id)
        .bind(reason)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Endpoint id of a delivery's endpoint (for disable after record).
pub async fn endpoint_of(pool: &PgPool, delivery: Uuid) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT endpoint_id FROM webhook_deliveries WHERE id = $1")
        .bind(delivery)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn filters() {
        let f = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(matches(&f(&["*"]), "invoice.paid"));
        assert!(matches(&f(&["invoice.*"]), "invoice.paid"));
        assert!(!matches(&f(&["invoice.*"]), "invoices.paid"));
        assert!(!matches(&f(&["invoice.*"]), "invoice"));
        assert!(matches(&f(&["order.created", "ticket.*"]), "ticket.opened"));
        assert!(!matches(&f(&["order.created"]), "order.approved"));
    }
}
