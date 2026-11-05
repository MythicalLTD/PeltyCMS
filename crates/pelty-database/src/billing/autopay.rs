//! Saved payment methods and auto-charge of due invoices. Attempts are unique
//! per (invoice, attempt index), so a run is idempotent and two workers can
//! never charge the same attempt twice.
use crate::{db_err, settings};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

pub const KEY: &str = "billing.autopay";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AutopaySettings {
    pub enabled: bool,
    /// Days after the due date of each attempt; the first is usually 0.
    pub retry_days: Vec<i32>,
}

impl Default for AutopaySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            retry_days: vec![0, 3, 7],
        }
    }
}

impl AutopaySettings {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        self.retry_days.sort_unstable();
        self.retry_days.dedup();
        if self.retry_days.is_empty()
            || self.retry_days.len() > 10
            || !self.retry_days.iter().all(|d| (0..=90).contains(d))
        {
            return Err("error.invalid_days");
        }
        Ok(self)
    }
}

pub async fn get_settings(pool: &PgPool) -> AppResult<AutopaySettings> {
    Ok(settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put_settings(pool: &PgPool, s: &AutopaySettings) -> AppResult<()> {
    settings::set(
        pool,
        KEY,
        serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

/// Index of the attempt due `today`, given how many were already made.
pub fn due_attempt(due: Date, retry_days: &[i32], made: usize, today: Date) -> Option<usize> {
    let d = *retry_days.get(made)?;
    (today >= due + Duration::days(d.into())).then_some(made)
}

/// Date of the attempt after `attempt`, if any.
pub fn next_retry(due: Date, retry_days: &[i32], attempt: usize) -> Option<Date> {
    retry_days
        .get(attempt + 1)
        .map(|d| due + Duration::days((*d).into()))
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct PaymentMethod {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub gateway: String,
    #[serde(skip)]
    pub token: String,
    pub brand: String,
    pub last4: String,
    pub exp_month: i16,
    pub exp_year: i16,
    pub is_default: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

const PM: &str =
    "id, customer_id, gateway, token, brand, last4, exp_month, exp_year, is_default, created_at";

pub async fn methods(pool: &PgPool, customer: Uuid) -> AppResult<Vec<PaymentMethod>> {
    sqlx::query_as(&format!("SELECT {PM} FROM payment_methods WHERE customer_id = $1 AND deleted_at IS NULL ORDER BY is_default DESC, created_at"))
        .bind(customer)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub struct NewMethod<'a> {
    pub gateway: &'a str,
    pub token: &'a str,
    pub brand: &'a str,
    pub last4: &'a str,
    pub exp_month: i16,
    pub exp_year: i16,
}

/// Store a method; the first one becomes the default.
pub async fn add_method(
    pool: &PgPool,
    customer: Uuid,
    m: &NewMethod<'_>,
) -> AppResult<PaymentMethod> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SELECT id FROM customers WHERE id = $1 FOR UPDATE")
        .bind(customer)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM payment_methods WHERE customer_id = $1 AND deleted_at IS NULL",
    )
    .bind(customer)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if n >= 10 {
        return Err(AppError::Conflict("error.too_many_payment_methods"));
    }
    let r: PaymentMethod = sqlx::query_as(&format!(
        "INSERT INTO payment_methods (id, customer_id, gateway, token, brand, last4, exp_month, exp_year, is_default)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {PM}"
    ))
    .bind(Uuid::new_v4())
    .bind(customer)
    .bind(m.gateway)
    .bind(m.token)
    .bind(m.brand)
    .bind(m.last4)
    .bind(m.exp_month)
    .bind(m.exp_year)
    .bind(n == 0)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(r)
}

pub async fn set_default(pool: &PgPool, customer: Uuid, id: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SELECT id FROM customers WHERE id = $1 FOR UPDATE")
        .bind(customer)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let ok: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM payment_methods WHERE id = $1 AND customer_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    ok.ok_or(AppError::NotFound)?;
    sqlx::query(
        "UPDATE payment_methods SET is_default = false WHERE customer_id = $1 AND is_default",
    )
    .bind(customer)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query("UPDATE payment_methods SET is_default = true WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Soft-delete; promotes the oldest remaining method to default.
pub async fn remove(pool: &PgPool, customer: Uuid, id: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SELECT id FROM customers WHERE id = $1 FOR UPDATE")
        .bind(customer)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let n = sqlx::query("UPDATE payment_methods SET deleted_at = now(), is_default = false WHERE id = $1 AND customer_id = $2 AND deleted_at IS NULL")
        .bind(id)
        .bind(customer)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        "UPDATE payment_methods SET is_default = true WHERE id = (SELECT id FROM payment_methods WHERE customer_id = $1 AND deleted_at IS NULL ORDER BY created_at LIMIT 1)
         AND NOT EXISTS (SELECT 1 FROM payment_methods WHERE customer_id = $1 AND deleted_at IS NULL AND is_default)",
    )
    .bind(customer)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

pub async fn auto_pay(pool: &PgPool, customer: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT auto_pay FROM customers WHERE id = $1")
        .bind(customer)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

/// Enabling requires a saved default method.
pub async fn set_auto_pay(pool: &PgPool, customer: Uuid, on: bool) -> AppResult<()> {
    let n = sqlx::query(
        "UPDATE customers SET auto_pay = $2 WHERE id = $1
           AND (NOT $2 OR EXISTS (SELECT 1 FROM payment_methods WHERE customer_id = $1 AND is_default AND deleted_at IS NULL))",
    )
    .bind(customer)
    .bind(on)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("error.no_payment_method"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Attempt {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
    pub payment_method_id: Option<Uuid>,
    pub payment_id: Option<Uuid>,
    pub attempt: i32,
    pub status: String,
    pub error: Option<String>,
    #[schema(value_type = Option<String>)]
    pub next_retry: Option<Date>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

const AT: &str = "id, invoice_id, customer_id, payment_method_id, payment_id, attempt, status, error, next_retry, created_at";

pub async fn attempts(pool: &PgPool, customer: Uuid, limit: i64) -> AppResult<Vec<Attempt>> {
    sqlx::query_as(&format!("SELECT {AT} FROM autopay_attempts WHERE customer_id = $1 ORDER BY created_at DESC, attempt DESC LIMIT $2"))
        .bind(customer)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// A claimed charge the caller must now perform.
#[derive(Debug, Clone)]
pub struct Claim {
    pub attempt_id: Uuid,
    pub attempt: usize,
    pub invoice_id: Uuid,
    pub customer_id: Uuid,
    pub number: String,
    pub balance: Decimal,
    pub currency: String,
    pub due: Date,
    pub method: PaymentMethod,
    pub next_retry: Option<Date>,
}

type DueRow = (Uuid, Uuid, String, Decimal, String, Date, i64);

/// Claim every attempt due on `today` (idempotent).
pub async fn claim_due(pool: &PgPool, cfg: &AutopaySettings, today: Date) -> AppResult<Vec<Claim>> {
    if !cfg.enabled {
        return Ok(vec![]);
    }
    let rows: Vec<DueRow> = sqlx::query_as(
        "SELECT i.id, i.customer_id, COALESCE(i.number, 'PF-' || upper(left(i.id::text, 8))), i.total - i.amount_paid, i.currency::text, i.due_at::date,
                (SELECT count(*) FROM autopay_attempts a WHERE a.invoice_id = i.id)
         FROM invoices i JOIN customers c ON c.id = i.customer_id
         WHERE c.auto_pay AND i.kind IN ('invoice', 'debit_note') AND i.status = 'unpaid' AND i.finalized_at IS NOT NULL
           AND i.due_at IS NOT NULL AND i.due_at::date <= $1 AND i.total > i.amount_paid
           AND NOT EXISTS (SELECT 1 FROM autopay_attempts a WHERE a.invoice_id = i.id AND a.status IN ('processing','pending','succeeded'))
         ORDER BY i.due_at, i.id LIMIT 500",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = vec![];
    for (invoice_id, customer_id, number, balance, currency, due, made) in rows {
        let made = usize::try_from(made).unwrap_or(usize::MAX);
        let Some(attempt) = due_attempt(due, &cfg.retry_days, made, today) else {
            continue;
        };
        let Some(method) = sqlx::query_as::<_, PaymentMethod>(&format!(
            "SELECT {PM} FROM payment_methods WHERE customer_id = $1 AND is_default AND deleted_at IS NULL"
        ))
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        else {
            continue;
        };
        let next = next_retry(due, &cfg.retry_days, attempt);
        let id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO autopay_attempts (id, invoice_id, customer_id, payment_method_id, attempt, status, next_retry)
             VALUES ($1,$2,$3,$4,$5,'processing',$6) ON CONFLICT (invoice_id, attempt) DO NOTHING RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(invoice_id)
        .bind(customer_id)
        .bind(method.id)
        .bind(i32::try_from(attempt).unwrap_or(i32::MAX))
        .bind(next)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        if let Some(attempt_id) = id {
            out.push(Claim {
                attempt_id,
                attempt,
                invoice_id,
                customer_id,
                number,
                balance,
                currency,
                due,
                method,
                next_retry: next,
            });
        }
    }
    Ok(out)
}

pub async fn finish(
    pool: &PgPool,
    attempt_id: Uuid,
    status: &str,
    payment_id: Option<Uuid>,
    error: Option<&str>,
) -> AppResult<()> {
    sqlx::query("UPDATE autopay_attempts SET status = $2, payment_id = COALESCE($3, payment_id), error = $4, updated_at = now() WHERE id = $1")
        .bind(attempt_id)
        .bind(status)
        .bind(payment_id)
        .bind(error)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn fail_payment(pool: &PgPool, payment_id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE payments SET status = 'failed', updated_at = now() WHERE id = $1 AND applied_at IS NULL")
        .bind(payment_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn schedule() {
        let r = [0, 3, 7];
        let due = date!(2026 - 10 - 01);
        assert_eq!(due_attempt(due, &r, 0, date!(2026 - 09 - 30)), None);
        assert_eq!(due_attempt(due, &r, 0, due), Some(0));
        assert_eq!(due_attempt(due, &r, 1, date!(2026 - 10 - 03)), None);
        assert_eq!(due_attempt(due, &r, 1, date!(2026 - 10 - 04)), Some(1));
        assert_eq!(due_attempt(due, &r, 3, date!(2026 - 12 - 01)), None);
        assert_eq!(next_retry(due, &r, 0), Some(date!(2026 - 10 - 04)));
        assert_eq!(next_retry(due, &r, 2), None);
        assert!(AutopaySettings {
            enabled: true,
            retry_days: vec![]
        }
        .normalized()
        .is_err());
        assert_eq!(
            AutopaySettings {
                enabled: true,
                retry_days: vec![5, 0, 5]
            }
            .normalized()
            .unwrap()
            .retry_days,
            vec![0, 5]
        );
    }
}
