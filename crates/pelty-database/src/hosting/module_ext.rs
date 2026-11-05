//! Server health, module log, the
//! failed-provisioning queue and licensing lookups.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

// ---- health ---------------------------------------------------------------

pub async fn record_check(pool: &PgPool, server: Uuid, ok: bool, error: &str) -> AppResult<()> {
    sqlx::query("UPDATE servers SET last_check_ok = $2, last_check_error = $3, last_checked_at = now() WHERE id = $1").bind(server).bind(ok).bind(error).execute(pool).await.map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ServerHealth {
    pub id: Uuid,
    pub name: String,
    pub module: String,
    pub enabled: bool,
    pub group_id: Option<Uuid>,
    pub max_services: Option<i32>,
    pub active: i64,
    pub suspended: i64,
    pub failed: i64,
    pub last_check_ok: Option<bool>,
    pub last_check_error: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_checked_at: Option<OffsetDateTime>,
}

pub async fn health(pool: &PgPool) -> AppResult<Vec<ServerHealth>> {
    sqlx::query_as(
        "SELECT v.id, v.name, v.module, v.enabled, v.group_id, v.max_services,
                (SELECT count(*) FROM services s WHERE s.server_id = v.id AND s.status = 'active') AS active,
                (SELECT count(*) FROM services s WHERE s.server_id = v.id AND s.status = 'suspended') AS suspended,
                (SELECT count(*) FROM services s WHERE s.server_id = v.id AND s.provision_status = 'failed') AS failed,
                v.last_check_ok, v.last_check_error, v.last_checked_at
           FROM servers v ORDER BY v.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn server_ids(pool: &PgPool) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM servers WHERE enabled ORDER BY name").fetch_all(pool).await.map_err(db_err)
}

// ---- module log -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LogEntry<'a> {
    pub service: Option<Uuid>,
    pub server: Option<Uuid>,
    pub module: &'a str,
    pub action: &'a str,
    pub actor: Option<Uuid>,
    pub source: &'a str,
    pub ok: bool,
    pub error: &'a str,
    pub output: &'a Value,
    pub duration_ms: i32,
}

/// Keys never written to the log.
const SECRET_KEYS: &[&str] = &["password", "initial_password", "ticket", "root_password", "root_pass", "certificate", "token", "api_key", "secret"];

pub fn redact(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), if SECRET_KEYS.contains(&k.to_ascii_lowercase().as_str()) && !x.is_null() { Value::String("[redacted]".into()) } else { redact(x) })).collect()),
        Value::Array(a) => Value::Array(a.iter().map(redact).collect()),
        other => other.clone(),
    }
}

pub async fn log(pool: &PgPool, e: LogEntry<'_>) -> AppResult<()> {
    let err: String = e.error.chars().take(2000).collect();
    sqlx::query("INSERT INTO module_log (service_id, server_id, module, action, actor_id, source, ok, error, output, duration_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(e.service)
        .bind(e.server)
        .bind(e.module.chars().take(64).collect::<String>())
        .bind(e.action.chars().take(64).collect::<String>())
        .bind(e.actor)
        .bind(e.source)
        .bind(e.ok)
        .bind(err)
        .bind(redact(e.output))
        .bind(e.duration_ms)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct LogRow {
    pub id: i64,
    pub service_id: Option<Uuid>,
    pub server_id: Option<Uuid>,
    pub module: String,
    pub action: String,
    pub actor_id: Option<Uuid>,
    pub source: String,
    pub ok: bool,
    pub error: String,
    pub output: Value,
    pub duration_ms: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn list_log(pool: &PgPool, service: Option<Uuid>, only_errors: bool, limit: i64, before: Option<i64>) -> AppResult<Vec<LogRow>> {
    sqlx::query_as(
        "SELECT id, service_id, server_id, module, action, actor_id, source, ok, error, output, duration_ms, created_at FROM module_log
          WHERE ($1::uuid IS NULL OR service_id = $1) AND (NOT $2 OR NOT ok) AND ($4::bigint IS NULL OR id < $4)
          ORDER BY id DESC LIMIT $3",
    )
    .bind(service)
    .bind(only_errors)
    .bind(limit.clamp(1, 200))
    .bind(before)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ---- failed queue ---------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct FailedService {
    pub service_id: Uuid,
    pub product_name: String,
    pub customer_email: String,
    pub module: Option<String>,
    pub provision_error: String,
    pub status: String,
}

pub async fn failed(pool: &PgPool) -> AppResult<Vec<FailedService>> {
    sqlx::query_as(
        "SELECT s.id AS service_id, p.name AS product_name, c.email::text AS customer_email, p.module, s.provision_error, s.status
           FROM services s JOIN products p ON p.id = s.product_id JOIN customers c ON c.id = s.customer_id
          WHERE s.provision_status = 'failed' ORDER BY s.id LIMIT 500",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Latest dead provisioning job of a service (to retry).
pub async fn last_dead_job(pool: &PgPool, service: Uuid) -> AppResult<Option<i64>> {
    sqlx::query_scalar("SELECT id FROM jobs WHERE kind = 'provision' AND status = 'dead' AND payload->>'service_id' = $1 ORDER BY id DESC LIMIT 1").bind(service.to_string()).fetch_optional(pool).await.map_err(db_err)
}

pub async fn set_pending(pool: &PgPool, service: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE services SET provision_status = 'pending' WHERE id = $1").bind(service).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn provision_status(pool: &PgPool, service: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar("SELECT provision_status FROM services WHERE id = $1").bind(service).fetch_optional(pool).await.map_err(db_err)
}

// ---- module data ----------------------------------------------------------

/// Shallow-merge `patch` into `services.module_data` (null values delete).
pub async fn patch_module_data(pool: &PgPool, service: Uuid, patch: &Value) -> AppResult<()> {
    let Some(obj) = patch.as_object() else { return Ok(()) };
    let set: serde_json::Map<String, Value> = obj.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect();
    let del: Vec<String> = obj.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect();
    sqlx::query("UPDATE services SET module_data = (module_data - $3::text[]) || $2 WHERE id = $1").bind(service).bind(Value::Object(set)).bind(&del).execute(pool).await.map_err(db_err)?;
    Ok(())
}

// ---- licensing ------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct LicenseRow {
    pub service_id: Uuid,
    pub status: String,
    pub product_name: String,
    pub product_config: Value,
    pub module_data: Value,
    pub server_id: Option<Uuid>,
    pub next_due_date: Option<time::Date>,
}

pub async fn license(pool: &PgPool, key: &str) -> AppResult<Option<LicenseRow>> {
    sqlx::query_as(
        "SELECT s.id AS service_id, s.status, p.name AS product_name, p.module_config AS product_config, s.module_data,
                COALESCE(s.server_id, p.server_id) AS server_id, s.next_due_date
           FROM services s JOIN products p ON p.id = s.product_id
          WHERE s.remote_id = $1 AND p.module = 'licensing' LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_nested() {
        let v = serde_json::json!({"password": "x", "a": [{"root_pass": "y", "ok": 1}], "url": "u", "ticket": null});
        let r = redact(&v);
        assert_eq!(r["password"], "[redacted]");
        assert_eq!(r["a"][0]["root_pass"], "[redacted]");
        assert_eq!(r["a"][0]["ok"], 1);
        assert_eq!(r["url"], "u");
        assert!(r["ticket"].is_null());
    }
}
