//! Services (provisioned products) with lifecycle and renewal billing.
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_billing::{cycle, lifecycle};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

const COLS: &str = "s.id, s.customer_id, s.product_id, p.name AS product_name, s.status, s.cycle, s.currency::text AS currency, \
    s.recurring_amount, s.next_due_date, s.suspend_reason, s.activated_at, s.terminated_at, s.created_at, s.custom_fields, s.config";

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Service {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub status: String,
    pub cycle: String,
    pub currency: String,
    pub recurring_amount: Decimal,
    pub next_due_date: Option<Date>,
    pub suspend_reason: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub activated_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub terminated_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Custom order field values (field_id → value).
    pub custom_fields: serde_json::Value,
    /// Configurable-option selections (option_id → choice id or qty).
    pub config: serde_json::Value,
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Service>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn get_owned(pool: &PgPool, id: Uuid, customer_id: Uuid) -> AppResult<Option<Service>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1 AND s.customer_id = $2"
    )))
    .bind(id)
    .bind(customer_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn list(
    pool: &PgPool,
    customer: Option<Uuid>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Service>, i64)> {
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        s: Service,
        total_count: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS}, count(*) OVER() AS total_count FROM services s JOIN products p ON p.id = s.product_id
         WHERE ($1::uuid IS NULL OR s.customer_id = $1) AND ($2::text IS NULL OR s.status = $2)
         ORDER BY s.created_at DESC, s.id LIMIT $3 OFFSET $4"
    )))
    .bind(customer)
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = rows.first().map(|r| r.total_count).unwrap_or(0);
    Ok((rows.into_iter().map(|r| r.s).collect(), total))
}

/// Apply a lifecycle action (`suspend`, `unsuspend`, `terminate`, …).
/// Checked here and again by the DB trigger. Returns (old, new) service.
pub async fn transition(
    pool: &PgPool,
    id: Uuid,
    action: &str,
    reason: &str,
) -> AppResult<(String, Service)> {
    let to =
        lifecycle::action_target(action).ok_or(AppError::Validation("error.invalid_action"))?;
    if reason.len() > 500 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let from: String = sqlx::query_scalar("SELECT status FROM services WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    if !lifecycle::can_transition(&from, to) {
        return Err(AppError::Conflict("error.invalid_transition"));
    }
    sqlx::query(
        "UPDATE services SET status = $2, updated_at = now(),
            suspend_reason = CASE WHEN $2 = 'suspended' THEN $3 ELSE '' END,
            activated_at = CASE WHEN $2 = 'active' THEN COALESCE(activated_at, now()) ELSE activated_at END,
            terminated_at = CASE WHEN $2 IN ('terminated','cancelled','fraud') THEN now() ELSE terminated_at END
         WHERE id = $1",
    )
    .bind(id)
    .bind(to)
    .bind(reason)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    let s = get(pool, id).await?.ok_or(AppError::NotFound)?;
    Ok((from, s))
}

#[derive(Debug, Clone, Serialize)]
pub struct Renewal {
    pub service_id: Uuid,
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
}

/// Invoice every recurring service whose next due date is on/before
/// `horizon`, one period each, then advance its due date. Idempotent: a
/// period that already has an invoice line is skipped (unique index), so
/// re-running the job never double-bills.
pub async fn bill_due(
    pool: &PgPool,
    horizon: Date,
    batch: i64,
    due_days: i64,
) -> AppResult<Vec<Renewal>> {
    bill_due_ahead(
        pool,
        horizon - time::Duration::days(due_days),
        due_days,
        batch,
        due_days,
    )
    .await
}

/// Like [`bill_due`], but each service is invoiced `products.invoice_days_before`
/// days ahead of its due date (falling back to `default_days`). The client's
/// pending `next_invoice` billable items are added to the renewal invoice.
pub async fn bill_due_ahead(
    pool: &PgPool,
    today: Date,
    default_days: i64,
    batch: i64,
    due_days: i64,
) -> AppResult<Vec<Renewal>> {
    type DueRow = (Uuid, Uuid, String, String, Decimal, Date, String);
    let due: Vec<DueRow> = sqlx::query_as(
        "SELECT s.id, s.customer_id, s.cycle, s.currency::text, s.recurring_amount, s.next_due_date, p.name
         FROM services s JOIN products p ON p.id = s.product_id
         WHERE s.status IN ('active','suspended') AND s.cycle <> 'one_time'
           AND s.next_due_date <= $1::date + COALESCE(p.invoice_days_before, $3::int)
           AND (s.cancel_at IS NULL OR s.next_due_date < s.cancel_at)
         ORDER BY s.next_due_date, s.id LIMIT $2",
    )
    .bind(today)
    .bind(batch)
    .bind(default_days.clamp(0, 365) as i32)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::new();
    for (sid, cust, cyc, cur, amount, next, pname) in due {
        let Some((start, end)) = cycle::period(next, &cyc) else {
            continue;
        };
        let mut tx = pool.begin().await.map_err(db_err)?;
        // Re-check under lock: another worker may have advanced it.
        let still: Option<Date> = sqlx::query_scalar(
            "SELECT next_due_date FROM services WHERE id = $1 FOR UPDATE SKIP LOCKED",
        )
        .bind(sid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .flatten();
        if still != Some(next) {
            continue;
        }
        if let Some(inv) = renew_locked(
            &mut tx, sid, cust, &cur, amount, &pname, start, end, due_days,
        )
        .await?
        {
            out.push(Renewal {
                service_id: sid,
                invoice_id: inv,
                customer_id: cust,
            });
        }
        tx.commit().await.map_err(db_err)?;
    }
    Ok(out)
}

/// Invoice one period (`start`..`end`) of a locked service and advance its
/// due date. Returns the invoice when one was created (None = that period
/// was already billed; the due date still advances).
#[allow(clippy::too_many_arguments)]
async fn renew_locked(
    tx: &mut invoices::Tx<'_>,
    sid: Uuid,
    cust: Uuid,
    cur: &str,
    amount: Decimal,
    pname: &str,
    start: Date,
    end: Date,
    due_days: i64,
) -> AppResult<Option<Uuid>> {
    let mut created = None;
    let already: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoice_items WHERE service_id = $1 AND period_start = $2)",
    )
    .bind(sid)
    .bind(start)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    if !already {
        let inv = invoices::create_draft(tx, cust, cur, None).await?;
        invoices::add_items(
            tx,
            inv,
            cur,
            &[NewItem {
                description: format!("{pname} ({start}, {end})"),
                quantity: Decimal::ONE,
                unit_price: amount,
                tax_rate: Decimal::ZERO,
                service_id: Some(sid),
                period: Some((start, end)),
                auto_tax: true,
            }],
        )
        .await?;
        crate::billable::attach_next(tx, cust, cur, inv).await?;
        invoices::finalize(tx, inv, due_days).await?;
        created = Some(inv);
    }
    sqlx::query("UPDATE services SET next_due_date = $2, updated_at = now() WHERE id = $1")
        .bind(sid)
        .bind(end)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    if !already {
        // Limited recurring promo: count this renewal; restore the full price after the last one.
        sqlx::query(
            "UPDATE services SET promo_cycles_left = promo_cycles_left - 1 WHERE id = $1 AND promo_cycles_left > 0",
        )
        .bind(sid)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "UPDATE services SET recurring_amount = recurring_amount + promo_discount, promo_discount = 0, promo_cycles_left = NULL
             WHERE id = $1 AND promo_cycles_left = 0",
        )
        .bind(sid)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(created)
}

/// Outcome of an on-demand renewal.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EarlyRenewal {
    pub invoice_id: Uuid,
    /// ISO date.
    pub period_start: String,
    pub period_end: String,
}

/// On-demand renewal: invoice the next period now instead of waiting for
/// the cron. Refused while an unpaid invoice for this service is open, for
/// one-time/terminated/cancelling services, or more than `max_days_ahead`
/// days before the due date (0 = no limit). `customer` restricts ownership.
pub async fn renew_now(
    pool: &PgPool,
    sid: Uuid,
    customer: Option<Uuid>,
    max_days_ahead: i64,
    today: Date,
) -> AppResult<EarlyRenewal> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    type Row = (
        Uuid,
        String,
        String,
        Decimal,
        Option<Date>,
        String,
        String,
        Option<Date>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT s.customer_id, s.cycle, s.currency::text, s.recurring_amount, s.next_due_date, p.name, s.status, s.cancel_at
         FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1 FOR UPDATE OF s",
    )
    .bind(sid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (cust, cyc, cur, amount, next, pname, status, cancel_at) = row.ok_or(AppError::NotFound)?;
    if customer.is_some_and(|c| c != cust) {
        return Err(AppError::NotFound);
    }
    let Some(next) = next else {
        return Err(AppError::Conflict("error.renewal_not_allowed"));
    };
    if !matches!(status.as_str(), "active" | "suspended")
        || cyc == "one_time"
        || cancel_at.is_some_and(|c| next >= c)
    {
        return Err(AppError::Conflict("error.renewal_not_allowed"));
    }
    let open: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoice_items ii JOIN invoices i ON i.id = ii.invoice_id
                       WHERE ii.service_id = $1 AND i.status = 'unpaid' AND i.kind = 'invoice')",
    )
    .bind(sid)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if open {
        return Err(AppError::Conflict("error.renewal_pending"));
    }
    if max_days_ahead > 0 && (next - today).whole_days() > max_days_ahead {
        return Err(AppError::Conflict("error.renewal_too_early"));
    }
    let (start, end) =
        cycle::period(next, &cyc).ok_or(AppError::Conflict("error.renewal_not_allowed"))?;
    // Due on the current due date (today when that has already passed).
    let due_days = (next - today).whole_days().max(0);
    let inv = renew_locked(
        &mut tx, sid, cust, &cur, amount, &pname, start, end, due_days,
    )
    .await?
    .ok_or(AppError::Conflict("error.renewal_pending"))?;
    tx.commit().await.map_err(db_err)?;
    Ok(EarlyRenewal {
        invoice_id: inv,
        period_start: start.to_string(),
        period_end: end.to_string(),
    })
}
