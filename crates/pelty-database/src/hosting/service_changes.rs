//! Plan changes (upgrade / downgrade) and cancellation requests.
//!
//! * Upgrade: prorated for the rest of the current period (credit for the
//!   unused old plan, charge for the new plan; exact decimals, currency
//!   rounding). An invoice is issued for the net amount plus the price
//!   difference of any period already billed ahead; the change applies
//!   when that invoice is paid (immediately when it is zero).
//! * Downgrade: scheduled for the end of the last billed period (no
//!   refund); the new price is used for every renewal after it.
//! * Cancellation: client request (`immediate` or `end_of_period`),
//!   approved or rejected by staff; end-of-period cancellations stop
//!   renewals after `cancel_at` and complete in [`sweep`].
use crate::db_err;
use crate::invoices::{self, Invoice, NewItem};
use pelty_billing::{cycle, lifecycle, proration};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Change {
    pub id: Uuid,
    pub service_id: Uuid,
    pub kind: String,
    pub from_product: Uuid,
    pub to_product: Uuid,
    pub to_product_name: String,
    pub old_amount: Decimal,
    pub new_amount: Decimal,
    pub credit: Decimal,
    pub charge: Decimal,
    pub period_start: Option<Date>,
    pub period_end: Option<Date>,
    pub invoice_id: Option<Uuid>,
    pub status: String,
    pub effective_date: Option<Date>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub applied_at: Option<OffsetDateTime>,
}

const CHANGE_COLS: &str = "c.id, c.service_id, c.kind, c.from_product, c.to_product, p.name AS to_product_name, c.old_amount, c.new_amount, \
    c.credit, c.charge, c.period_start, c.period_end, c.invoice_id, c.status, c.effective_date, c.created_at, c.applied_at";

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Cancellation {
    pub id: Uuid,
    pub service_id: Uuid,
    pub customer_id: Uuid,
    pub product_name: String,
    pub customer_email: String,
    pub kind: String,
    pub reason: String,
    pub status: String,
    pub effective_date: Option<Date>,
    pub decision_note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub decided_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

const CANCEL_COLS: &str = "r.id, r.service_id, r.customer_id, p.name AS product_name, cu.email::text AS customer_email, r.kind, r.reason, \
    r.status, r.effective_date, r.decision_note, r.created_at, r.decided_at, r.completed_at";
const CANCEL_FROM: &str = "cancellation_requests r JOIN services s ON s.id = r.service_id JOIN products p ON p.id = s.product_id \
    JOIN customers cu ON cu.id = r.customer_id";

/// A product the service can move to, with a preview of the change.
#[derive(Debug, Clone, Serialize)]
pub struct ChangeOption {
    pub product_id: Uuid,
    pub name: String,
    pub amount: Decimal,
    pub kind: &'static str,
    /// Upgrade: amount due now. Downgrade: zero (applies at `effective_date`).
    pub due_now: Decimal,
    pub proration: Option<proration::Proration>,
    pub effective_date: Option<Date>,
}

struct Svc {
    customer_id: Uuid,
    product_id: Uuid,
    group_id: Uuid,
    status: String,
    cycle: String,
    currency: String,
    amount: Decimal,
    next_due: Option<Date>,
    name: String,
}

async fn load(pool: &PgPool, id: Uuid) -> AppResult<Svc> {
    type Row = (
        Uuid,
        Uuid,
        Uuid,
        String,
        String,
        String,
        Decimal,
        Option<Date>,
        String,
    );
    let r: Option<Row> = sqlx::query_as(
        "SELECT s.customer_id, s.product_id, p.group_id, s.status, s.cycle, s.currency::text, s.recurring_amount, s.next_due_date, p.name
         FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (customer_id, product_id, group_id, status, cycle, currency, amount, next_due, name) =
        r.ok_or(AppError::NotFound)?;
    Ok(Svc {
        customer_id,
        product_id,
        group_id,
        status,
        cycle,
        currency,
        amount,
        next_due,
        name,
    })
}

async fn exponent(pool: &PgPool, currency: &str) -> AppResult<u32> {
    let e: Option<i16> = sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1")
        .bind(currency)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(e.unwrap_or(2).max(0) as u32)
}

/// The billed period containing `today` (from invoice lines), else the
/// period ending at `next_due_date`.
async fn current_period(
    pool: &PgPool,
    sid: Uuid,
    s: &Svc,
    today: Date,
) -> AppResult<Option<(Date, Date)>> {
    let billed: Option<(Date, Date)> = sqlx::query_as(
        "SELECT i.period_start, i.period_end FROM invoice_items i JOIN invoices v ON v.id = i.invoice_id
         WHERE i.service_id = $1 AND v.status IN ('unpaid','paid') AND i.period_start <= $2 AND i.period_end > $2
         ORDER BY i.period_start DESC LIMIT 1",
    )
    .bind(sid)
    .bind(today)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    if billed.is_some() {
        return Ok(billed);
    }
    let (Some(next), Some(m)) = (s.next_due, cycle::months(&s.cycle)) else {
        return Ok(None);
    };
    let start = cycle::sub_months(next, m);
    Ok((start <= today && today < next).then_some((start, next)))
}

/// Count of whole periods already billed after `after` (renewals issued ahead).
async fn billed_ahead(pool: &PgPool, sid: Uuid, after: Date) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT i.period_start) FROM invoice_items i JOIN invoices v ON v.id = i.invoice_id
         WHERE i.service_id = $1 AND v.status IN ('unpaid','paid') AND i.period_start >= $2",
    )
    .bind(sid)
    .bind(after)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

fn changeable(s: &Svc) -> AppResult<()> {
    if s.status != "active" || cycle::months(&s.cycle).is_none() {
        return Err(AppError::Conflict("error.service_not_changeable"));
    }
    Ok(())
}

/// Products in the same group with a price for the service's currency/cycle.
pub async fn options(pool: &PgPool, sid: Uuid, today: Date) -> AppResult<Vec<ChangeOption>> {
    let s = load(pool, sid).await?;
    changeable(&s)?;
    let exp = exponent(pool, &s.currency).await?;
    let rows: Vec<(Uuid, String, Decimal)> = sqlx::query_as(
        "SELECT p.id, p.name, pp.amount FROM products p JOIN product_prices pp ON pp.product_id = p.id
         WHERE p.group_id = $1 AND p.id <> $2 AND p.status = 'active' AND pp.currency = $3 AND pp.cycle = $4
         ORDER BY pp.amount, p.sort_order, p.name",
    )
    .bind(s.group_id)
    .bind(s.product_id)
    .bind(&s.currency)
    .bind(&s.cycle)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    // Addons stay on the service across a plan change, so they are part of
    // the new recurring price; the old product's configurable options are not.
    let addons: Decimal = sqlx::query_scalar("SELECT COALESCE(sum(recurring_amount), 0) FROM service_addons WHERE service_id = $1 AND status = 'active'")
        .bind(sid)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let period = current_period(pool, sid, &s, today).await?;
    let ahead = match period {
        Some((_, end)) => billed_ahead(pool, sid, end).await?,
        None => 0,
    };
    Ok(rows
        .into_iter()
        .map(|(product_id, name, base)| {
            let amount = base + addons;
            if amount > s.amount {
                let p = period
                    .and_then(|(a, b)| proration::prorate(s.amount, amount, a, b, today, exp));
                let due = p.as_ref().map(|p| p.net).unwrap_or_default()
                    + (amount - s.amount) * Decimal::from(ahead);
                ChangeOption {
                    product_id,
                    name,
                    amount,
                    kind: "upgrade",
                    due_now: due,
                    proration: p,
                    effective_date: None,
                }
            } else {
                ChangeOption {
                    product_id,
                    name,
                    amount,
                    kind: "downgrade",
                    due_now: Decimal::ZERO,
                    proration: None,
                    effective_date: s.next_due,
                }
            }
        })
        .collect())
}

/// Request a plan change. Returns the change and the upgrade invoice (if any).
pub async fn request_change(
    pool: &PgPool,
    sid: Uuid,
    to: Uuid,
    by: Option<Uuid>,
    today: Date,
    due_days: i64,
) -> AppResult<(Change, Option<Invoice>)> {
    let s = load(pool, sid).await?;
    let opt = options(pool, sid, today)
        .await?
        .into_iter()
        .find(|o| o.product_id == to)
        .ok_or(AppError::Validation("error.invalid_product"))?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    // Serialize with renewals / other changes on this service.
    sqlx::query("SELECT 1 FROM services WHERE id = $1 FOR UPDATE")
        .bind(sid)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let id = Uuid::new_v4();
    let mut invoice = None;
    let status = if opt.kind == "upgrade" {
        let p = opt
            .proration
            .clone()
            .ok_or(AppError::Conflict("error.service_not_changeable"))?;
        let (start, end) = current_period(pool, sid, &s, today)
            .await?
            .ok_or(AppError::Conflict("error.service_not_changeable"))?;
        let ahead = billed_ahead(pool, sid, end).await?;
        let mut items = vec![
            NewItem {
                description: format!("{} → {} ({today}, {end})", s.name, opt.name),
                quantity: Decimal::ONE,
                unit_price: p.charge,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            },
            NewItem {
                description: format!("{}, unused ({today}, {end})", s.name),
                quantity: Decimal::ONE,
                unit_price: -p.credit,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            },
        ];
        if ahead > 0 {
            items.push(NewItem {
                description: format!("{} → {} (billed periods after {end})", s.name, opt.name),
                quantity: Decimal::from(ahead),
                unit_price: opt.amount - s.amount,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            });
        }
        let inv = invoices::create_draft(&mut tx, s.customer_id, &s.currency, None).await?;
        invoices::add_items(&mut tx, inv, &s.currency, &items).await?;
        let fin = invoices::finalize(&mut tx, inv, due_days).await?;
        sqlx::query(
            "INSERT INTO service_changes (id, service_id, kind, from_product, to_product, old_amount, new_amount, credit, charge,
                period_start, period_end, invoice_id, status, requested_by)
             VALUES ($1,$2,'upgrade',$3,$4,$5,$6,$7,$8,$9,$10,$11,'awaiting_payment',$12)",
        )
        .bind(id)
        .bind(sid)
        .bind(s.product_id)
        .bind(to)
        .bind(s.amount)
        .bind(opt.amount)
        .bind(p.credit)
        .bind(p.charge)
        .bind(start)
        .bind(end)
        .bind(inv)
        .bind(by)
        .execute(&mut *tx)
        .await
        .map_err(|e| unique_conflict(e, "error.change_pending"))?;
        let paid = fin.status == "paid";
        invoice = Some(fin);
        if paid {
            "paid"
        } else {
            "awaiting_payment"
        }
    } else {
        let eff = s
            .next_due
            .ok_or(AppError::Conflict("error.service_not_changeable"))?;
        sqlx::query(
            "INSERT INTO service_changes (id, service_id, kind, from_product, to_product, old_amount, new_amount, status, effective_date, requested_by)
             VALUES ($1,$2,'downgrade',$3,$4,$5,$6,'scheduled',$7,$8)",
        )
        .bind(id)
        .bind(sid)
        .bind(s.product_id)
        .bind(to)
        .bind(s.amount)
        .bind(opt.amount)
        .bind(eff)
        .bind(by)
        .execute(&mut *tx)
        .await
        .map_err(|e| unique_conflict(e, "error.change_pending"))?;
        // Every renewal from `eff` on is billed at the new price.
        sqlx::query("UPDATE services SET recurring_amount = $2, updated_at = now() WHERE id = $1")
            .bind(sid)
            .bind(opt.amount)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        "scheduled"
    };
    if status == "paid" {
        apply(&mut tx, id).await?;
    }
    tx.commit().await.map_err(db_err)?;
    let c = get_change(pool, id).await?.ok_or(AppError::NotFound)?;
    Ok((c, invoice))
}

fn unique_conflict(e: sqlx::Error, code: &'static str) -> AppError {
    if let sqlx::Error::Database(d) = &e {
        if d.code().as_deref() == Some("23505") {
            return AppError::Conflict(code);
        }
    }
    db_err(e)
}

/// Switch the service to the target product (and price for upgrades).
async fn apply(tx: &mut invoices::Tx<'_>, change: Uuid) -> AppResult<Option<Uuid>> {
    let r: Option<(Uuid, Uuid, Decimal)> = sqlx::query_as(
        "UPDATE service_changes SET status = 'applied', applied_at = now()
         WHERE id = $1 AND status IN ('awaiting_payment','scheduled') RETURNING service_id, to_product, new_amount",
    )
    .bind(change)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let Some((sid, product, amount)) = r else {
        return Ok(None);
    };
    sqlx::query("UPDATE services SET product_id = $2, recurring_amount = $3, updated_at = now() WHERE id = $1")
        .bind(sid)
        .bind(product)
        .bind(amount)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(Some(sid))
}

/// Apply upgrades whose invoice is now paid. Returns changed service ids.
pub async fn after_payment(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Uuid>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT c.id FROM service_changes c JOIN invoices v ON v.id = c.invoice_id
         WHERE c.invoice_id = $1 AND c.status = 'awaiting_payment' AND v.status = 'paid'",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    apply_all(pool, ids).await
}

async fn apply_all(pool: &PgPool, ids: Vec<Uuid>) -> AppResult<Vec<Uuid>> {
    let mut out = Vec::new();
    for id in ids {
        let mut tx = pool.begin().await.map_err(db_err)?;
        if let Some(sid) = apply(&mut tx, id).await? {
            out.push(sid);
        }
        tx.commit().await.map_err(db_err)?;
    }
    Ok(out)
}

/// Withdraw a pending change. An unpaid upgrade invoice is voided; a
/// scheduled downgrade restores the old price.
pub async fn cancel_change(pool: &PgPool, sid: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r: Option<(Uuid, String, Option<Uuid>, Decimal)> = sqlx::query_as(
        "UPDATE service_changes SET status = 'cancelled' WHERE service_id = $1 AND status IN ('awaiting_payment','scheduled')
         RETURNING id, kind, invoice_id, old_amount",
    )
    .bind(sid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (_, kind, inv, old) = r.ok_or(AppError::NotFound)?;
    if kind == "downgrade" {
        sqlx::query("UPDATE services SET recurring_amount = $2, updated_at = now() WHERE id = $1")
            .bind(sid)
            .bind(old)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    if let Some(inv) = inv {
        // Only unpaid invoices can be voided; a paid one was already applied.
        if let Some(i) = invoices::get(pool, inv).await? {
            if i.status == "unpaid" {
                invoices::void(pool, inv).await?;
            }
        }
    }
    Ok(())
}

pub async fn get_change(pool: &PgPool, id: Uuid) -> AppResult<Option<Change>> {
    sqlx::query_as(&format!("SELECT {CHANGE_COLS} FROM service_changes c JOIN products p ON p.id = c.to_product WHERE c.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn changes_for(pool: &PgPool, sid: Uuid) -> AppResult<Vec<Change>> {
    sqlx::query_as(&format!(
        "SELECT {CHANGE_COLS} FROM service_changes c JOIN products p ON p.id = c.to_product WHERE c.service_id = $1 ORDER BY c.created_at DESC LIMIT 20"
    ))
    .bind(sid)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// cancellation

pub const CANCEL_KINDS: &[&str] = &["immediate", "end_of_period"];
pub const CANCEL_STATUSES: &[&str] = &["pending", "approved", "rejected", "withdrawn", "completed"];

pub async fn request_cancel(
    pool: &PgPool,
    sid: Uuid,
    kind: &str,
    reason: &str,
    by: Option<Uuid>,
) -> AppResult<Cancellation> {
    if !CANCEL_KINDS.contains(&kind) {
        return Err(AppError::Validation("error.invalid_kind"));
    }
    if reason.chars().count() > 2000 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let s = load(pool, sid).await?;
    if lifecycle::is_terminal(&s.status) {
        return Err(AppError::Conflict("error.invalid_transition"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO cancellation_requests (id, service_id, customer_id, kind, reason, requested_by) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(sid)
        .bind(s.customer_id)
        .bind(kind)
        .bind(reason.trim())
        .bind(by)
        .execute(pool)
        .await
        .map_err(|e| unique_conflict(e, "error.cancellation_pending"))?;
    get_cancel(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn get_cancel(pool: &PgPool, id: Uuid) -> AppResult<Option<Cancellation>> {
    sqlx::query_as(&format!(
        "SELECT {CANCEL_COLS} FROM {CANCEL_FROM} WHERE r.id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Open (pending/approved) request of a service.
pub async fn open_cancel(pool: &PgPool, sid: Uuid) -> AppResult<Option<Cancellation>> {
    sqlx::query_as(&format!("SELECT {CANCEL_COLS} FROM {CANCEL_FROM} WHERE r.service_id = $1 AND r.status IN ('pending','approved')"))
        .bind(sid)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn list_cancels(
    pool: &PgPool,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Cancellation>, i64)> {
    let rows = sqlx::query_as(&format!(
        "SELECT {CANCEL_COLS} FROM {CANCEL_FROM} WHERE ($1::text IS NULL OR r.status = $1) ORDER BY r.created_at DESC, r.id LIMIT $2 OFFSET $3"
    ))
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cancellation_requests WHERE ($1::text IS NULL OR status = $1)",
    )
    .bind(status)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((rows, total))
}

/// Client withdraws a pending (or approved, not yet effective) request.
pub async fn withdraw_cancel(pool: &PgPool, sid: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let n = sqlx::query("UPDATE cancellation_requests SET status = 'withdrawn' WHERE service_id = $1 AND status IN ('pending','approved')")
        .bind(sid)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("UPDATE services SET cancel_at = NULL, updated_at = now() WHERE id = $1")
        .bind(sid)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

/// Outcome of a decision: `cancel_now` means the caller must transition
/// the service to `cancelled` (and run provisioning hooks).
pub struct Decision {
    pub request: Cancellation,
    pub cancel_now: bool,
}

pub async fn decide(
    pool: &PgPool,
    id: Uuid,
    approve: bool,
    note: &str,
    by: Uuid,
) -> AppResult<Decision> {
    if note.chars().count() > 2000 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r: Option<(Uuid, String, String, Option<Date>)> = sqlx::query_as(
        "SELECT r.service_id, r.kind, r.status, s.next_due_date FROM cancellation_requests r JOIN services s ON s.id = r.service_id
         WHERE r.id = $1 FOR UPDATE OF r",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (sid, kind, status, next_due) = r.ok_or(AppError::NotFound)?;
    if status != "pending" {
        return Err(AppError::Conflict("error.invalid_status"));
    }
    let today = OffsetDateTime::now_utc().date();
    let (new_status, eff, cancel_now) = match (approve, kind.as_str(), next_due) {
        (false, _, _) => ("rejected", None, false),
        (true, "end_of_period", Some(d)) if d > today => ("approved", Some(d), false),
        // Immediate, or nothing left to wait for.
        (true, _, _) => ("completed", Some(today), true),
    };
    sqlx::query(
        "UPDATE cancellation_requests SET status = $2, effective_date = $3, decision_note = $4, decided_by = $5, decided_at = now(),
            completed_at = CASE WHEN $2 = 'completed' THEN now() END WHERE id = $1",
    )
    .bind(id)
    .bind(new_status)
    .bind(eff)
    .bind(note.trim())
    .bind(by)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if new_status == "approved" {
        sqlx::query("UPDATE services SET cancel_at = $2, updated_at = now() WHERE id = $1")
            .bind(sid)
            .bind(eff)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    let request = get_cancel(pool, id).await?.ok_or(AppError::NotFound)?;
    Ok(Decision {
        request,
        cancel_now,
    })
}

/// Due work for `today`: apply scheduled downgrades and return services
/// whose approved end-of-period cancellation is now effective (the caller
/// transitions them so provisioning hooks run).
pub async fn sweep(pool: &PgPool, today: Date) -> AppResult<(Vec<Uuid>, Vec<Uuid>)> {
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM service_changes WHERE status = 'scheduled' AND effective_date <= $1",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut changed = apply_all(pool, due).await?;
    // Fallback for upgrades whose payment hook did not run.
    let paid: Vec<Uuid> = sqlx::query_scalar(
        "SELECT c.id FROM service_changes c JOIN invoices v ON v.id = c.invoice_id WHERE c.status = 'awaiting_payment' AND v.status = 'paid'",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    changed.extend(apply_all(pool, paid).await?);
    let cancel: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE cancellation_requests SET status = 'completed', completed_at = now()
         WHERE status = 'approved' AND effective_date <= $1 RETURNING service_id",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok((changed, cancel))
}
