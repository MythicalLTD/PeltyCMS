//! Installed-addon registry (see `make docs`).
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct InstalledAddon {
    pub id: String,
    pub kind: String,
    pub engine: String,
    pub version: String,
    pub source: String,
    pub enabled: bool,
    pub settings: Value,
    pub permissions: Vec<String>,
    pub manifest_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub installed_at: OffsetDateTime,
}

const COLS: &str = "id, kind, engine, version, source, enabled, settings, permissions, manifest_hash, installed_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<InstalledAddon>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM installed_addons ORDER BY id")).fetch_all(pool).await.map_err(db_err)
}

pub async fn get(pool: &PgPool, id: &str) -> AppResult<Option<InstalledAddon>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM installed_addons WHERE id = $1")).bind(id).fetch_optional(pool).await.map_err(db_err)
}

/// True when an addon of `kind` providing `engine` is installed and enabled.
pub async fn engine_enabled(pool: &PgPool, kind: &str, engine: &str) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM installed_addons WHERE kind = $1 AND engine = $2 AND enabled)")
        .bind(kind)
        .bind(engine)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Settings of the first enabled addon of `kind` providing `engine`.
pub async fn engine_settings(pool: &PgPool, kind: &str, engine: &str) -> AppResult<Option<Value>> {
    sqlx::query_scalar("SELECT settings FROM installed_addons WHERE kind = $1 AND engine = $2 AND enabled ORDER BY id LIMIT 1")
        .bind(kind)
        .bind(engine)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub struct NewAddon<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub engine: &'a str,
    pub version: &'a str,
    pub source: &'a str,
    pub settings: &'a Value,
    pub permissions: &'a [String],
    pub manifest_hash: &'a str,
}

/// Insert the registry row and run the addon's SQL migrations atomically.
/// `migrations` = (name, checksum, sql).
pub async fn install(pool: &PgPool, a: &NewAddon<'_>, migrations: &[(String, String, String)]) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query(
        "INSERT INTO installed_addons (id, kind, engine, version, source, enabled, settings, permissions, manifest_hash)
         VALUES ($1,$2,$3,$4,$5,false,$6,$7,$8)",
    )
    .bind(a.id)
    .bind(a.kind)
    .bind(a.engine)
    .bind(a.version)
    .bind(a.source)
    .bind(a.settings)
    .bind(a.permissions)
    .bind(a.manifest_hash)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    for (name, sum, sql) in migrations {
        let done: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM addon_migrations WHERE addon_id = $1 AND name = $2)")
            .bind(a.id)
            .bind(name)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        if done {
            continue;
        }
        sqlx::Executor::execute(&mut *tx, sql.as_str()).await.map_err(db_err)?;
        sqlx::query("INSERT INTO addon_migrations (addon_id, name, checksum) VALUES ($1,$2,$3)")
            .bind(a.id)
            .bind(name)
            .bind(sum)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)
}

pub async fn set_enabled(pool: &PgPool, id: &str, enabled: bool) -> AppResult<bool> {
    let r = sqlx::query("UPDATE installed_addons SET enabled = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(enabled)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn set_settings(pool: &PgPool, id: &str, settings: &Value) -> AppResult<bool> {
    let r = sqlx::query("UPDATE installed_addons SET settings = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(settings)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn remove(pool: &PgPool, id: &str) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Products whose provisioning module is `engine`.
pub async fn products_using_module(pool: &PgPool, engine: &str) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM products WHERE module = $1").bind(engine).fetch_one(pool).await.map_err(db_err)
}

/// TLDs priced with, plus live domains held at, registrar `engine`.
pub async fn registrar_in_use(pool: &PgPool, engine: &str) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM tld_pricing WHERE registrar = $1)
              + (SELECT count(*) FROM domains WHERE registrar = $1 AND status NOT IN ('cancelled','failed','expired','transferred_away'))",
    )
    .bind(engine)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Disable every payment_gateways row served by `engine` (custom-* for custom_http).
pub async fn disable_gateway_rows(pool: &PgPool, engine: &str, custom_prefix: &str) -> AppResult<()> {
    sqlx::query("UPDATE payment_gateways SET enabled = false WHERE id = $1 OR ($1 = 'custom_http' AND starts_with(id, $2))")
        .bind(engine)
        .bind(custom_prefix)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Names of migrations already applied for an addon.
pub async fn applied_migrations(pool: &PgPool, id: &str) -> AppResult<Vec<String>> {
    sqlx::query_scalar("SELECT name FROM addon_migrations WHERE addon_id = $1 ORDER BY name").bind(id).fetch_all(pool).await.map_err(db_err)
}

/// Recorded checksums of migrations already applied; no SQL is executed.
pub async fn applied_migration_checksums(pool: &PgPool, id: &str) -> AppResult<std::collections::BTreeMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT name, checksum FROM addon_migrations WHERE addon_id = $1 ORDER BY name")
        .bind(id).fetch_all(pool).await.map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

/// Record an in-place package upgrade (new version + manifest hash).
pub async fn set_package(pool: &PgPool, id: &str, version: &str, manifest_hash: &str) -> AppResult<bool> {
    let r = sqlx::query("UPDATE installed_addons SET version = $2, manifest_hash = $3 WHERE id = $1").bind(id).bind(version).bind(manifest_hash).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Map a row installed by an older release (source `builtin`) to the addon
/// package that now provides it; enabled state and settings are kept.
pub async fn adopt_package(pool: &PgPool, id: &str, version: &str, manifest_hash: &str, source: &str, permissions: &[String]) -> AppResult<bool> {
    let r = sqlx::query("UPDATE installed_addons SET version = $2, manifest_hash = $3, source = $4, permissions = $5 WHERE id = $1")
        .bind(id)
        .bind(version)
        .bind(manifest_hash)
        .bind(source)
        .bind(permissions)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}
