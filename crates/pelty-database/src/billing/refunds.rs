//! Refunds against payments. Two phases so a gateway call never runs inside
//! a DB transaction: [`reserve`] locks the payment and records a pending
//! refund (capacity is checked under the lock, so concurrent refunds can
//! never exceed the payment), then [`complete`] / [`fail`] settle it.
use crate::invoices::{self, Tx};
use crate::{credit, db_err};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Refund {
    pub id: Uuid,
    pub payment_id: Uuid,
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
    pub gateway: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub currency: String,
    pub status: String,
    pub provider_ref: Option<String>,
    pub reason: String,
    pub error: Option<String>,
    pub credit_note_id: Option<Uuid>,
    /// Settled as account credit instead of through the gateway.
    pub to_credit: bool,
    /// Chargeback/reversal recorded by staff; the gateway was not called.
    pub reversal: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, payment_id, invoice_id, customer_id, gateway, amount, currency::text AS currency, status, provider_ref, reason, error, credit_note_id, to_credit, reversal, created_at";

/// What the gateway layer needs to execute a reserved refund.
#[derive(Debug, Clone)]
pub struct Reserved {
    pub refund: Refund,
    /// Provider reference of the original payment (None for manual payments).
    pub payment_ref: Option<String>,
    /// True when this key was already used: nothing new to execute.
    pub replay: bool,
}

type PaymentRow = (Option<Uuid>, Option<Uuid>, String, Option<String>, Decimal, String, String, Option<OffsetDateTime>);

/// Amount of a payment still refundable (succeeded payments only).
pub async fn refundable(pool: &PgPool, payment_id: Uuid) -> AppResult<Decimal> {
    let v: Option<Decimal> = sqlx::query_scalar(
        "SELECT p.amount - COALESCE((SELECT sum(amount) FROM refunds r WHERE r.payment_id = p.id AND r.status <> 'failed'), 0)
         FROM payments p WHERE p.id = $1 AND p.status IN ('succeeded','refunded')",
    )
    .bind(payment_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(v.unwrap_or_default().max(Decimal::ZERO))
}

/// Lock the payment, validate the amount (None = everything left) and insert
/// a pending refund. Replaying the same key returns the existing refund.
pub async fn reserve(
    pool: &PgPool,
    payment_id: Uuid,
    amount: Option<Decimal>,
    reason: &str,
    key: &str,
    actor: Option<Uuid>,
    to_credit: bool,
) -> AppResult<Reserved> {
    if reason.chars().count() > 500 || key.trim().is_empty() || key.len() > 200 {
        return Err(AppError::Validation("error.validation"));
    }
    let key = format!("refund:{payment_id}:{}", key.trim());
    let mut tx = pool.begin().await.map_err(db_err)?;
    let p: Option<PaymentRow> = sqlx::query_as(
        "SELECT invoice_id, customer_id, gateway, provider_ref, amount, currency::text, status, applied_at FROM payments WHERE id = $1 FOR UPDATE",
    )
    .bind(payment_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (invoice_id, customer_id, gateway, pref, paid, currency, status, applied) = p.ok_or(AppError::NotFound)?;
    if let Some(r) = by_key(&mut tx, &key).await? {
        return Ok(Reserved { refund: r, payment_ref: pref, replay: true });
    }
    let (Some(invoice_id), Some(customer_id)) = (invoice_id, customer_id) else {
        return Err(AppError::Conflict("error.payment_not_refundable"));
    };
    if !matches!(status.as_str(), "succeeded" | "refunded") || applied.is_none() {
        return Err(AppError::Conflict("error.payment_not_refundable"));
    }
    let used: Decimal = sqlx::query_scalar("SELECT COALESCE(sum(amount), 0) FROM refunds WHERE payment_id = $1 AND status <> 'failed'")
        .bind(payment_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let left = paid - used;
    if left <= Decimal::ZERO {
        return Err(AppError::Conflict("error.payment_fully_refunded"));
    }
    let exp = invoices::exponent(&mut tx, &currency).await?;
    let amount = amount.unwrap_or(left);
    if amount <= Decimal::ZERO || amount.normalize().scale() > exp {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    if amount > left {
        return Err(AppError::Conflict("error.refund_exceeds_payment"));
    }
    let r: Refund = sqlx::query_as(&format!(
        "INSERT INTO refunds (id, payment_id, invoice_id, customer_id, gateway, amount, currency, status, reason, idempotency_key, actor_id, to_credit)
         VALUES ($1,$2,$3,$4,$5,$6,$7,'pending',$8,$9,$10,$11) RETURNING {COLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(payment_id)
    .bind(invoice_id)
    .bind(customer_id)
    .bind(&gateway)
    .bind(amount)
    .bind(&currency)
    .bind(reason.trim())
    .bind(&key)
    .bind(actor)
    .bind(to_credit || gateway == "credit")
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    // Add-funds deposit: the credit it produced must come back out first.
    let funded: Option<String> = sqlx::query_scalar("SELECT currency::text FROM add_funds WHERE invoice_id = $1 AND credited_at IS NOT NULL")
        .bind(invoice_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    if let Some(cur) = funded {
        sqlx::query("SELECT 1 FROM customers WHERE id = $1 FOR UPDATE").bind(customer_id).execute(&mut *tx).await.map_err(db_err)?;
        let bal: Decimal = sqlx::query_scalar("SELECT COALESCE(sum(amount), 0) FROM credit_ledger WHERE customer_id = $1 AND currency = $2")
            .bind(customer_id)
            .bind(&cur)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        if bal < amount {
            return Err(AppError::Conflict("error.add_funds_refund_insufficient_credit"));
        }
        sqlx::query("INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, invoice_id, note, actor_id) VALUES ($1,$2,$3,$4,'add_funds',$5,$6,$7)")
            .bind(Uuid::new_v4())
            .bind(customer_id)
            .bind(&cur)
            .bind(-amount)
            .bind(invoice_id)
            .bind(format!("refund:{}", r.id))
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(Reserved { refund: r, payment_ref: pref, replay: false })
}

async fn by_key(tx: &mut Tx<'_>, key: &str) -> AppResult<Option<Refund>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM refunds WHERE idempotency_key = $1"))
        .bind(key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Refund>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM refunds WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn for_invoice(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Refund>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM refunds WHERE invoice_id = $1 ORDER BY created_at"))
        .bind(invoice_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// The gateway rejected the refund: release the reserved capacity.
pub async fn fail(pool: &PgPool, id: Uuid, error: &str) -> AppResult<Refund> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r: Refund = sqlx::query_as(&format!(
        "UPDATE refunds SET status = 'failed', error = left($2, 500), updated_at = now() WHERE id = $1 AND status = 'pending' RETURNING {COLS}"
    ))
    .bind(id)
    .bind(error)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?
    .ok_or(AppError::Conflict("error.refund_not_pending"))?;
    // Give back add-funds credit held for this refund.
    sqlx::query(
        "INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, invoice_id, note)
         SELECT gen_random_uuid(), customer_id, currency, -amount, 'add_funds', invoice_id, 'refund failed' FROM credit_ledger WHERE note = $1 AND amount < 0",
    )
    .bind(format!("refund:{id}"))
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(r)
}

/// The gateway accepted the refund but settles it later (webhook).
pub async fn mark_pending_ref(pool: &PgPool, id: Uuid, provider_ref: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE refunds SET provider_ref = COALESCE($2, provider_ref), updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(provider_ref)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Money went back: mark succeeded, issue the credit note, update payment
/// and invoice status, and (for credit-balance payments) return the credit.
/// Idempotent: completing an already-succeeded refund is a no-op.
pub async fn complete(pool: &PgPool, id: Uuid, provider_ref: Option<&str>) -> AppResult<Refund> {
    complete_opts(pool, id, provider_ref, None).await.map(|(r, _)| r)
}

/// What a reversal does to the invoice it reverses.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reopen {
    /// Suspend the invoice's active services (reason `overdue`, so a later
    /// payment unsuspends them through the normal path).
    pub suspend: bool,
}

/// Settle a reversal by reopening the invoice as unpaid: no credit note,
/// the reversed amount comes off `amount_paid`. Returns suspended services.
pub async fn complete_reopen(pool: &PgPool, id: Uuid, opts: Reopen) -> AppResult<(Refund, Vec<Uuid>)> {
    complete_opts(pool, id, None, Some(opts)).await
}

async fn complete_opts(pool: &PgPool, id: Uuid, provider_ref: Option<&str>, reopen: Option<Reopen>) -> AppResult<(Refund, Vec<Uuid>)> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r: Refund = sqlx::query_as(&format!("SELECT {COLS} FROM refunds WHERE id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    if r.status == "succeeded" {
        return Ok((r, Vec::new()));
    }
    if r.status != "pending" {
        return Err(AppError::Conflict("error.refund_not_pending"));
    }
    let cn = if reopen.is_some() { None } else { Some(credit_note_for(&mut tx, &r).await?) };
    if r.to_credit || r.gateway == "credit" {
        credit::return_locked(&mut tx, r.customer_id, &r.currency, r.amount, r.invoice_id, r.payment_id).await?;
    }
    // Payment fully refunded?
    sqlx::query(
        "UPDATE payments p SET status = 'refunded', updated_at = now() WHERE p.id = $1 AND p.amount <=
            (SELECT COALESCE(sum(amount),0) FROM refunds WHERE payment_id = p.id AND (status = 'succeeded' OR id = $2))",
    )
    .bind(r.payment_id)
    .bind(r.id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut suspended = Vec::new();
    if let Some(o) = reopen {
        sqlx::query(
            "UPDATE invoices SET amount_paid = GREATEST(amount_paid - $2, 0), status = 'unpaid', paid_at = NULL
             WHERE id = $1 AND status IN ('paid','unpaid','refunded')",
        )
        .bind(r.invoice_id)
        .bind(r.amount)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if o.suspend {
            sqlx::query("INSERT INTO overdue_actions (invoice_id, action) VALUES ($1, 'suspend') ON CONFLICT DO NOTHING")
                .bind(r.invoice_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            suspended = sqlx::query_scalar(
                "UPDATE services SET status = 'suspended', suspend_reason = $2, updated_at = now()
                 WHERE status = 'active' AND id IN (SELECT service_id FROM invoice_items WHERE invoice_id = $1 AND service_id IS NOT NULL)
                 RETURNING id",
            )
            .bind(r.invoice_id)
            .bind(crate::overdue::REASON)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        }
    } else {
        // Invoice fully refunded: every succeeded payment refunded in full.
    sqlx::query(
        "UPDATE invoices i SET status = 'refunded' WHERE i.id = $1 AND i.status = 'paid' AND NOT EXISTS (
            SELECT 1 FROM payments p WHERE p.invoice_id = i.id AND p.status = 'succeeded')",
    )
    .bind(r.invoice_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    }
    let r: Refund = sqlx::query_as(&format!(
        "UPDATE refunds SET status = 'succeeded', provider_ref = COALESCE($2, provider_ref), credit_note_id = $3, updated_at = now()
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(provider_ref)
    .bind(cn)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((r, suspended))
}

/// Flag a freshly reserved (pending) refund as a reversal.
pub async fn mark_reversal(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE refunds SET reversal = true, updated_at = now() WHERE id = $1 AND status = 'pending' AND NOT to_credit")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Net/tax split of a gross refund using the invoice's effective tax rate,
/// rounded to `exp` decimals; net + tax == gross exactly.
pub fn split_gross(gross: Decimal, subtotal: Decimal, tax_total: Decimal, exp: u32) -> (Decimal, Decimal, Decimal) {
    let total = subtotal + tax_total;
    if total.is_zero() || tax_total.is_zero() {
        return (gross, Decimal::ZERO, Decimal::ZERO);
    }
    let net = (gross * subtotal / total).round_dp_with_strategy(exp, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
    let rate = if subtotal.is_zero() { Decimal::ZERO } else { (tax_total * Decimal::from(100) / subtotal).round_dp(4) };
    (net, gross - net, rate)
}

async fn credit_note_for(tx: &mut Tx<'_>, r: &Refund) -> AppResult<Uuid> {
    let (number, subtotal, tax_total, inclusive, rc): (Option<String>, Decimal, Decimal, bool, bool) =
        sqlx::query_as("SELECT number, subtotal, tax_total, tax_inclusive, reverse_charge FROM invoices WHERE id = $1 FOR UPDATE")
            .bind(r.invoice_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
    let exp = invoices::exponent(tx, &r.currency).await?;
    let (net, tax, rate) = split_gross(r.amount, subtotal, tax_total, exp);
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO invoices (id, kind, credit_for, customer_id, currency) VALUES ($1, 'credit_note', $2, $3, $4)")
        .bind(id)
        .bind(r.invoice_id)
        .bind(r.customer_id)
        .bind(&r.currency)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    let desc = format!("Refund, {}{}", number.unwrap_or_default(), if r.reason.is_empty() { String::new() } else { format!(" ({})", r.reason) });
    sqlx::query(
        "INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, tax_rate, amount, tax_amount, sort_order)
         VALUES ($1,$2,$3,1,$4,$5,$4,$6,0)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(desc.chars().take(500).collect::<String>())
    .bind(net)
    .bind(rate)
    .bind(tax)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    invoices::recompute(tx, id).await?;
    sqlx::query(
        "UPDATE invoices c SET tax_inclusive = $3, reverse_charge = $4, customer_snapshot = o.customer_snapshot,
            tax_breakdown = CASE WHEN $5::numeric = 0 THEN '[]'::jsonb ELSE jsonb_build_array(jsonb_build_object('name', 'Tax', 'rate', $6::text, 'compound', false, 'amount', $5::text)) END
         FROM invoices o WHERE c.id = $1 AND o.id = $2",
    )
    .bind(id)
    .bind(r.invoice_id)
    .bind(inclusive)
    .bind(rc)
    .bind(tax)
    .bind(rate.normalize().to_string())
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    invoices::finalize(tx, id, 0).await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::split_gross;
    use rust_decimal::Decimal;
    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn split_is_exact() {
        assert_eq!(split_gross(dec("10"), dec("100"), dec("19"), 2), (dec("8.40"), dec("1.60"), dec("19")));
        assert_eq!(split_gross(dec("119"), dec("100"), dec("19"), 2), (dec("100.00"), dec("19.00"), dec("19")));
        assert_eq!(split_gross(dec("5"), dec("50"), dec("0"), 2), (dec("5"), dec("0"), dec("0")));
        for g in [dec("0.01"), dec("0.03"), dec("33.33"), dec("99.99")] {
            let (n, t, _) = split_gross(g, dec("37.5"), dec("7.13"), 2);
            assert_eq!(n + t, g);
        }
    }
}
