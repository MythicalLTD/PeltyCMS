//! Invoices, items, finalization (gapless numbering), credit notes and
//! idempotent payment application. Finalized invoices are immutable; the DB
//! triggers in 0004 enforce it even if this code had a bug.
use crate::db_err;
use pelty_billing::math::{self, LineInput};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

pub type Tx<'a> = Transaction<'a, Postgres>;

const COLS: &str = "id, number, kind, credit_for, customer_id, order_id, currency::text AS currency, status, subtotal, \
    tax_total, total, amount_paid, issued_at, due_at, paid_at, finalized_at, created_at, tax_inclusive, reverse_charge, tax_breakdown, customer_snapshot";

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Invoice {
    pub id: Uuid,
    pub number: Option<String>,
    pub kind: String,
    pub credit_for: Option<Uuid>,
    pub customer_id: Uuid,
    pub order_id: Option<Uuid>,
    pub currency: String,
    pub status: String,
    pub subtotal: Decimal,
    pub tax_total: Decimal,
    pub total: Decimal,
    pub amount_paid: Decimal,
    #[serde(with = "time::serde::rfc3339::option")]
    pub issued_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub due_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub paid_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finalized_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub tax_inclusive: bool,
    pub reverse_charge: bool,
    pub tax_breakdown: serde_json::Value,
    pub customer_snapshot: Option<serde_json::Value>,
}

impl Invoice {
    pub fn balance(&self) -> Decimal {
        (self.total - self.amount_paid).max(Decimal::ZERO)
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Item {
    pub id: Uuid,
    pub description: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub tax_rate: Decimal,
    pub amount: Decimal,
    pub tax_amount: Decimal,
    pub service_id: Option<Uuid>,
    pub period_start: Option<Date>,
    pub period_end: Option<Date>,
}

#[derive(Debug, Clone)]
pub struct NewItem {
    pub description: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub tax_rate: Decimal,
    pub service_id: Option<Uuid>,
    pub period: Option<(Date, Date)>,
    /// Price this line with the tax engine at finalization (ignores `tax_rate`).
    pub auto_tax: bool,
}

pub async fn exponent(tx: &mut Tx<'_>, currency: &str) -> AppResult<u32> {
    let e: Option<i16> =
        sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1 AND enabled")
            .bind(currency)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    e.map(|e| e.max(0) as u32)
        .ok_or(AppError::Validation("error.invalid_currency"))
}

pub async fn create_draft(
    tx: &mut Tx<'_>,
    customer_id: Uuid,
    currency: &str,
    order_id: Option<Uuid>,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO invoices (id, customer_id, currency, order_id) VALUES ($1,$2,$3,$4)")
        .bind(id)
        .bind(customer_id)
        .bind(currency)
        .bind(order_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(id)
}

/// Add priced lines (exact decimal math, rounded to the currency exponent)
/// and recompute totals. A service period billed twice fails with 409
/// (unique index), which keeps renewals idempotent.
pub async fn add_items(
    tx: &mut Tx<'_>,
    invoice_id: Uuid,
    currency: &str,
    items: &[NewItem],
) -> AppResult<()> {
    let exp = exponent(tx, currency).await?;
    for it in items {
        if it.description.trim().is_empty() || it.description.len() > 500 {
            return Err(AppError::Validation("error.validation"));
        }
        let l = math::line(
            &LineInput {
                quantity: it.quantity,
                unit_price: it.unit_price,
                tax_rate: it.tax_rate,
            },
            exp,
        )
        .map_err(|_| AppError::Validation("error.invalid_amount"))?;
        sqlx::query(
            "INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, tax_rate, amount, tax_amount,
                service_id, period_start, period_end, tax_auto, sort_order)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,
                (SELECT COALESCE(max(sort_order), -1) + 1 FROM invoice_items WHERE invoice_id = $2))",
        )
        .bind(Uuid::new_v4())
        .bind(invoice_id)
        .bind(it.description.trim())
        .bind(it.quantity)
        .bind(it.unit_price)
        .bind(it.tax_rate)
        .bind(l.amount)
        .bind(l.tax_amount)
        .bind(it.service_id)
        .bind(it.period.map(|p| p.0))
        .bind(it.period.map(|p| p.1))
        .bind(it.auto_tax)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    recompute(tx, invoice_id).await
}

/// Totals are the sum of already-rounded lines, so they always add up.
pub async fn recompute(tx: &mut Tx<'_>, invoice_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE invoices i SET subtotal = s.sub, tax_total = s.tax, total = s.sub + s.tax
         FROM (SELECT COALESCE(sum(amount),0) sub, COALESCE(sum(tax_amount),0) tax FROM invoice_items WHERE invoice_id = $1) s
         WHERE i.id = $1 AND i.finalized_at IS NULL",
    )
    .bind(invoice_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub const PROFORMA_KEY: &str = "billing.proforma";

/// WHMCS "Sequential Paid Invoice Numbering": unpaid invoices are proforma.
pub async fn proforma_enabled(tx: &mut Tx<'_>) -> AppResult<bool> {
    let v: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM system_settings WHERE key = $1")
            .bind(PROFORMA_KEY)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    Ok(v.and_then(|v| v.as_bool()).unwrap_or(false))
}

/// Per-entity, per-year numbering when legal entities exist; else the
/// legacy global sequence.
async fn next_number(
    tx: &mut Tx<'_>,
    entity: Option<&crate::legal_entities::LegalEntity>,
    kind: &str,
) -> AppResult<String> {
    if let Some(e) = entity {
        let year: i32 =
            sqlx::query_scalar("SELECT extract(year FROM now() AT TIME ZONE 'UTC')::int")
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
        return crate::legal_entities::next_number(tx, e, kind, year).await;
    }
    let seq: i64 = sqlx::query_scalar(
        "UPDATE invoice_sequences SET next = next + 1 WHERE name = $1 RETURNING next - 1",
    )
    .bind(kind)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    let prefix = match kind {
        "credit_note" => "CN",
        "debit_note" => "DN",
        _ => "INV",
    };
    Ok(format!("{prefix}-{seq:06}"))
}

/// Give a paid proforma invoice its legal number; the issue date becomes the
/// payment date. No-op when it already has one.
async fn number_if_proforma(tx: &mut Tx<'_>, invoice_id: Uuid) -> AppResult<()> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM invoices WHERE id = $1 AND number IS NULL AND finalized_at IS NOT NULL AND status = 'paid'",
    )
    .bind(invoice_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let Some(kind) = row else { return Ok(()) };
    let entity = crate::legal_entities::resolve(tx, invoice_id).await?;
    let number = next_number(tx, entity.as_ref(), &kind).await?;
    sqlx::query("UPDATE invoices SET number = $2, issued_at = now() WHERE id = $1")
        .bind(invoice_id)
        .bind(number)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Assign the next gapless number and freeze the invoice. Numbering uses a
/// row lock on `invoice_sequences`, so concurrent finalizations serialize and
/// a rolled-back transaction never burns a number.
pub async fn finalize(tx: &mut Tx<'_>, invoice_id: Uuid, due_days: i64) -> AppResult<Invoice> {
    let inv: Option<(String, Option<OffsetDateTime>, i64)> = sqlx::query_as(
        "SELECT kind, finalized_at, (SELECT count(*) FROM invoice_items WHERE invoice_id = $1) FROM invoices WHERE id = $1 FOR UPDATE",
    )
    .bind(invoice_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (kind, fin, n_items) = inv.ok_or(AppError::NotFound)?;
    if fin.is_some() {
        return Err(AppError::Conflict("error.invoice_finalized"));
    }
    if n_items == 0 {
        return Err(AppError::Validation("error.invoice_empty"));
    }
    if kind != "credit_note" {
        crate::tax::apply(tx, invoice_id).await?;
    }
    let entity = crate::legal_entities::resolve(tx, invoice_id).await?;
    let due_days = entity
        .as_ref()
        .and_then(|e| e.due_days)
        .map_or(due_days, i64::from);
    // Proforma mode: a payable invoice gets its legal number only once paid.
    let total: Decimal = sqlx::query_scalar("SELECT total FROM invoices WHERE id = $1")
        .bind(invoice_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    let number = if kind == "invoice" && total > Decimal::ZERO && proforma_enabled(tx).await? {
        None
    } else {
        Some(next_number(tx, entity.as_ref(), &kind).await?)
    };
    let snapshot = entity
        .as_ref()
        .map(|e| serde_json::to_value(e).unwrap_or_default());
    // A zero-total invoice is paid on issue; credit notes are settled by definition.
    sqlx::query_as(&format!(
        "UPDATE invoices SET number = $2, finalized_at = now(), issued_at = now(),
            due_at = now() + make_interval(days => $3::int),
            status = CASE WHEN total = 0 OR kind = 'credit_note' THEN 'paid' ELSE 'unpaid' END,
            paid_at = CASE WHEN total = 0 OR kind = 'credit_note' THEN now() END,
            legal_entity_id = $4, seller_snapshot = $5
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(invoice_id)
    .bind(number)
    .bind(due_days.clamp(0, 365) as i32)
    .bind(entity.map(|e| e.id))
    .bind(snapshot)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Invoice>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM invoices WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Fetch only if owned by `customer_id` (ownership enforced in SQL).
pub async fn get_owned(pool: &PgPool, id: Uuid, customer_id: Uuid) -> AppResult<Option<Invoice>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM invoices WHERE id = $1 AND customer_id = $2 AND status <> 'draft'"
    ))
    .bind(id)
    .bind(customer_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn items(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Item>> {
    sqlx::query_as(
        "SELECT id, description, quantity, unit_price, tax_rate, amount, tax_amount, service_id, period_start, period_end
         FROM invoice_items WHERE invoice_id = $1 ORDER BY sort_order, id",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// List; `customer` scopes to one customer (client area hides drafts).
pub async fn list(
    pool: &PgPool,
    customer: Option<Uuid>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Invoice>, i64)> {
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        i: Invoice,
        total_count: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS}, count(*) OVER() AS total_count FROM invoices
         WHERE ($1::uuid IS NULL OR (customer_id = $1 AND status <> 'draft')) AND ($2::text IS NULL OR status = $2)
         ORDER BY created_at DESC, id LIMIT $3 OFFSET $4"
    ))
    .bind(customer)
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = rows.first().map(|r| r.total_count).unwrap_or(0);
    Ok((rows.into_iter().map(|r| r.i).collect(), total))
}

/// Void an unpaid invoice that has received no money.
pub async fn void(pool: &PgPool, id: Uuid) -> AppResult<Invoice> {
    sqlx::query_as(&format!(
        "UPDATE invoices SET status = 'void' WHERE id = $1 AND status = 'unpaid' AND amount_paid = 0 RETURNING {COLS}"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::Conflict("error.invoice_not_voidable"))
}

/// Credit-note groundwork: a finalized credit note mirroring a paid invoice's
/// lines; the original is marked refunded. Money movement (gateway refund)
/// is a separate step.
pub async fn credit_note(pool: &PgPool, invoice_id: Uuid) -> AppResult<Invoice> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let orig: Option<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT customer_id, currency::text, status, kind FROM invoices WHERE id = $1 FOR UPDATE",
    )
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (customer, currency, status, kind) = orig.ok_or(AppError::NotFound)?;
    if status != "paid" || kind != "invoice" {
        return Err(AppError::Conflict("error.invoice_not_creditable"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO invoices (id, kind, credit_for, customer_id, currency) VALUES ($1, 'credit_note', $2, $3, $4)")
        .bind(id)
        .bind(invoice_id)
        .bind(customer)
        .bind(&currency)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query(
        "INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, tax_rate, amount, tax_amount, sort_order)
         SELECT gen_random_uuid(), $1, description, quantity, unit_price, tax_rate, amount, tax_amount, sort_order
         FROM invoice_items WHERE invoice_id = $2",
    )
    .bind(id)
    .bind(invoice_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    recompute(&mut tx, id).await?;
    sqlx::query("UPDATE invoices c SET tax_inclusive = o.tax_inclusive, reverse_charge = o.reverse_charge, tax_breakdown = o.tax_breakdown,
            customer_snapshot = o.customer_snapshot FROM invoices o WHERE c.id = $1 AND o.id = $2")
        .bind(id)
        .bind(invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let cn = finalize(&mut tx, id, 0).await?;
    sqlx::query("UPDATE invoices SET status = 'refunded' WHERE id = $1")
        .bind(invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(cn)
}

// payments

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Payment {
    pub id: Uuid,
    pub invoice_id: Option<Uuid>,
    pub gateway: String,
    pub provider_ref: Option<String>,
    pub amount: Decimal,
    pub currency: String,
    pub status: String,
    /// Gateway processing fee (merchant cost), when configured.
    pub fee: Option<Decimal>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub applied_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const PAYCOLS: &str = "id, invoice_id, gateway, provider_ref, amount, currency::text AS currency, status, fee, applied_at, created_at";

pub async fn payments(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Payment>> {
    sqlx::query_as(&format!(
        "SELECT {PAYCOLS} FROM payments WHERE invoice_id = $1 ORDER BY created_at"
    ))
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Create a pending payment for the invoice's outstanding balance (checkout
/// start). Idempotent on `idempotency_key`.
pub async fn create_pending(
    pool: &PgPool,
    inv: &Invoice,
    gateway: &str,
    user: Option<Uuid>,
    key: &str,
) -> AppResult<Payment> {
    if inv.status != "unpaid" || inv.balance() <= Decimal::ZERO {
        return Err(AppError::Conflict("error.invoice_not_payable"));
    }
    let row: Option<Payment> = sqlx::query_as(&format!(
        "INSERT INTO payments (id, user_id, gateway, amount, currency, status, idempotency_key, invoice_id, customer_id)
         VALUES ($1,$2,$3,$4,$5,'pending',$6,$7,$8) ON CONFLICT (idempotency_key) DO NOTHING RETURNING {PAYCOLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(gateway)
    .bind(inv.balance())
    .bind(&inv.currency)
    .bind(key)
    .bind(inv.id)
    .bind(inv.customer_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some(p) => Ok(p),
        None => sqlx::query_as(&format!(
            "SELECT {PAYCOLS} FROM payments WHERE idempotency_key = $1 AND invoice_id = $2"
        ))
        .bind(key)
        .bind(inv.id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::Conflict("error.conflict")),
    }
}

/// One checkout for several unpaid invoices (same customer + currency). Keeps
/// each invoice intact; `payment_targets` splits the capture on apply.
pub async fn create_pending_multi(
    pool: &PgPool,
    invs: &[Invoice],
    gateway: &str,
    user: Option<Uuid>,
    key: &str,
) -> AppResult<Payment> {
    if invs.len() < 2 || invs.len() > 50 {
        return Err(AppError::Validation("error.merge_needs_two"));
    }
    let first = &invs[0];
    for inv in invs {
        if inv.customer_id != first.customer_id || inv.currency != first.currency {
            return Err(AppError::Validation("error.merge_mixed"));
        }
        if inv.status != "unpaid"
            || inv.amount_paid > Decimal::ZERO
            || inv.kind != "invoice"
            || inv.balance() <= Decimal::ZERO
        {
            return Err(AppError::Conflict("error.invoice_not_payable"));
        }
    }
    let total = invs.iter().fold(Decimal::ZERO, |a, i| a + i.balance());
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<Payment> = sqlx::query_as(&format!(
        "INSERT INTO payments (id, user_id, gateway, amount, currency, status, idempotency_key, invoice_id, customer_id)
         VALUES ($1,$2,$3,$4,$5,'pending',$6,$7,$8) ON CONFLICT (idempotency_key) DO NOTHING RETURNING {PAYCOLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(gateway)
    .bind(total)
    .bind(&first.currency)
    .bind(key)
    .bind(first.id)
    .bind(first.customer_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let pay = match row {
        Some(p) => p,
        None => {
            tx.rollback().await.ok();
            return sqlx::query_as(&format!(
                "SELECT {PAYCOLS} FROM payments WHERE idempotency_key = $1 AND invoice_id = $2"
            ))
            .bind(key)
            .bind(first.id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .ok_or(AppError::Conflict("error.conflict"));
        }
    };
    for inv in invs {
        sqlx::query(
            "INSERT INTO payment_targets (payment_id, invoice_id, amount) VALUES ($1,$2,$3)
             ON CONFLICT DO NOTHING",
        )
        .bind(pay.id)
        .bind(inv.id)
        .bind(inv.balance())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(pay)
}

pub async fn set_provider_ref(
    pool: &PgPool,
    payment_id: Uuid,
    provider_ref: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE payments SET provider_ref = $2, updated_at = now() WHERE id = $1 AND provider_ref IS NULL")
        .bind(payment_id)
        .bind(provider_ref)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub payment_id: Uuid,
    pub invoice_id: Uuid,
    /// True if this call was a replay and changed nothing.
    pub duplicate: bool,
    /// True if this call moved the invoice to `paid`.
    pub invoice_paid: bool,
}

/// Credit a payment exactly once. Single-invoice payments use `payments.invoice_id`;
/// multi-invoice checkouts (no merge) distribute via `payment_targets`.
pub(crate) async fn apply_locked(tx: &mut Tx<'_>, payment_id: Uuid) -> AppResult<Applied> {
    let p: (Option<Uuid>, Decimal, String, Option<OffsetDateTime>) =
        sqlx::query_as("SELECT invoice_id, amount, currency::text, applied_at FROM payments WHERE id = $1 FOR UPDATE")
            .bind(payment_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
    let (primary, pay_amount, currency, applied) = p;
    let Some(primary_id) = primary else {
        return Err(AppError::NotFound);
    };
    if applied.is_some() {
        return Ok(Applied {
            payment_id,
            invoice_id: primary_id,
            duplicate: true,
            invoice_paid: false,
        });
    }
    let mut targets: Vec<(Uuid, Decimal)> = sqlx::query_as(
        "SELECT invoice_id, amount FROM payment_targets WHERE payment_id = $1 ORDER BY invoice_id",
    )
    .bind(payment_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if targets.is_empty() {
        targets.push((primary_id, pay_amount));
    } else {
        let sum = targets.iter().fold(Decimal::ZERO, |a, (_, n)| a + *n);
        if sum != pay_amount {
            return Err(AppError::Conflict("error.payment_targets_mismatch"));
        }
    }
    sqlx::query("UPDATE payments SET status = 'succeeded', applied_at = now(), updated_at = now() WHERE id = $1")
        .bind(payment_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    let mut any_newly_paid = false;
    for (invoice_id, amount) in &targets {
        let inv: (String, String) =
            sqlx::query_as("SELECT status, currency::text FROM invoices WHERE id = $1 FOR UPDATE")
                .bind(invoice_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
        if inv.1 != currency {
            return Err(AppError::Validation("error.currency_mismatch"));
        }
        if !matches!(inv.0.as_str(), "unpaid" | "paid") {
            return Err(AppError::Conflict("error.invoice_not_payable"));
        }
        let now_paid: bool = sqlx::query_scalar(
            "UPDATE invoices SET amount_paid = amount_paid + $2,
                status = CASE WHEN amount_paid + $2 >= total THEN 'paid' ELSE status END,
                paid_at = CASE WHEN amount_paid + $2 >= total AND paid_at IS NULL THEN now() ELSE paid_at END
             WHERE id = $1 RETURNING (status = 'paid' AND $3 <> 'paid')",
        )
        .bind(invoice_id)
        .bind(amount)
        .bind(&inv.0)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
        if now_paid {
            number_if_proforma(tx, *invoice_id).await?;
            any_newly_paid = true;
        }
    }
    Ok(Applied {
        payment_id,
        invoice_id: primary_id,
        duplicate: false,
        invoice_paid: any_newly_paid,
    })
}

/// Record a payment received outside a checkout (manual/bank transfer, or
/// staff entry). Idempotent on `key`: replays return `duplicate = true`.
pub async fn record_payment(
    pool: &PgPool,
    invoice_id: Uuid,
    gateway: &str,
    provider_ref: Option<&str>,
    amount: Decimal,
    key: &str,
    actor: Option<Uuid>,
) -> AppResult<Applied> {
    if amount <= Decimal::ZERO {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let inv: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT customer_id, currency::text, status FROM invoices WHERE id = $1 FOR UPDATE",
    )
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (customer, currency, _status) = inv.ok_or(AppError::NotFound)?;
    let exp = exponent(&mut tx, &currency).await?;
    if amount.normalize().scale() > exp {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO payments (id, user_id, gateway, provider_ref, amount, currency, status, idempotency_key, invoice_id, customer_id)
         VALUES ($1,$2,$3,$4,$5,$6,'pending',$7,$8,$9) ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(gateway)
    .bind(provider_ref)
    .bind(amount)
    .bind(&currency)
    .bind(key)
    .bind(invoice_id)
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let res = match id {
        Some(id) => apply_locked(&mut tx, id).await?,
        None => {
            let existing: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
                "SELECT id, invoice_id FROM payments WHERE idempotency_key = $1 OR (gateway = $2 AND provider_ref = $3)",
            )
            .bind(key)
            .bind(gateway)
            .bind(provider_ref)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
            match existing {
                Some((pid, Some(inv))) if inv == invoice_id => Applied {
                    payment_id: pid,
                    invoice_id,
                    duplicate: true,
                    invoice_paid: false,
                },
                _ => return Err(AppError::Conflict("error.idempotency_conflict")),
            }
        }
    };
    tx.commit().await.map_err(db_err)?;
    Ok(res)
}

/// Webhook path: a provider confirmed payment `provider_ref`. Applies it to
/// its invoice once; replays and unknown refs are no-ops.
pub async fn apply_by_ref(
    pool: &PgPool,
    gateway: &str,
    provider_ref: &str,
) -> AppResult<Option<Applied>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM payments WHERE gateway = $1 AND provider_ref = $2 AND invoice_id IS NOT NULL")
        .bind(gateway)
        .bind(provider_ref)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let Some(id) = id else { return Ok(None) };
    let r = apply_locked(&mut tx, id).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(r))
}

/// Finalized credit notes issued against an invoice, oldest first.
/// A line on a debit note (tax is computed by the tax engine at issue).
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DebitLine {
    pub description: String,
    #[schema(value_type = String)]
    pub quantity: Decimal,
    #[schema(value_type = String)]
    pub unit_price: Decimal,
}

/// Issue a debit note against a finalized invoice: an additional,
/// separately payable charge (late fee, under-billing, extra usage…).
pub async fn debit_note(
    pool: &PgPool,
    invoice_id: Uuid,
    lines: &[DebitLine],
    reason: &str,
    due_days: i64,
) -> AppResult<Invoice> {
    let reason = reason.trim();
    if lines.is_empty() || lines.len() > 50 || reason.len() > 2000 {
        return Err(AppError::Validation("error.validation"));
    }
    if lines
        .iter()
        .any(|l| l.quantity <= Decimal::ZERO || l.unit_price <= Decimal::ZERO)
    {
        return Err(AppError::Validation("error.debit_note_amount"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let orig: Option<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT customer_id, currency::text, status, kind FROM invoices WHERE id = $1 FOR UPDATE",
    )
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (customer, currency, status, kind) = orig.ok_or(AppError::NotFound)?;
    if kind != "invoice" || !matches!(status.as_str(), "unpaid" | "paid") {
        return Err(AppError::Conflict("error.invoice_not_debitable"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO invoices (id, kind, credit_for, customer_id, currency, note_reason) VALUES ($1, 'debit_note', $2, $3, $4, $5)")
        .bind(id)
        .bind(invoice_id)
        .bind(customer)
        .bind(&currency)
        .bind(reason)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let items: Vec<NewItem> = lines
        .iter()
        .map(|l| NewItem {
            description: l.description.trim().to_string(),
            quantity: l.quantity,
            unit_price: l.unit_price,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        })
        .collect();
    add_items(&mut tx, id, &currency, &items).await?;
    let dn = finalize(&mut tx, id, due_days).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(dn)
}

/// Debit notes issued against an invoice.
pub async fn debit_notes_for(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Invoice>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM invoices WHERE credit_for = $1 AND kind = 'debit_note' AND number IS NOT NULL ORDER BY created_at"))
        .bind(invoice_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Reason text stored on a credit/debit note.
pub async fn note_reason(pool: &PgPool, id: Uuid) -> AppResult<String> {
    sqlx::query_scalar("SELECT note_reason FROM invoices WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
        .map(Option::unwrap_or_default)
}

pub async fn credit_notes_for(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Invoice>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM invoices WHERE credit_for = $1 AND kind = 'credit_note' AND number IS NOT NULL ORDER BY created_at"))
        .bind(invoice_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Number shown to people: the real number, or `PF-XXXXXXXX` for an
/// unnumbered proforma (same label the cron SQL builds with COALESCE).
pub fn display_number(number: Option<String>, id: Uuid) -> String {
    number
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("PF-{}", id.to_string()[..8].to_uppercase()))
}

#[cfg(test)]
mod display_number_tests {
    #[test]
    fn proforma_label() {
        let id: uuid::Uuid = "0a1b2c3d-0000-0000-0000-000000000000".parse().unwrap();
        assert_eq!(super::display_number(None, id), "PF-0A1B2C3D");
        assert_eq!(super::display_number(Some("INV-1".into()), id), "INV-1");
    }
}
