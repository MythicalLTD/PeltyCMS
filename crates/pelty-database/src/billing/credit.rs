//! Customer credit: an append-only ledger per currency. Staff adjust it;
//! credit is spent by applying it to an unpaid invoice, which records a
//! `credit` gateway payment through the normal (exactly-once) payment path.
use crate::{db_err, invoices::{self, Applied, Tx}};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const GATEWAY: &str = "credit";

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Entry {
    pub id: Uuid,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub kind: String,
    pub invoice_id: Option<Uuid>,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Balance {
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
}

pub async fn balances(pool: &PgPool, customer: Uuid) -> AppResult<Vec<Balance>> {
    sqlx::query_as("SELECT currency::text AS currency, sum(amount) AS amount FROM credit_ledger WHERE customer_id = $1 GROUP BY currency HAVING sum(amount) <> 0 ORDER BY currency")
        .bind(customer)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn entries(pool: &PgPool, customer: Uuid, limit: i64) -> AppResult<Vec<Entry>> {
    sqlx::query_as("SELECT id, currency::text AS currency, amount, kind, invoice_id, note, created_at FROM credit_ledger WHERE customer_id = $1 ORDER BY created_at DESC, id LIMIT $2")
        .bind(customer)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Lock the customer (serializes balance changes) and read one balance.
async fn locked_balance(tx: &mut Tx<'_>, customer: Uuid, currency: &str) -> AppResult<Decimal> {
    sqlx::query("SELECT 1 FROM customers WHERE id = $1 FOR UPDATE")
        .bind(customer)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    sqlx::query_scalar("SELECT COALESCE(sum(amount), 0) FROM credit_ledger WHERE customer_id = $1 AND currency = $2")
        .bind(customer)
        .bind(currency)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)
}

/// Staff adjustment (+ adds credit, − removes it). Returns the new balance.
pub async fn adjust(pool: &PgPool, customer: Uuid, currency: &str, amount: Decimal, note: &str, actor: Option<Uuid>) -> AppResult<Decimal> {
    if note.chars().count() > 500 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let currency = currency.trim().to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exp = invoices::exponent(&mut tx, &currency).await?;
    if amount.is_zero() || amount.normalize().scale() > exp || amount.abs() > Decimal::from(1_000_000_000) {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    let bal = locked_balance(&mut tx, customer, &currency).await?;
    if bal + amount < Decimal::ZERO {
        return Err(AppError::Conflict("error.insufficient_credit"));
    }
    sqlx::query("INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, note, actor_id) VALUES ($1,$2,$3,$4,'adjustment',$5,$6)")
        .bind(Uuid::new_v4())
        .bind(customer)
        .bind(&currency)
        .bind(amount)
        .bind(note.trim())
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(bal + amount)
}

#[derive(Debug, Clone, Serialize)]
pub struct CreditApplied {
    pub applied: Applied,
    pub amount: Decimal,
    pub balance: Decimal,
}

/// Apply up to `amount` (default: as much as possible) of the customer's
/// credit to their unpaid invoice.
pub async fn apply_to_invoice(pool: &PgPool, customer: Uuid, invoice_id: Uuid, amount: Option<Decimal>, actor: Option<Uuid>) -> AppResult<CreditApplied> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let inv: Option<(String, String, String, Decimal, Decimal)> = sqlx::query_as(
        "SELECT status, kind, currency::text, total, amount_paid FROM invoices WHERE id = $1 AND customer_id = $2",
    )
    .bind(invoice_id)
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (status, kind, currency, total, paid) = inv.ok_or(AppError::NotFound)?;
    if status != "unpaid" || kind != "invoice" {
        return Err(AppError::Conflict("error.invoice_not_payable"));
    }
    // A deposit can't be paid with the credit it creates.
    let deposit: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM add_funds WHERE invoice_id = $1)")
        .bind(invoice_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    if deposit {
        return Err(AppError::Conflict("error.add_funds_no_credit"));
    }
    let bal = locked_balance(&mut tx, customer, &currency).await?;
    let due = total - paid;
    let exp = invoices::exponent(&mut tx, &currency).await?;
    if let Some(a) = amount {
        if a <= Decimal::ZERO || a.normalize().scale() > exp {
            return Err(AppError::Validation("error.invalid_amount"));
        }
        if a > bal {
            return Err(AppError::Conflict("error.insufficient_credit"));
        }
    }
    let amt = amount.unwrap_or(bal).min(bal).min(due);
    if amt <= Decimal::ZERO {
        return Err(AppError::Conflict("error.insufficient_credit"));
    }
    let pid = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO payments (id, user_id, gateway, amount, currency, status, idempotency_key, invoice_id, customer_id)
         VALUES ($1,$2,$3,$4,$5,'pending',$6,$7,$8)",
    )
    .bind(pid)
    .bind(actor)
    .bind(GATEWAY)
    .bind(amt)
    .bind(&currency)
    .bind(format!("credit:{pid}"))
    .bind(invoice_id)
    .bind(customer)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let applied = invoices::apply_locked(&mut tx, pid).await?;
    sqlx::query("INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, invoice_id, payment_id, actor_id) VALUES ($1,$2,$3,$4,'applied',$5,$6,$7)")
        .bind(Uuid::new_v4())
        .bind(customer)
        .bind(&currency)
        .bind(-amt)
        .bind(invoice_id)
        .bind(pid)
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(CreditApplied { applied, amount: amt, balance: bal - amt })
}

/// Return refunded credit-balance money to the ledger (inside the refund tx).
pub(crate) async fn return_locked(tx: &mut Tx<'_>, customer: Uuid, currency: &str, amount: Decimal, invoice: Uuid, payment: Uuid) -> AppResult<()> {
    locked_balance(tx, customer, currency).await?;
    sqlx::query("INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, invoice_id, payment_id, note) VALUES ($1,$2,$3,$4,'adjustment',$5,$6,'refund')")
        .bind(Uuid::new_v4())
        .bind(customer)
        .bind(currency)
        .bind(amount)
        .bind(invoice)
        .bind(payment)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}
