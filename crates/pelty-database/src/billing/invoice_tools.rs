//! Bulk invoice helpers and pre-due invoice reminder settings/run.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

pub const DUE_KEY: &str = "billing.due_reminders";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DueReminderSettings {
    pub enabled: bool,
    /// Days before the due date on which a reminder is mailed (1..=60).
    pub days_before: Vec<i32>,
}

impl Default for DueReminderSettings {
    fn default() -> Self {
        Self { enabled: false, days_before: vec![3] }
    }
}

impl DueReminderSettings {
    pub fn normalized(mut self) -> AppResult<Self> {
        self.days_before.sort_unstable();
        self.days_before.dedup();
        if self.days_before.len() > 5 || self.days_before.iter().any(|d| !(1..=60).contains(d)) || (self.enabled && self.days_before.is_empty()) {
            return Err(AppError::Validation("error.invalid_reminder_days"));
        }
        Ok(self)
    }
}

pub async fn due_settings(pool: &PgPool) -> AppResult<DueReminderSettings> {
    Ok(crate::settings::get(pool, DUE_KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn set_due_settings(pool: &PgPool, s: DueReminderSettings) -> AppResult<DueReminderSettings> {
    let s = s.normalized()?;
    crate::settings::set(pool, DUE_KEY, serde_json::to_value(&s).map_err(AppError::internal)?).await?;
    Ok(s)
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct DueSoon {
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
    pub number: String,
    pub balance: Decimal,
    pub currency: String,
    pub due_date: Date,
    pub days_left: i32,
}

/// Unpaid invoices due within a configured threshold whose reminder for
/// that threshold was not sent yet. Marks them (once per invoice per run,
/// at the smallest crossed threshold) and returns what to mail.
pub async fn run_due_reminders(pool: &PgPool, today: Date) -> AppResult<Vec<DueSoon>> {
    let cfg = due_settings(pool).await?;
    if !cfg.enabled || cfg.days_before.is_empty() {
        return Ok(vec![]);
    }
    let max = *cfg.days_before.iter().max().unwrap_or(&0);
    let rows: Vec<DueSoon> = sqlx::query_as(
        "SELECT i.id AS invoice_id, i.customer_id, COALESCE(i.number, 'PF-' || upper(left(i.id::text, 8))) AS number, (i.total - i.amount_paid) AS balance, i.currency::text AS currency, \
         (i.due_at AT TIME ZONE 'UTC')::date AS due_date, ((i.due_at AT TIME ZONE 'UTC')::date - $1)::int AS days_left \
         FROM invoices i WHERE i.status = 'unpaid' AND i.kind IN ('invoice', 'debit_note') AND i.finalized_at IS NOT NULL AND i.total > i.amount_paid \
         AND (i.due_at AT TIME ZONE 'UTC')::date BETWEEN $1 + 1 AND $1 + $2 ORDER BY i.due_at LIMIT 2000",
    )
    .bind(today)
    .bind(max)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = vec![];
    for r in rows {
        let mut newly = false;
        for d in cfg.days_before.iter().filter(|d| r.days_left <= **d) {
            let n = sqlx::query("INSERT INTO invoice_due_reminders (invoice_id, days) VALUES ($1,$2) ON CONFLICT DO NOTHING")
                .bind(r.invoice_id)
                .bind(d)
                .execute(pool)
                .await
                .map_err(db_err)?
                .rows_affected();
            newly |= n > 0;
        }
        if newly {
            out.push(r);
        }
    }
    Ok(out)
}

/// Remaining balance and status of invoices (for bulk actions).
pub async fn balances(pool: &PgPool, ids: &[Uuid]) -> AppResult<Vec<(Uuid, String, Decimal, Uuid)>> {
    sqlx::query_as("SELECT id, status, GREATEST(total - amount_paid, 0), customer_id FROM invoices WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes() {
        let s = DueReminderSettings { enabled: true, days_before: vec![7, 3, 7] }.normalized().unwrap();
        assert_eq!(s.days_before, vec![3, 7]);
        assert!(DueReminderSettings { enabled: true, days_before: vec![] }.normalized().is_err());
        assert!(DueReminderSettings { enabled: false, days_before: vec![0] }.normalized().is_err());
        assert!(DueReminderSettings { enabled: false, days_before: vec![] }.normalized().is_ok());
    }
}
