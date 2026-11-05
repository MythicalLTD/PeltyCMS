//! Billable items: one-off charges (held, added to the client's next
//! renewal invoice, or invoiced immediately) and recurring charges invoiced
//! by the daily billing job.
use crate::db_err;
use crate::invoices::Tx;
use crate::invoices::{self, NewItem};
use pelty_billing::cycle;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

pub const ACTIONS: [&str; 4] = ["hold", "next_invoice", "invoice_now", "recurring"];

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct BillableInput {
    pub customer_id: Uuid,
    pub description: String,
    #[serde(default = "one")]
    #[schema(value_type = String)]
    pub quantity: Decimal,
    #[schema(value_type = String)]
    pub unit_price: Decimal,
    pub invoice_action: String,
    /// Required for `recurring` (e.g. monthly).
    #[serde(default)]
    pub recur_cycle: Option<String>,
    /// First invoice date for `recurring` (YYYY-MM-DD).
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub next_due_date: Option<Date>,
    /// Number of times to bill a recurring item (omit = until cancelled).
    #[serde(default)]
    pub recur_times: Option<i32>,
}
fn one() -> Decimal {
    Decimal::ONE
}

fn v() -> AppError {
    AppError::Validation("error.validation")
}

/// Pure validation (unit-tested).
pub fn check(b: &BillableInput) -> AppResult<()> {
    let n = b.description.trim().chars().count();
    if n == 0 || n > 500 || b.quantity <= Decimal::ZERO || b.quantity > Decimal::from(100_000) {
        return Err(v());
    }
    if b.unit_price < Decimal::ZERO || b.unit_price > Decimal::from(1_000_000_000) {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    if !ACTIONS.contains(&b.invoice_action.as_str()) {
        return Err(v());
    }
    if b.invoice_action == "recurring" {
        let c = b.recur_cycle.as_deref().unwrap_or("");
        if !cycle::is_valid(c)
            || c == "one_time"
            || b.next_due_date.is_none()
            || b.recur_times.is_some_and(|t| !(1..=1000).contains(&t))
        {
            return Err(v());
        }
    }
    Ok(())
}

pub async fn list(pool: &PgPool, customer: Option<Uuid>, limit: i64) -> AppResult<Value> {
    sqlx::query_scalar(
        "SELECT COALESCE(json_agg(row_to_json(x) ORDER BY x.created_at DESC), '[]'::json)::jsonb FROM (
           SELECT b.id, b.customer_id, c.email AS customer_email, b.description, b.quantity::text AS quantity,
                  b.unit_price::text AS unit_price, b.currency::text AS currency, b.invoice_action, b.recur_cycle,
                  b.next_due_date, b.recur_remaining, b.invoice_id, i.number AS invoice_number, b.invoiced_at, b.created_at
           FROM billable_items b JOIN customers c ON c.id = b.customer_id LEFT JOIN invoices i ON i.id = b.invoice_id
           WHERE ($1::uuid IS NULL OR b.customer_id = $1) ORDER BY b.created_at DESC LIMIT $2) x",
    )
    .bind(customer)
    .bind(limit.clamp(1, 500))
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

fn line(desc: &str, qty: Decimal, price: Decimal) -> NewItem {
    NewItem {
        description: desc.to_string(),
        quantity: qty,
        unit_price: price,
        tax_rate: Decimal::ZERO,
        service_id: None,
        period: None,
        auto_tax: true,
    }
}

/// Create an item; `invoice_now` also issues its invoice (returned).
pub async fn create(
    pool: &PgPool,
    by: Uuid,
    b: &BillableInput,
    due_days: i64,
) -> AppResult<(Uuid, Option<Uuid>)> {
    check(b)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let cur: Option<String> =
        sqlx::query_scalar("SELECT currency::text FROM customers WHERE id = $1")
            .bind(b.customer_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let cur = cur.ok_or(AppError::NotFound)?;
    let id = Uuid::new_v4();
    let recurring = b.invoice_action == "recurring";
    sqlx::query(
        "INSERT INTO billable_items (id, customer_id, description, quantity, unit_price, currency, invoice_action,
            recur_cycle, next_due_date, recur_remaining, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(b.customer_id)
    .bind(b.description.trim())
    .bind(b.quantity)
    .bind(b.unit_price)
    .bind(&cur)
    .bind(&b.invoice_action)
    .bind(if recurring { b.recur_cycle.as_deref() } else { None })
    .bind(if recurring { b.next_due_date } else { None })
    .bind(if recurring { b.recur_times } else { None })
    .bind(by)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut inv = None;
    if b.invoice_action == "invoice_now" {
        let i = invoices::create_draft(&mut tx, b.customer_id, &cur, None).await?;
        invoices::add_items(
            &mut tx,
            i,
            &cur,
            &[line(b.description.trim(), b.quantity, b.unit_price)],
        )
        .await?;
        invoices::finalize(&mut tx, i, due_days).await?;
        mark(&mut tx, &[id], i).await?;
        inv = Some(i);
    }
    tx.commit().await.map_err(db_err)?;
    Ok((id, inv))
}

async fn mark(tx: &mut Tx<'_>, ids: &[Uuid], invoice: Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE billable_items SET invoice_id = $2, invoiced_at = now() WHERE id = ANY($1)",
    )
    .bind(ids)
    .bind(invoice)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Delete an item that has not been invoiced (recurring items: always; it
/// stops future billing).
pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM billable_items WHERE id = $1 AND (invoice_id IS NULL OR invoice_action = 'recurring')")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

type Pending = (Uuid, String, Decimal, Decimal);

async fn pending(
    tx: &mut Tx<'_>,
    customer: Uuid,
    currency: &str,
    actions: &[&str],
) -> AppResult<Vec<Pending>> {
    let acts: Vec<String> = actions.iter().map(|s| s.to_string()).collect();
    sqlx::query_as(
        "SELECT id, description, quantity, unit_price FROM billable_items
         WHERE customer_id = $1 AND currency = $2 AND invoice_id IS NULL AND invoice_action = ANY($3)
         ORDER BY created_at FOR UPDATE SKIP LOCKED",
    )
    .bind(customer)
    .bind(currency)
    .bind(&acts)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)
}

/// Add the client's `next_invoice` items to a draft (renewal) invoice.
pub async fn attach_next(
    tx: &mut Tx<'_>,
    customer: Uuid,
    currency: &str,
    invoice: Uuid,
) -> AppResult<usize> {
    let rows = pending(tx, customer, currency, &["next_invoice"]).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let lines: Vec<NewItem> = rows.iter().map(|(_, d, q, p)| line(d, *q, *p)).collect();
    invoices::add_items(tx, invoice, currency, &lines).await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    mark(tx, &ids, invoice).await?;
    Ok(ids.len())
}

/// Invoice all of a client's held / next-invoice items now (one invoice).
pub async fn invoice_pending(
    pool: &PgPool,
    customer: Uuid,
    due_days: i64,
) -> AppResult<Option<Uuid>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let cur: Option<String> =
        sqlx::query_scalar("SELECT currency::text FROM customers WHERE id = $1")
            .bind(customer)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let cur = cur.ok_or(AppError::NotFound)?;
    let rows = pending(&mut tx, customer, &cur, &["hold", "next_invoice"]).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let inv = invoices::create_draft(&mut tx, customer, &cur, None).await?;
    let lines: Vec<NewItem> = rows.iter().map(|(_, d, q, p)| line(d, *q, *p)).collect();
    invoices::add_items(&mut tx, inv, &cur, &lines).await?;
    invoices::finalize(&mut tx, inv, due_days).await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    mark(&mut tx, &ids, inv).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(inv))
}

/// Invoice recurring items due on/before `horizon`, one period each, then
/// advance them (idempotent under concurrent workers).
pub async fn bill_recurring(
    pool: &PgPool,
    horizon: Date,
    due_days: i64,
    batch: i64,
) -> AppResult<Vec<(Uuid, Uuid)>> {
    type Row = (
        Uuid,
        Uuid,
        String,
        Decimal,
        Decimal,
        String,
        String,
        Date,
        Option<i32>,
    );
    let due: Vec<Row> = sqlx::query_as(
        "SELECT id, customer_id, description, quantity, unit_price, currency::text, recur_cycle, next_due_date, recur_remaining
         FROM billable_items WHERE invoice_action = 'recurring' AND next_due_date <= $1
           AND (recur_remaining IS NULL OR recur_remaining > 0)
         ORDER BY next_due_date, id LIMIT $2",
    )
    .bind(horizon)
    .bind(batch)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::new();
    for (id, cust, desc, qty, price, cur, cyc, next, left) in due {
        let Some((start, end)) = cycle::period(next, &cyc) else {
            continue;
        };
        let mut tx = pool.begin().await.map_err(db_err)?;
        let still: Option<Option<Date>> = sqlx::query_scalar(
            "SELECT next_due_date FROM billable_items WHERE id = $1 FOR UPDATE SKIP LOCKED",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        if still.flatten() != Some(next) {
            continue;
        }
        let inv = invoices::create_draft(&mut tx, cust, &cur, None).await?;
        invoices::add_items(
            &mut tx,
            inv,
            &cur,
            &[line(&format!("{desc} ({start}, {end})"), qty, price)],
        )
        .await?;
        invoices::finalize(&mut tx, inv, due_days).await?;
        let left = left.map(|l| l - 1);
        sqlx::query("UPDATE billable_items SET next_due_date = CASE WHEN $3::int = 0 THEN NULL ELSE $2 END, recur_remaining = $3, invoice_id = $4, invoiced_at = now() WHERE id = $1")
            .bind(id)
            .bind(end)
            .bind(left)
            .bind(inv)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        out.push((inv, cust));
    }
    Ok(out)
}

/// Add one-off lines to a draft invoice (409 once finalized).
pub async fn add_draft_lines(pool: &PgPool, invoice: Uuid, lines: &[NewItem]) -> AppResult<()> {
    if lines.is_empty() || lines.len() > 100 {
        return Err(v());
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT currency::text, finalized_at IS NOT NULL FROM invoices WHERE id = $1 FOR UPDATE",
    )
    .bind(invoice)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (cur, fin) = row.ok_or(AppError::NotFound)?;
    if fin {
        return Err(AppError::Conflict("error.invoice_finalized"));
    }
    invoices::add_items(&mut tx, invoice, &cur, lines).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

/// Per-product invoice lead time (days before due), `None` = global default.
pub async fn product_days(pool: &PgPool, product: Uuid) -> AppResult<Option<i32>> {
    let r: Option<Option<i32>> =
        sqlx::query_scalar("SELECT invoice_days_before FROM products WHERE id = $1")
            .bind(product)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    r.ok_or(AppError::NotFound)
}

pub async fn set_product_days(pool: &PgPool, product: Uuid, days: Option<i32>) -> AppResult<()> {
    if days.is_some_and(|d| !(0..=365).contains(&d)) {
        return Err(v());
    }
    let r = sqlx::query("UPDATE products SET invoice_days_before = $2 WHERE id = $1")
        .bind(product)
        .bind(days)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Fixed-term length in days for a product (null = not fixed-term).
pub async fn product_fixed_term(pool: &PgPool, product: Uuid) -> AppResult<Option<i32>> {
    let r: Option<Option<i32>> =
        sqlx::query_scalar("SELECT auto_terminate_days FROM products WHERE id = $1")
            .bind(product)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    r.ok_or(AppError::NotFound)
}

pub async fn set_product_fixed_term(
    pool: &PgPool,
    product: Uuid,
    days: Option<i32>,
) -> AppResult<()> {
    if days.is_some_and(|d| !(1..=3650).contains(&d)) {
        return Err(v());
    }
    let r = sqlx::query("UPDATE products SET auto_terminate_days = $2 WHERE id = $1")
        .bind(product)
        .bind(days)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Active/suspended services of fixed-term products whose term has elapsed.
pub async fn fixed_term_due(pool: &PgPool, limit: i64) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT s.id FROM services s JOIN products p ON p.id = s.product_id
         WHERE p.auto_terminate_days IS NOT NULL AND s.status IN ('active','suspended')
           AND s.activated_at IS NOT NULL
           AND s.activated_at + make_interval(days => p.auto_terminate_days) <= now()
         ORDER BY s.activated_at LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn input(action: &str) -> BillableInput {
        BillableInput {
            customer_id: Uuid::nil(),
            description: "Setup work".into(),
            quantity: Decimal::ONE,
            unit_price: Decimal::from_str("12.50").unwrap(),
            invoice_action: action.into(),
            recur_cycle: None,
            next_due_date: None,
            recur_times: None,
        }
    }

    #[test]
    fn validates_items() {
        assert!(check(&input("hold")).is_ok());
        assert!(check(&input("bogus")).is_err());
        assert!(check(&BillableInput {
            description: " ".into(),
            ..input("hold")
        })
        .is_err());
        assert!(check(&BillableInput {
            quantity: Decimal::ZERO,
            ..input("hold")
        })
        .is_err());
        assert!(check(&BillableInput {
            unit_price: Decimal::from(-1),
            ..input("hold")
        })
        .is_err());
        // recurring needs a real cycle and a start date
        assert!(check(&input("recurring")).is_err());
        let d = Date::from_calendar_date(2030, time::Month::January, 1).unwrap();
        let r = BillableInput {
            recur_cycle: Some("monthly".into()),
            next_due_date: Some(d),
            ..input("recurring")
        };
        assert!(check(&r).is_ok());
        assert!(check(&BillableInput {
            recur_cycle: Some("one_time".into()),
            ..r.clone()
        })
        .is_err());
        assert!(check(&BillableInput {
            recur_times: Some(0),
            ..r
        })
        .is_err());
    }
}
