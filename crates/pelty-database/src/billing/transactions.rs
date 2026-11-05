//! Admin transactions list (all gateway payments) with filters and totals.

use crate::db_err;
use pelty_core::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub gateway: Option<String>,
    pub status: Option<String>,
    pub currency: Option<String>,
    pub customer: Option<Uuid>,
    pub from: Option<OffsetDateTime>,
    pub to: Option<OffsetDateTime>,
    pub q: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Transaction {
    pub id: Uuid,
    pub gateway: String,
    pub provider_ref: Option<String>,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub fee: Option<Decimal>,
    pub currency: String,
    pub status: String,
    pub invoice_id: Option<Uuid>,
    pub invoice_number: Option<String>,
    pub customer_id: Option<Uuid>,
    pub customer_email: Option<String>,
    pub admin_note: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Total {
    pub currency: String,
    pub status: String,
    pub count: i64,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
}

const WHERE: &str = "FROM payments p LEFT JOIN invoices i ON i.id = p.invoice_id LEFT JOIN customers c ON c.id = p.customer_id
 WHERE ($1::text IS NULL OR p.gateway = $1) AND ($2::text IS NULL OR p.status = $2) AND ($3::text IS NULL OR p.currency = $3)
   AND ($4::uuid IS NULL OR p.customer_id = $4) AND ($5::timestamptz IS NULL OR p.created_at >= $5) AND ($6::timestamptz IS NULL OR p.created_at < $6)
   AND ($7::text IS NULL OR p.provider_ref ILIKE '%' || $7 || '%' OR i.number ILIKE '%' || $7 || '%' OR c.email ILIKE '%' || $7 || '%')";

macro_rules! bind_filter {
    ($q:expr, $f:expr) => {
        $q.bind($f.gateway.as_deref())
            .bind($f.status.as_deref())
            .bind($f.currency.as_deref())
            .bind($f.customer)
            .bind($f.from)
            .bind($f.to)
            .bind($f.q.as_deref().map(|s| s.replace(['%', '_'], "")))
    };
}

pub async fn list(
    pool: &PgPool,
    f: &Filter,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Transaction>, i64, Vec<Total>)> {
    let sql = format!(
        "SELECT p.id, p.gateway, p.provider_ref, p.amount, p.fee, p.currency::text AS currency, p.status, p.invoice_id, i.number AS invoice_number,
                p.customer_id, c.email AS customer_email, p.admin_note, p.created_at {WHERE} ORDER BY p.created_at DESC, p.id LIMIT $8 OFFSET $9"
    );
    let items: Vec<Transaction> = bind_filter!(sqlx::query_as(&sql), f)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let csql = format!("SELECT count(*) {WHERE}");
    let (count,): (i64,) = bind_filter!(sqlx::query_as(&csql), f)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let tsql = format!("SELECT p.currency::text AS currency, p.status, count(*) AS count, COALESCE(sum(p.amount), 0) AS amount {WHERE} GROUP BY 1, 2 ORDER BY 1, 2");
    let totals: Vec<Total> = bind_filter!(sqlx::query_as(&tsql), f)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok((items, count, totals))
}

/// WHMCS `UpdateTransaction`: only the staff note is editable; amounts,
/// status and references come from the gateway and stay immutable.
pub async fn set_note(pool: &PgPool, id: Uuid, note: Option<&str>) -> AppResult<()> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > 2000) {
        return Err(pelty_core::AppError::Validation("error.too_long"));
    }
    let r = sqlx::query("UPDATE payments SET admin_note = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(note)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(pelty_core::AppError::NotFound);
    }
    Ok(())
}
