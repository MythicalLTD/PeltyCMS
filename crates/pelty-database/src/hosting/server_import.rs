//! Import accounts that already exist on a provisioning server as services
//! (WHMCS "Server Sync / Import"). Accounts come from the module's
//! `list_accounts`; this module only validates and writes.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

/// Live services on `server`: remote id → service id.
pub async fn linked(pool: &PgPool, server: Uuid) -> AppResult<Vec<(String, Uuid)>> {
    sqlx::query_as("SELECT remote_id, id FROM services WHERE server_id = $1 AND remote_id IS NOT NULL AND status NOT IN ('terminated','cancelled')")
        .bind(server)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct ImportItem {
    pub remote_id: String,
    pub customer_id: Uuid,
    pub product_id: Uuid,
    pub cycle: String,
    #[schema(value_type = String)]
    pub recurring_amount: Decimal,
    #[serde(default)]
    pub next_due_date: Option<Date>,
    /// `active` (default) or `suspended`.
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportResult {
    pub remote_id: String,
    pub service_id: Option<Uuid>,
    pub error: Option<&'static str>,
}

const CYCLES: &[&str] = &[
    "one_time",
    "monthly",
    "quarterly",
    "semiannual",
    "annual",
    "biennial",
    "triennial",
];

/// Import each item in its own transaction; failures are reported per item.
/// `known` = remote ids the module reported (anything else is refused).
pub async fn import(
    pool: &PgPool,
    server: Uuid,
    server_module: &str,
    known: &[String],
    items: &[ImportItem],
) -> AppResult<Vec<ImportResult>> {
    if items.is_empty() || items.len() > 200 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut out = Vec::new();
    for it in items {
        let res = one(pool, server, server_module, known, it).await;
        out.push(match res {
            Ok(id) => ImportResult {
                remote_id: it.remote_id.clone(),
                service_id: Some(id),
                error: None,
            },
            Err(AppError::Validation(k)) | Err(AppError::Conflict(k)) => ImportResult {
                remote_id: it.remote_id.clone(),
                service_id: None,
                error: Some(k),
            },
            Err(AppError::NotFound) => ImportResult {
                remote_id: it.remote_id.clone(),
                service_id: None,
                error: Some("error.not_found"),
            },
            Err(e) => return Err(e),
        });
    }
    Ok(out)
}

async fn one(
    pool: &PgPool,
    server: Uuid,
    server_module: &str,
    known: &[String],
    it: &ImportItem,
) -> AppResult<Uuid> {
    let rid = it.remote_id.trim();
    if rid.is_empty() || rid.len() > 200 || !known.iter().any(|k| k == rid) {
        return Err(AppError::Validation("error.import_unknown_account"));
    }
    let status = it.status.as_deref().unwrap_or("active");
    if !CYCLES.contains(&it.cycle.as_str())
        || !["active", "suspended"].contains(&status)
        || it.recurring_amount.is_sign_negative()
    {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    // Serialize imports per server so two admins can't link one account twice.
    sqlx::query("SELECT id FROM servers WHERE id = $1 FOR UPDATE")
        .bind(server)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    let module: Option<Option<String>> =
        sqlx::query_scalar("SELECT module FROM products WHERE id = $1")
            .bind(it.product_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    if module.ok_or(AppError::NotFound)?.as_deref() != Some(server_module) {
        return Err(AppError::Validation("error.import_product_module"));
    }
    let currency: String = sqlx::query_scalar("SELECT currency::text FROM customers WHERE id = $1")
        .bind(it.customer_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    let taken: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM services WHERE server_id = $1 AND remote_id = $2 AND status NOT IN ('terminated','cancelled'))")
        .bind(server).bind(rid).fetch_one(&mut *tx).await.map_err(db_err)?;
    if taken {
        return Err(AppError::Conflict("error.import_already_linked"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date, server_id, remote_id,
                               provision_status, provisioned_at, activated_at, imported_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'ok',now(),now(),now())",
    )
    .bind(id).bind(it.customer_id).bind(it.product_id).bind(status).bind(&it.cycle).bind(&currency).bind(it.recurring_amount)
    .bind(it.next_due_date.filter(|_| it.cycle != "one_time")).bind(server).bind(rid)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}
