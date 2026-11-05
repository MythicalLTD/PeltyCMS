//! Overdue invoice automation: reminders, a late fee as a separate invoice,
//! suspension of linked services, and unsuspension once paid. Every action
//! is recorded in `overdue_actions` (unique per invoice + action), so a run
//! is idempotent and safe to repeat.
use crate::{
    db_err,
    invoices::{self, NewItem},
    settings,
};
use pelty_billing::math;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

pub const KEY: &str = "billing.overdue";
/// Services suspended by this automation carry this reason.
pub const REASON: &str = "overdue";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OverdueSettings {
    pub enabled: bool,
    /// Days after the due date on which a reminder is sent.
    pub reminder_days: Vec<i32>,
    /// Days overdue before the late fee is charged (0 = never).
    pub late_fee_days: i32,
    #[schema(value_type = String)]
    pub late_fee_fixed: Decimal,
    /// Percent of the open balance.
    #[schema(value_type = String)]
    pub late_fee_percent: Decimal,
    /// Days overdue before linked services are suspended (0 = never).
    pub suspend_days: i32,
    /// Days overdue before services suspended by this automation are
    /// terminated (0 = never; WHMCS "Termination Days").
    pub terminate_days: i32,
    /// Days overdue before an unpaid invoice that received no money is voided
    /// (0 = never; WHMCS "Invoice Auto Cancellation").
    pub cancel_unpaid_days: i32,
}

impl Default for OverdueSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            reminder_days: vec![1, 7, 14],
            late_fee_days: 0,
            late_fee_fixed: Decimal::ZERO,
            late_fee_percent: Decimal::ZERO,
            suspend_days: 14,
            terminate_days: 0,
            cancel_unpaid_days: 0,
        }
    }
}

impl OverdueSettings {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        self.reminder_days.sort_unstable();
        self.reminder_days.dedup();
        let day = |d: i32| (1..=365).contains(&d);
        if self.reminder_days.len() > 10 || !self.reminder_days.iter().all(|d| day(*d)) {
            return Err("error.invalid_days");
        }
        if [
            self.late_fee_days,
            self.suspend_days,
            self.terminate_days,
            self.cancel_unpaid_days,
        ]
        .iter()
        .any(|d| !(0..=730).contains(d))
            || (self.terminate_days > 0
                && self.suspend_days > 0
                && self.terminate_days <= self.suspend_days)
        {
            return Err("error.invalid_days");
        }
        if self.late_fee_fixed < Decimal::ZERO
            || self.late_fee_fixed > Decimal::from(1_000_000)
            || self.late_fee_percent < Decimal::ZERO
            || self.late_fee_percent > Decimal::from(100)
        {
            return Err("error.invalid_amount");
        }
        Ok(self)
    }
}

pub async fn get_settings(pool: &PgPool) -> AppResult<OverdueSettings> {
    Ok(settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put_settings(pool: &PgPool, s: &OverdueSettings) -> AppResult<()> {
    settings::set(
        pool,
        KEY,
        serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

#[derive(Debug, Clone, Serialize)]
pub struct Reminder {
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
    pub number: String,
    pub balance: Decimal,
    pub currency: String,
    pub due_date: Date,
    pub days_overdue: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LateFee {
    pub invoice_id: Uuid,
    pub fee_invoice_id: Uuid,
    pub customer_id: Uuid,
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suspended {
    pub service_id: Uuid,
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub reminders: Vec<Reminder>,
    pub late_fees: Vec<LateFee>,
    pub suspended: Vec<Suspended>,
    pub terminated: Vec<Suspended>,
    pub voided: Vec<Uuid>,
}

/// Late fee for an open balance, rounded to the currency exponent.
pub fn late_fee(s: &OverdueSettings, balance: Decimal, exponent: u32) -> Decimal {
    math::round(
        s.late_fee_fixed + balance * s.late_fee_percent / Decimal::from(100),
        exponent,
    )
}

type Row = (Uuid, Uuid, String, Decimal, Decimal, String, Date);

/// Run all overdue actions due on `today`.
pub async fn run_overdue(pool: &PgPool, today: Date) -> AppResult<Report> {
    let cfg = get_settings(pool).await?;
    let mut report = Report::default();
    if !cfg.enabled {
        return Ok(report);
    }
    // Late-fee invoices are not themselves charged fees or reminded twice over.
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT i.id, i.customer_id, COALESCE(i.number, 'PF-' || upper(left(i.id::text, 8))), i.total, i.amount_paid, i.currency::text, i.due_at::date
         FROM invoices i
         WHERE i.kind IN ('invoice', 'debit_note') AND i.status = 'unpaid' AND i.finalized_at IS NOT NULL
           AND i.due_at IS NOT NULL AND i.due_at::date < $1 AND i.total > i.amount_paid
           AND NOT EXISTS (SELECT 1 FROM overdue_actions a WHERE a.ref_id = i.id)
         ORDER BY i.due_at, i.id",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for (id, customer_id, number, total, paid, currency, due) in rows {
        let days = (today - due).whole_days();
        let balance = total - paid;

        // Reminders: record every crossed threshold, mail once per run.
        let mut newly = false;
        for d in cfg.reminder_days.iter().filter(|d| i64::from(**d) <= days) {
            newly |= mark(pool, id, &format!("reminder:{d}"), None).await?;
        }
        if newly {
            report.reminders.push(Reminder {
                invoice_id: id,
                customer_id,
                number: number.clone(),
                balance,
                currency: currency.clone(),
                due_date: due,
                days_overdue: days,
            });
        }

        if cfg.late_fee_days > 0
            && days >= i64::from(cfg.late_fee_days)
            && !late_fee_exempt(pool, customer_id).await?
        {
            if let Some(f) =
                charge_late_fee(pool, &cfg, id, customer_id, &number, balance, &currency).await?
            {
                report.late_fees.push(f);
            }
        }

        if cfg.suspend_days > 0
            && days >= i64::from(cfg.suspend_days)
            && mark(pool, id, "suspend", None).await?
        {
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "UPDATE services SET status = 'suspended', suspend_reason = $2, updated_at = now()
                 WHERE status = 'active' AND id IN (SELECT service_id FROM invoice_items WHERE invoice_id = $1 AND service_id IS NOT NULL)
                 RETURNING id",
            )
            .bind(id)
            .bind(REASON)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
            report
                .suspended
                .extend(ids.into_iter().map(|service_id| Suspended {
                    service_id,
                    invoice_id: id,
                    customer_id,
                }));
        }

        // Only services this automation suspended are terminated; a manual
        // suspension (other reason) is left to staff, as in WHMCS.
        if cfg.terminate_days > 0
            && days >= i64::from(cfg.terminate_days)
            && mark(pool, id, "terminate", None).await?
        {
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "UPDATE services SET status = 'terminated', updated_at = now()
                 WHERE status = 'suspended' AND suspend_reason = $2
                   AND id IN (SELECT service_id FROM invoice_items WHERE invoice_id = $1 AND service_id IS NOT NULL)
                 RETURNING id",
            )
            .bind(id)
            .bind(REASON)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
            report
                .terminated
                .extend(ids.into_iter().map(|service_id| Suspended {
                    service_id,
                    invoice_id: id,
                    customer_id,
                }));
        }

        if cfg.cancel_unpaid_days > 0
            && days >= i64::from(cfg.cancel_unpaid_days)
            && paid == Decimal::ZERO
            && mark(pool, id, "auto_void", None).await?
        {
            let n = sqlx::query("UPDATE invoices SET status = 'void' WHERE id = $1 AND status = 'unpaid' AND amount_paid = 0")
                .bind(id)
                .execute(pool)
                .await
                .map_err(db_err)?
                .rows_affected();
            if n == 1 {
                report.voided.push(id);
            }
        }
    }
    Ok(report)
}

/// Record an action; false if it was already recorded.
/// Client groups can waive late fees for their members.
async fn late_fee_exempt(pool: &PgPool, customer_id: Uuid) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT COALESCE(g.late_fee_exempt, false) FROM customers c LEFT JOIN client_groups g ON g.id = c.client_group_id WHERE c.id = $1",
    )
    .bind(customer_id)
    .fetch_optional(pool)
    .await
    .map(|v| v.unwrap_or(false))
    .map_err(db_err)
}

async fn mark(
    pool: &PgPool,
    invoice_id: Uuid,
    action: &str,
    ref_id: Option<Uuid>,
) -> AppResult<bool> {
    let r = sqlx::query("INSERT INTO overdue_actions (invoice_id, action, ref_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(invoice_id)
        .bind(action)
        .bind(ref_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

async fn charge_late_fee(
    pool: &PgPool,
    cfg: &OverdueSettings,
    invoice_id: Uuid,
    customer_id: Uuid,
    number: &str,
    balance: Decimal,
    currency: &str,
) -> AppResult<Option<LateFee>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let claimed = sqlx::query("INSERT INTO overdue_actions (invoice_id, action) VALUES ($1, 'late_fee') ON CONFLICT DO NOTHING")
        .bind(invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected()
        == 1;
    if !claimed {
        return Ok(None);
    }
    let exp = invoices::exponent(&mut tx, currency).await?;
    let amount = late_fee(cfg, balance, exp);
    if amount <= Decimal::ZERO {
        // Recorded (so it is not retried daily) but nothing to charge.
        tx.commit().await.map_err(db_err)?;
        return Ok(None);
    }
    let fee = invoices::create_draft(&mut tx, customer_id, currency, None).await?;
    let line = NewItem {
        description: format!("Late fee for invoice {number}"),
        quantity: Decimal::ONE,
        unit_price: amount,
        tax_rate: Decimal::ZERO,
        service_id: None,
        period: None,
        auto_tax: false,
    };
    invoices::add_items(&mut tx, fee, currency, &[line]).await?;
    invoices::finalize(&mut tx, fee, 7).await?;
    sqlx::query(
        "UPDATE overdue_actions SET ref_id = $2 WHERE invoice_id = $1 AND action = 'late_fee'",
    )
    .bind(invoice_id)
    .bind(fee)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(LateFee {
        invoice_id,
        fee_invoice_id: fee,
        customer_id,
        amount,
    }))
}

/// After `invoice_id` is paid: reactivate services this automation suspended
/// for it, unless another suspended-for invoice of theirs is still unpaid.
pub async fn after_payment(pool: &PgPool, invoice_id: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "UPDATE services s SET status = 'active', suspend_reason = '', updated_at = now()
         WHERE s.status = 'suspended' AND s.suspend_reason = $2
           AND s.id IN (SELECT service_id FROM invoice_items WHERE invoice_id = $1 AND service_id IS NOT NULL)
           AND NOT EXISTS (
               SELECT 1 FROM invoice_items ii
               JOIN invoices i ON i.id = ii.invoice_id AND i.status = 'unpaid'
               JOIN overdue_actions a ON a.invoice_id = i.id AND a.action = 'suspend'
               WHERE ii.service_id = s.id)
         RETURNING s.id",
    )
    .bind(invoice_id)
    .bind(REASON)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// (email, display name, preferred locale) for mailing a customer.
pub async fn recipient(
    pool: &PgPool,
    customer_id: Uuid,
) -> AppResult<Option<(String, String, Option<String>)>> {
    sqlx::query_as(
        "SELECT c.email::text, COALESCE(NULLIF(trim(c.first_name || ' ' || c.last_name), ''), c.email::text), u.locale
         FROM customers c LEFT JOIN users u ON u.id = c.owner_user_id WHERE c.id = $1",
    )
    .bind(customer_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn fee_is_fixed_plus_percent_rounded() {
        let s = OverdueSettings {
            late_fee_fixed: Decimal::from(5),
            late_fee_percent: Decimal::from_str("2.5").unwrap(),
            ..Default::default()
        };
        assert_eq!(
            late_fee(&s, Decimal::from_str("123.45").unwrap(), 2),
            Decimal::from_str("8.09").unwrap()
        );
        assert_eq!(
            late_fee(&s, Decimal::from_str("10").unwrap(), 0),
            Decimal::from(5)
        );
    }

    #[test]
    fn settings_validation() {
        let ok = OverdueSettings {
            reminder_days: vec![7, 1, 7],
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(ok.reminder_days, vec![1, 7]);
        assert!(OverdueSettings {
            reminder_days: vec![0],
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(OverdueSettings {
            late_fee_percent: Decimal::from(101),
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(OverdueSettings {
            late_fee_fixed: Decimal::from(-1),
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(OverdueSettings {
            suspend_days: 800,
            ..Default::default()
        }
        .normalized()
        .is_err());
    }
}
