//! Provisioning persistence: panel servers (API keys sealed at rest), the
//! per-service remote state, and enqueueing module actions as durable jobs.
use crate::db_err;
use crate::jobs;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const JOB_KIND: &str = "provision";
pub const MAX_ATTEMPTS: i32 = 6;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Server {
    pub id: Uuid,
    pub name: String,
    pub module: String,
    pub base_url: String,
    pub config: Value,
    pub enabled: bool,
    pub max_services: Option<i32>,
    pub services: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "v.id, v.name, v.module, v.base_url, v.config, v.enabled, v.max_services, v.created_at,
    (SELECT count(*) FROM services s WHERE s.server_id = v.id AND s.status IN ('pending','active','suspended')) AS services";

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ServerInput {
    pub name: String,
    pub module: String,
    pub base_url: String,
    /// Required on create; omit on update to keep the stored key.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "empty_obj")]
    pub config: Value,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub max_services: Option<i32>,
}

fn empty_obj() -> Value {
    json!({})
}
fn yes() -> bool {
    true
}

pub fn valid_base_url(u: &str) -> bool {
    let u = u.trim();
    (u.starts_with("https://") || u.starts_with("http://"))
        && u.len() <= 500
        && !u.contains(char::is_whitespace)
        && u.len() > "http://".len() + 1
}

pub async fn list_servers(pool: &PgPool) -> AppResult<Vec<Server>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM servers v ORDER BY v.name, v.id"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get_server(pool: &PgPool, id: Uuid) -> AppResult<Option<Server>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM servers v WHERE v.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Sealed key + base url + module + config, for the worker only.
pub async fn server_secret(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<Option<(String, String, Vec<u8>, Value, bool)>> {
    sqlx::query_as(
        "SELECT module, base_url, api_key_sealed, config, enabled FROM servers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

fn check(i: &ServerInput) -> AppResult<()> {
    if i.name.trim().is_empty()
        || i.name.len() > 120
        || !valid_base_url(&i.base_url)
        || !i.config.is_object()
        || i.max_services.is_some_and(|m| m <= 0)
    {
        return Err(AppError::Validation("error.invalid_server"));
    }
    Ok(())
}

pub async fn create_server(
    pool: &PgPool,
    i: &ServerInput,
    sealed_key: Vec<u8>,
) -> AppResult<Server> {
    check(i)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO servers (id, name, module, base_url, api_key_sealed, config, enabled, max_services) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(id).bind(i.name.trim()).bind(&i.module).bind(i.base_url.trim().trim_end_matches('/')).bind(sealed_key).bind(&i.config).bind(i.enabled).bind(i.max_services)
        .execute(pool).await.map_err(db_err)?;
    get_server(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn update_server(
    pool: &PgPool,
    id: Uuid,
    i: &ServerInput,
    sealed_key: Option<Vec<u8>>,
) -> AppResult<Server> {
    check(i)?;
    let n = sqlx::query(
        "UPDATE servers SET name=$2, module=$3, base_url=$4, api_key_sealed=COALESCE($5, api_key_sealed), config=$6, enabled=$7, max_services=$8 WHERE id=$1",
    )
    .bind(id).bind(i.name.trim()).bind(&i.module).bind(i.base_url.trim().trim_end_matches('/')).bind(sealed_key).bind(&i.config).bind(i.enabled).bind(i.max_services)
    .execute(pool).await.map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    get_server(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn delete_server(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let used: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM services WHERE server_id = $1 AND status IN ('pending','active','suspended'))")
        .bind(id).fetch_one(pool).await.map_err(db_err)?;
    if used {
        return Err(AppError::Conflict("error.server_in_use"));
    }
    let n = sqlx::query("DELETE FROM servers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Bind a product to a module (+ optional default server and module config).
pub async fn set_product_module(
    pool: &PgPool,
    product: Uuid,
    module: Option<&str>,
    server: Option<Uuid>,
    config: &Value,
) -> AppResult<()> {
    if !config.is_object() {
        return Err(AppError::Validation("error.invalid_server"));
    }
    let n = sqlx::query(
        "UPDATE products SET module = $2, server_id = $3, module_config = $4 WHERE id = $1",
    )
    .bind(product)
    .bind(module)
    .bind(server)
    .bind(config)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// One module-declared checkout field (see `pelty_provisioning::CheckoutField`).
pub struct ModuleField<'a> {
    pub key: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub kind: &'a str,
    pub choices: Vec<String>,
    pub pattern: &'a str,
    pub required: bool,
}

/// Make the product's module-tagged custom fields match `fields` exactly:
/// upsert by key, drop keys the (new) module no longer declares. Values of
/// admin-made fields are untouched.
pub async fn sync_module_fields(
    pool: &PgPool,
    product: Uuid,
    fields: &[ModuleField<'_>],
) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let keys: Vec<&str> = fields.iter().map(|f| f.key).collect();
    sqlx::query("DELETE FROM product_custom_fields WHERE product_id = $1 AND module_key IS NOT NULL AND NOT (module_key = ANY($2))")
        .bind(product).bind(&keys).execute(&mut *tx).await.map_err(db_err)?;
    for (i, f) in fields.iter().enumerate() {
        sqlx::query(
            "INSERT INTO product_custom_fields (id, product_id, name, description, kind, choices, pattern, required, admin_only, sort_order, module_key)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,FALSE,$9,$10)
             ON CONFLICT (product_id, module_key) WHERE module_key IS NOT NULL DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
               kind = EXCLUDED.kind, choices = EXCLUDED.choices, pattern = EXCLUDED.pattern, required = EXCLUDED.required, admin_only = FALSE",
        )
        .bind(Uuid::new_v4()).bind(product).bind(f.name).bind(f.description).bind(f.kind)
        .bind(serde_json::json!(f.choices)).bind(f.pattern).bind(f.required).bind(-1000 + i as i32).bind(f.key)
        .execute(&mut *tx).await.map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

pub async fn product_module(
    pool: &PgPool,
    product: Uuid,
) -> AppResult<Option<(Option<String>, Option<Uuid>, Value)>> {
    sqlx::query_as("SELECT module, server_id, module_config FROM products WHERE id = $1")
        .bind(product)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Everything the worker needs for one service.
#[derive(Debug, Clone, FromRow)]
pub struct ServiceTarget {
    pub service_id: Uuid,
    pub status: String,
    pub module: Option<String>,
    pub product_config: Value,
    pub server_id: Option<Uuid>,
    pub server_group_id: Option<Uuid>,
    pub remote_id: Option<String>,
    pub module_data: Value,
    pub custom_fields: Value,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
}

pub async fn target(pool: &PgPool, service: Uuid) -> AppResult<Option<ServiceTarget>> {
    sqlx::query_as(
        "SELECT s.id AS service_id, s.status, p.module, p.module_config AS product_config,
                COALESCE(s.server_id, p.server_id) AS server_id, p.server_group_id, s.remote_id, s.module_data, COALESCE(oi.custom_fields, '{}'::jsonb) || COALESCE((SELECT jsonb_object_agg(f.module_key, oi.custom_fields->(f.id::text)) FROM product_custom_fields f
                    WHERE f.product_id = p.id AND f.module_key IS NOT NULL AND oi.custom_fields ? (f.id::text)), '{}'::jsonb) AS custom_fields,
                c.email::text AS email, c.first_name, c.last_name
         FROM services s JOIN products p ON p.id = s.product_id JOIN customers c ON c.id = s.customer_id
         LEFT JOIN order_items oi ON oi.id = s.order_item_id
         WHERE s.id = $1",
    )
    .bind(service)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Queue a module action for a service if its product uses a module.
/// Returns the job id (None when the product has no module or a duplicate
/// job with the same key exists).
pub async fn enqueue(
    pool: &PgPool,
    service: Uuid,
    action: &str,
    key_suffix: &str,
) -> AppResult<Option<i64>> {
    let has_module: bool = sqlx::query_scalar("SELECT p.module IS NOT NULL FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1")
        .bind(service).fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound)?;
    if !has_module {
        return Ok(None);
    }
    sqlx::query(
        "UPDATE services SET provision_status = 'pending', provision_error = '' WHERE id = $1",
    )
    .bind(service)
    .execute(pool)
    .await
    .map_err(db_err)?;
    let key = format!("provision:{service}:{action}:{key_suffix}");
    jobs::enqueue(
        pool,
        JOB_KIND,
        json!({ "service_id": service, "action": action }),
        None,
        Some(&key),
        MAX_ATTEMPTS,
    )
    .await
}

pub async fn record_success(
    pool: &PgPool,
    service: Uuid,
    server: Option<Uuid>,
    created: Option<(&str, &Value)>,
) -> AppResult<()> {
    match created {
        Some((rid, data)) => sqlx::query(
            "UPDATE services SET provision_status='ok', provision_error='', remote_id=$2, module_data=$3, server_id=COALESCE(server_id,$4), provisioned_at=COALESCE(provisioned_at, now()) WHERE id=$1",
        )
        .bind(service).bind(rid).bind(data).bind(server),
        None => sqlx::query("UPDATE services SET provision_status='ok', provision_error='' WHERE id=$1").bind(service),
    }
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn record_failure(pool: &PgPool, service: Uuid, error: &str) -> AppResult<()> {
    let e: String = error.chars().take(2000).collect();
    sqlx::query("UPDATE services SET provision_status='failed', provision_error=$2 WHERE id=$1")
        .bind(service)
        .bind(e)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProvisionState {
    pub server_id: Option<Uuid>,
    pub server_name: Option<String>,
    pub module: Option<String>,
    pub remote_id: Option<String>,
    pub provision_status: String,
    pub provision_error: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub provisioned_at: Option<OffsetDateTime>,
}

pub async fn state(pool: &PgPool, service: Uuid) -> AppResult<Option<ProvisionState>> {
    sqlx::query_as(
        "SELECT COALESCE(s.server_id, p.server_id) AS server_id, v.name AS server_name, p.module, s.remote_id, s.provision_status, s.provision_error, s.provisioned_at
         FROM services s JOIN products p ON p.id = s.product_id LEFT JOIN servers v ON v.id = COALESCE(s.server_id, p.server_id) WHERE s.id = $1",
    )
    .bind(service)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Recent provisioning jobs for a service (newest first).
pub async fn jobs_for(pool: &PgPool, service: Uuid) -> AppResult<Vec<Value>> {
    let rows: Vec<(i64, Value, String, i32, Option<String>, OffsetDateTime)> = sqlx::query_as(
        "SELECT id, payload, status, attempts, last_error, created_at FROM jobs WHERE kind = $1 AND payload->>'service_id' = $2 ORDER BY id DESC LIMIT 20",
    )
    .bind(JOB_KIND).bind(service.to_string()).fetch_all(pool).await.map_err(db_err)?;
    Ok(rows.into_iter().map(|(id, p, status, attempts, err, at)| json!({
        "id": id, "action": p["action"], "status": status, "attempts": attempts, "last_error": err,
        "created_at": at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
    })).collect())
}

/// Last attempt error (status stays `pending` while retries remain).
pub async fn set_error(pool: &PgPool, service: Uuid, error: &str) -> AppResult<()> {
    let e: String = error.chars().take(2000).collect();
    sqlx::query("UPDATE services SET provision_error = $2 WHERE id = $1")
        .bind(service)
        .bind(e)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

// ----- server groups -----

pub const GROUP_STRATEGIES: &[&str] = &["least_services", "fill", "round_robin"];

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ServerGroup {
    pub id: Uuid,
    pub name: String,
    pub strategy: String,
    pub servers: i64,
    pub products: i64,
}

pub async fn list_groups(pool: &PgPool) -> AppResult<Vec<ServerGroup>> {
    sqlx::query_as(
        "SELECT g.id, g.name, g.strategy,
                (SELECT count(*) FROM servers v WHERE v.group_id = g.id) AS servers,
                (SELECT count(*) FROM products p WHERE p.server_group_id = g.id) AS products
         FROM server_groups g ORDER BY g.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn upsert_group(
    pool: &PgPool,
    id: Option<Uuid>,
    name: &str,
    strategy: &str,
) -> AppResult<Uuid> {
    match id {
        Some(id) => {
            let n = sqlx::query("UPDATE server_groups SET name = $2, strategy = $3 WHERE id = $1")
                .bind(id)
                .bind(name)
                .bind(strategy)
                .execute(pool)
                .await
                .map_err(db_err)?;
            if n.rows_affected() == 0 {
                return Err(AppError::NotFound);
            }
            Ok(id)
        }
        None => {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO server_groups (id, name, strategy) VALUES ($1,$2,$3)")
                .bind(id)
                .bind(name)
                .bind(strategy)
                .execute(pool)
                .await
                .map_err(db_err)?;
            Ok(id)
        }
    }
}

pub async fn delete_group(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    Ok(sqlx::query("DELETE FROM server_groups WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected()
        == 1)
}

/// Replace the member list of a group (servers not listed leave it).
pub async fn set_group_servers(pool: &PgPool, id: Uuid, servers: &[Uuid]) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM server_groups WHERE id = $1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    if !exists {
        return Err(AppError::NotFound);
    }
    sqlx::query("UPDATE servers SET group_id = NULL WHERE group_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE servers SET group_id = $1 WHERE id = ANY($2)")
        .bind(id)
        .bind(servers)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

pub async fn group_servers(pool: &PgPool, id: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM servers WHERE group_id = $1 ORDER BY name")
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn set_product_group(
    pool: &PgPool,
    product: Uuid,
    group: Option<Uuid>,
) -> AppResult<bool> {
    Ok(sqlx::query("UPDATE products SET server_group_id = $2 WHERE id = $1 AND ($2::uuid IS NULL OR EXISTS (SELECT 1 FROM server_groups WHERE id = $2))").bind(product).bind(group).execute(pool).await.map_err(db_err)?.rows_affected() == 1)
}

pub async fn product_group(pool: &PgPool, product: Uuid) -> AppResult<Option<Uuid>> {
    Ok(
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT server_group_id FROM products WHERE id = $1")
            .bind(product)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .flatten(),
    )
}

/// Pick a server for a service whose product is bound to a group and assign
/// it to the service (sticky across retries). Serialised per group with a row
/// lock so concurrent orders don't overfill one server. Returns None when the
/// product has no group or no member has capacity.
pub async fn assign_from_group(
    pool: &PgPool,
    service: Uuid,
    module: &str,
) -> AppResult<Option<Uuid>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT s.server_id, p.server_group_id FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1 FOR UPDATE OF s",
    )
    .bind(service)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some((current, Some(group))) = row else {
        return Ok(None);
    };
    if current.is_some() {
        return Ok(current);
    }
    let g: Option<(String, i64)> =
        sqlx::query_as("SELECT strategy, rr_cursor FROM server_groups WHERE id = $1 FOR UPDATE")
            .bind(group)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let Some((strategy, cursor)) = g else {
        return Ok(None);
    };
    let candidates: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT v.id, (SELECT count(*) FROM services s WHERE s.server_id = v.id AND s.status IN ('pending','active','suspended')) AS used
         FROM servers v
         WHERE v.group_id = $1 AND v.enabled AND v.module = $2
           AND (v.max_services IS NULL OR (SELECT count(*) FROM services s WHERE s.server_id = v.id AND s.status IN ('pending','active','suspended')) < v.max_services)
         ORDER BY v.created_at, v.id",
    )
    .bind(group)
    .bind(module)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some(pick) = pick_server(&strategy, cursor, &candidates) else {
        return Ok(None);
    };
    if strategy == "round_robin" {
        sqlx::query("UPDATE server_groups SET rr_cursor = rr_cursor + 1 WHERE id = $1")
            .bind(group)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    sqlx::query("UPDATE services SET server_id = $2 WHERE id = $1")
        .bind(service)
        .bind(pick)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(pick))
}

/// Pure selection over (server, used) candidates already filtered for capacity.
pub fn pick_server(strategy: &str, cursor: i64, c: &[(Uuid, i64)]) -> Option<Uuid> {
    if c.is_empty() {
        return None;
    }
    match strategy {
        // Fill the busiest server with room first (first in order on ties).
        "fill" => c
            .iter()
            .enumerate()
            .max_by_key(|(i, (_, u))| (*u, std::cmp::Reverse(*i)))
            .map(|(_, (id, _))| *id),
        "round_robin" => Some(c[(cursor.rem_euclid(c.len() as i64)) as usize].0),
        // least_services: emptiest first (first in order on ties).
        _ => c
            .iter()
            .enumerate()
            .min_by_key(|(i, (_, u))| (*u, *i))
            .map(|(_, (id, _))| *id),
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn strategies() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let cs = [(a, 3), (b, 1), (c, 1)];
        assert_eq!(pick_server("least_services", 0, &cs), Some(b));
        assert_eq!(pick_server("fill", 0, &cs), Some(a));
        assert_eq!(pick_server("round_robin", 0, &cs), Some(a));
        assert_eq!(pick_server("round_robin", 4, &cs), Some(b));
        assert_eq!(pick_server("fill", 0, &[(b, 2), (c, 2)]), Some(b));
        assert_eq!(pick_server("least_services", 0, &[]), None);
    }
}
