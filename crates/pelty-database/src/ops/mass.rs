//! Row updates behind admin list mass actions (customers, domains).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use sqlx::PgPool;
use uuid::Uuid;

pub const CUSTOMER_STATUSES: [&str; 3] = ["active", "inactive", "closed"];

pub async fn customer_status(pool: &PgPool, id: Uuid, status: &str) -> AppResult<()> {
    if !CUSTOMER_STATUSES.contains(&status) {
        return Err(AppError::Validation("error.invalid_status"));
    }
    let r = sqlx::query("UPDATE customers SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id).bind(status).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 { Err(AppError::NotFound) } else { Ok(()) }
}

pub async fn domain_auto_renew(pool: &PgPool, id: Uuid, on: bool) -> AppResult<()> {
    let r = sqlx::query("UPDATE domains SET auto_renew = $2, updated_at = now() WHERE id = $1")
        .bind(id).bind(on).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 { Err(AppError::NotFound) } else { Ok(()) }
}
