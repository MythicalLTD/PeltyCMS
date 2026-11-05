//! Downloads area. Bytes live in the database; access requires an active
//! service of a linked product (or no linked products at all).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const MAX_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Download {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub product_ids: Vec<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "d.id, d.title, d.description, d.filename, d.content_type, d.size_bytes,
    COALESCE((SELECT array_agg(product_id) FROM download_products WHERE download_id = d.id), '{}') AS product_ids, d.created_at";

/// SQL predicate: customer $1 may access download d.
const ALLOWED: &str = "(NOT EXISTS (SELECT 1 FROM download_products dp WHERE dp.download_id = d.id)
    OR EXISTS (SELECT 1 FROM download_products dp JOIN services s ON s.product_id = dp.product_id
               WHERE dp.download_id = d.id AND s.customer_id = $1 AND s.status = 'active'))";

pub struct NewDownload<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub filename: &'a str,
    pub content_type: &'a str,
    pub sha256: &'a [u8],
    pub data: &'a [u8],
    pub product_ids: &'a [Uuid],
}

pub async fn create(pool: &PgPool, n: NewDownload<'_>) -> AppResult<Uuid> {
    if n.title.trim().is_empty()
        || n.title.len() > 200
        || n.description.len() > 2000
        || n.product_ids.len() > 200
    {
        return Err(AppError::Validation("error.validation"));
    }
    if n.data.is_empty() || n.data.len() > pelty_core::config::upload_limits().download {
        return Err(AppError::BadRequest("error.download.too_large"));
    }
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query(
        "INSERT INTO downloads (id, title, description, filename, content_type, size_bytes, sha256, data) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(n.title.trim())
    .bind(n.description)
    .bind(n.filename)
    .bind(n.content_type)
    .bind(n.data.len() as i64)
    .bind(n.sha256)
    .bind(n.data)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    for p in n.product_ids {
        sqlx::query("INSERT INTO download_products (download_id, product_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(p)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

pub async fn list_all(pool: &PgPool) -> AppResult<Vec<Download>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM downloads d ORDER BY d.created_at DESC"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn list_for(pool: &PgPool, customer: Uuid) -> AppResult<Vec<Download>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM downloads d WHERE {ALLOWED} ORDER BY d.title"
    )))
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// (filename, content_type, bytes) when `customer` (None = staff) may read it.
pub async fn fetch(
    pool: &PgPool,
    id: Uuid,
    customer: Option<Uuid>,
) -> AppResult<Option<(String, String, Vec<u8>)>> {
    let sql = match customer {
        Some(_) => format!("SELECT d.filename, d.content_type, d.data FROM downloads d WHERE d.id = $2 AND {ALLOWED}"),
        None => "SELECT d.filename, d.content_type, d.data FROM downloads d WHERE d.id = $2 AND $1::uuid IS NULL".to_string(),
    };
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(customer)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM downloads WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
