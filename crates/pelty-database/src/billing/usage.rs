//! Metered (usage) billing. Records land in the service's open period; when
//! the period ends (the service's billing cycle) it is invoiced once:
//! per metric `max(0, sum - included) × unit_price`, rounded to the currency.
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_billing::math;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Metric {
    pub id: Uuid,
    pub product_id: Uuid,
    pub key: String,
    pub name: String,
    pub unit: String,
    pub unit_price: Decimal,
    pub included_quantity: Decimal,
    pub active: bool,
    /// Dotted path into the server module's usage JSON ('' = API-reported only).
    pub source_key: String,
    /// Module value ÷ divisor = billed quantity (e.g. 1024 for MB→GB).
    pub source_divisor: Decimal,
    /// `sum` (records add up) or `max` (peak sample: disk, monthly bandwidth).
    pub aggregate: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct MetricInput {
    pub product_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub unit: String,
    #[schema(value_type = String)]
    pub unit_price: Decimal,
    #[serde(default)]
    #[schema(value_type = String)]
    pub included_quantity: Decimal,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub source_key: String,
    #[serde(default = "one")]
    #[schema(value_type = String)]
    pub source_divisor: Decimal,
    #[serde(default = "sum")]
    pub aggregate: String,
}

fn one() -> Decimal {
    Decimal::ONE
}
fn sum() -> String {
    "sum".into()
}

fn yes() -> bool {
    true
}

const MCOLS: &str = "id, product_id, key, name, unit, unit_price, included_quantity, active, source_key, source_divisor, aggregate";

fn check_err(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.is_check_violation() || d.is_foreign_key_violation() => {
            AppError::Validation("error.validation")
        }
        _ => db_err(e),
    }
}

pub async fn metrics(pool: &PgPool, product: Option<Uuid>) -> AppResult<Vec<Metric>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {MCOLS} FROM usage_metrics WHERE ($1::uuid IS NULL OR product_id = $1) ORDER BY product_id, key")))
        .bind(product)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn create_metric(pool: &PgPool, m: &MetricInput) -> AppResult<Metric> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO usage_metrics (product_id, key, name, unit, unit_price, included_quantity, active, source_key, source_divisor, aggregate) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING {MCOLS}"
    )))
    .bind(m.product_id)
    .bind(m.key.trim())
    .bind(m.name.trim())
    .bind(m.unit.trim())
    .bind(m.unit_price)
    .bind(m.included_quantity)
    .bind(m.active)
    .bind(m.source_key.trim())
    .bind(m.source_divisor)
    .bind(&m.aggregate)
    .fetch_one(pool)
    .await
    .map_err(check_err)
}

/// Key and product are immutable (records reference the metric).
pub async fn update_metric(pool: &PgPool, id: Uuid, m: &MetricInput) -> AppResult<Metric> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE usage_metrics SET name = $2, unit = $3, unit_price = $4, included_quantity = $5, active = $6, source_key = $7, source_divisor = $8, aggregate = $9 WHERE id = $1 RETURNING {MCOLS}"
    )))
    .bind(id)
    .bind(m.name.trim())
    .bind(m.unit.trim())
    .bind(m.unit_price)
    .bind(m.included_quantity)
    .bind(m.active)
    .bind(m.source_key.trim())
    .bind(m.source_divisor)
    .bind(&m.aggregate)
    .fetch_optional(pool)
    .await
    .map_err(check_err)?
    .ok_or(AppError::NotFound)
}

pub async fn delete_metric(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let used: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM usage_records WHERE metric_id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
    if used {
        return Err(AppError::Conflict("error.metric_in_use"));
    }
    let r = sqlx::query("DELETE FROM usage_metrics WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// End of a period starting at `start`: the service's next due date when it
/// is later, else one month on.
pub fn period_end(start: Date, next_due: Option<Date>) -> Date {
    match next_due {
        Some(d) if d > start => d,
        _ => {
            let (y, m) = if start.month() == time::Month::December {
                (start.year() + 1, time::Month::January)
            } else {
                (start.year(), start.month().next())
            };
            let mut day = start.day();
            loop {
                if let Ok(d) = Date::from_calendar_date(y, m, day) {
                    return d;
                }
                day -= 1;
            }
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Period {
    pub id: Uuid,
    pub service_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub status: String,
    pub invoice_id: Option<Uuid>,
    pub total: Option<Decimal>,
}

const PCOLS: &str = "id, service_id, period_start, period_end, status, invoice_id, total";

pub struct Recorded {
    pub id: Uuid,
    pub duplicate: bool,
    pub period: Period,
}

/// Record usage for a service (idempotent per `idempotency_key`). Usage
/// timestamped after the open period's end opens the next period.
pub async fn record(
    pool: &PgPool,
    service_id: Uuid,
    metric_key: &str,
    quantity: Decimal,
    at: OffsetDateTime,
    key: &str,
) -> AppResult<Recorded> {
    if quantity <= Decimal::ZERO || quantity.scale() > 6 {
        return Err(AppError::Validation("error.invalid_quantity"));
    }
    let key = key.trim();
    if key.is_empty() || key.len() > 100 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    // Serialize per service so concurrent records agree on the period.
    let svc: Option<(Uuid, Option<Date>, String)> = sqlx::query_as(
        "SELECT product_id, next_due_date, status FROM services WHERE id = $1 FOR UPDATE",
    )
    .bind(service_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (product, next_due, status) = svc.ok_or(AppError::NotFound)?;
    if status == "terminated" || status == "cancelled" {
        return Err(AppError::Validation("error.service_not_active"));
    }
    if let Some((id, pid)) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT id, period_id FROM usage_records WHERE service_id = $1 AND idempotency_key = $2",
    )
    .bind(service_id)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?
    {
        let period = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {PCOLS} FROM usage_periods WHERE id = $1"
        )))
        .bind(pid)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        return Ok(Recorded {
            id,
            duplicate: true,
            period,
        });
    }
    let metric: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM usage_metrics WHERE product_id = $1 AND key = $2 AND active",
    )
    .bind(product)
    .bind(metric_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let metric = metric.ok_or(AppError::Validation("error.unknown_metric"))?;
    let day = at.date();
    let mut open: Option<Period> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {PCOLS} FROM usage_periods WHERE service_id = $1 AND status = 'open'"
    )))
    .bind(service_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    if let Some(p) = open.clone() {
        if day < p.period_start {
            return Err(AppError::Validation("error.usage_period_closed"));
        }
        if day >= p.period_end {
            // Period over but not billed yet: close it for new records.
            sqlx::query("UPDATE usage_periods SET status = 'closed' WHERE id = $1")
                .bind(p.id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            open = None;
        }
    }
    if open.is_none() {
        let last_end: Option<Date> =
            sqlx::query_scalar("SELECT max(period_end) FROM usage_periods WHERE service_id = $1")
                .bind(service_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        if last_end.is_some_and(|e| day < e) {
            return Err(AppError::Validation("error.usage_period_closed"));
        }
        let start = last_end
            .filter(|e| day < period_end(*e, next_due))
            .unwrap_or(day);
        let end = period_end(start, next_due);
        open = Some(
            sqlx::query_as(sqlx::AssertSqlSafe(format!("INSERT INTO usage_periods (service_id, period_start, period_end) VALUES ($1,$2,$3) RETURNING {PCOLS}")))
                .bind(service_id)
                .bind(start)
                .bind(end)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?,
        );
    }
    let period = open.ok_or(AppError::NotFound)?;
    let id: Uuid = sqlx::query_scalar("INSERT INTO usage_records (service_id, metric_id, period_id, quantity, recorded_at, idempotency_key) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(service_id)
        .bind(metric)
        .bind(period.id)
        .bind(quantity)
        .bind(at)
        .bind(key)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Recorded {
        id,
        duplicate: false,
        period,
    })
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct MetricUsage {
    pub metric_id: Uuid,
    pub key: String,
    pub name: String,
    pub unit: String,
    pub unit_price: Decimal,
    pub included_quantity: Decimal,
    pub quantity: Decimal,
}

impl MetricUsage {
    pub fn billable(&self) -> Decimal {
        (self.quantity - self.included_quantity).max(Decimal::ZERO)
    }
    /// Unrounded charge.
    pub fn charge(&self) -> Decimal {
        self.billable() * self.unit_price
    }
}

pub async fn period_usage(pool: &PgPool, period_id: Uuid) -> AppResult<Vec<MetricUsage>> {
    sqlx::query_as(
        "SELECT m.id AS metric_id, m.key, m.name, m.unit, m.unit_price, m.included_quantity, CASE WHEN m.aggregate = 'max' THEN max(r.quantity) ELSE sum(r.quantity) END AS quantity
         FROM usage_records r JOIN usage_metrics m ON m.id = r.metric_id WHERE r.period_id = $1
         GROUP BY m.id ORDER BY m.key",
    )
    .bind(period_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn periods(pool: &PgPool, service_id: Uuid) -> AppResult<Vec<Period>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {PCOLS} FROM usage_periods WHERE service_id = $1 ORDER BY period_start DESC LIMIT 24")))
        .bind(service_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Invoice every open period that ended on or before `today`. Each period
/// is locked and billed once; zero-charge periods close without an invoice.
/// Returns the number of invoices created.
pub async fn bill_due(pool: &PgPool, today: Date) -> AppResult<u32> {
    let due: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM usage_periods WHERE status IN ('open','closed') AND period_end <= $1 ORDER BY period_end LIMIT 500")
        .bind(today)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let mut n = 0;
    for pid in due {
        if bill_period(pool, pid).await?.is_some() {
            n += 1;
        }
    }
    Ok(n)
}

pub async fn bill_period(pool: &PgPool, pid: Uuid) -> AppResult<Option<Uuid>> {
    let usage = period_usage(pool, pid).await?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let p: Option<(Uuid, Date, Date, Uuid, String, String)> = sqlx::query_as(
        "SELECT p.service_id, p.period_start, p.period_end, s.customer_id, s.currency::text, pr.name
         FROM usage_periods p JOIN services s ON s.id = p.service_id JOIN products pr ON pr.id = s.product_id
         WHERE p.id = $1 AND p.status IN ('open','closed') FOR UPDATE OF p SKIP LOCKED",
    )
    .bind(pid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some((_sid, start, end, customer, currency, product)) = p else {
        return Ok(None);
    };
    let exp = invoices::exponent(&mut tx, &currency).await?;
    let last = end - Duration::days(1);
    let items: Vec<NewItem> = usage
        .iter()
        .filter_map(|u| {
            let amount = math::round(u.charge(), exp);
            (amount > Decimal::ZERO).then(|| NewItem {
                description: format!(
                    "{product}, {}: {} {} × {} ({start}, {last})",
                    u.name,
                    u.billable().normalize(),
                    u.unit,
                    u.unit_price.normalize()
                ),
                quantity: Decimal::ONE,
                unit_price: amount,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            })
        })
        .collect();
    let (invoice, total) = if items.is_empty() {
        (None, Decimal::ZERO)
    } else {
        let inv = invoices::create_draft(&mut tx, customer, &currency, None).await?;
        invoices::add_items(&mut tx, inv, &currency, &items).await?;
        let i = invoices::finalize(&mut tx, inv, 7).await?;
        (Some(inv), i.subtotal)
    };
    sqlx::query("UPDATE usage_periods SET status = 'billed', invoice_id = $2, total = $3, billed_at = now() WHERE id = $1")
        .bind(pid)
        .bind(invoice)
        .bind(total)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(invoice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;
    #[test]
    fn periods_follow_cycle() {
        assert_eq!(
            period_end(date!(2026 - 10 - 05), Some(date!(2026 - 11 - 01))),
            date!(2026 - 11 - 01)
        );
        assert_eq!(
            period_end(date!(2026 - 01 - 31), None),
            date!(2026 - 02 - 28)
        );
        assert_eq!(
            period_end(date!(2026 - 12 - 15), Some(date!(2026 - 12 - 01))),
            date!(2027 - 01 - 15)
        );
    }
    #[test]
    fn charge_is_exact() {
        let u = MetricUsage {
            metric_id: Uuid::nil(),
            key: "gb".into(),
            name: "GB".into(),
            unit: "GB".into(),
            unit_price: "0.00012345".parse().unwrap(),
            included_quantity: "10".parse().unwrap(),
            quantity: "1010.5".parse().unwrap(),
        };
        assert_eq!(u.billable(), "1000.5".parse::<Decimal>().unwrap());
        assert_eq!(u.charge(), "0.123511725".parse::<Decimal>().unwrap());
        assert_eq!(
            math::round(u.charge(), 2),
            "0.12".parse::<Decimal>().unwrap()
        );
        let none = MetricUsage {
            quantity: "5".parse().unwrap(),
            ..u
        };
        assert_eq!(none.charge(), Decimal::ZERO);
    }
}

/// Number at dotted `path` in a module usage object (numbers or numeric
/// strings), divided by `divisor` and rounded half-up to 6 dp.
pub fn sample_value(usage: &serde_json::Value, path: &str, divisor: Decimal) -> Option<Decimal> {
    use std::str::FromStr;
    let mut v = usage;
    for part in path.split('.') {
        v = v.get(part)?;
    }
    let d = match v {
        serde_json::Value::Number(n) => Decimal::from_str(&n.to_string())
            .or_else(|_| Decimal::from_scientific(&n.to_string()))
            .ok()?,
        serde_json::Value::String(s) => Decimal::from_str(s.trim()).ok()?,
        _ => return None,
    };
    if divisor <= Decimal::ZERO || d.is_sign_negative() {
        return None;
    }
    Some(
        (d / divisor)
            .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero),
    )
}

/// Module-sourced metrics of a service's product: (key, source_key, divisor).
pub async fn module_metrics(
    pool: &PgPool,
    service_id: Uuid,
) -> AppResult<Vec<(String, String, Decimal)>> {
    sqlx::query_as(
        "SELECT m.key, m.source_key, m.source_divisor FROM usage_metrics m JOIN services s ON s.product_id = m.product_id
         WHERE s.id = $1 AND m.active AND m.source_key <> '' ORDER BY m.key",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Active services whose product has module-sourced metrics.
pub async fn services_with_module_metrics(pool: &PgPool, limit: i64) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT DISTINCT s.id FROM services s JOIN usage_metrics m ON m.product_id = s.product_id
         WHERE s.status IN ('active','suspended') AND m.active AND m.source_key <> '' ORDER BY s.id LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Record one sample per module metric from a usage object. Idempotent per
/// hour (`module:<key>:<YYYY-MM-DDTHH>`); zero/missing values are skipped.
pub async fn record_samples(
    pool: &PgPool,
    service_id: Uuid,
    usage: &serde_json::Value,
    at: OffsetDateTime,
) -> AppResult<Vec<(String, Decimal)>> {
    let hour = format!("{}T{:02}", at.date(), at.hour());
    let mut out = Vec::new();
    for (key, path, div) in module_metrics(pool, service_id).await? {
        let Some(q) = sample_value(usage, &path, div).filter(|q| *q > Decimal::ZERO) else {
            continue;
        };
        let r = record(
            pool,
            service_id,
            &key,
            q,
            at,
            &format!("module:{key}:{hour}"),
        )
        .await?;
        if !r.duplicate {
            out.push((key, q));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use serde_json::json;
    use std::str::FromStr;

    #[test]
    fn sample_paths() {
        let u = json!({"disk_mb": 3072, "bw": {"gb": "12.5"}, "neg": -1, "f": 0.1, "txt": "x"});
        let d = |s: &str| Decimal::from_str(s).unwrap();
        assert_eq!(sample_value(&u, "disk_mb", d("1024")), Some(d("3")));
        assert_eq!(sample_value(&u, "bw.gb", Decimal::ONE), Some(d("12.5")));
        assert_eq!(sample_value(&u, "f", Decimal::ONE), Some(d("0.1")));
        assert_eq!(sample_value(&u, "disk_mb", d("7")), Some(d("438.857143")));
        assert_eq!(sample_value(&u, "neg", Decimal::ONE), None);
        assert_eq!(sample_value(&u, "txt", Decimal::ONE), None);
        assert_eq!(sample_value(&u, "missing.x", Decimal::ONE), None);
        assert_eq!(sample_value(&u, "disk_mb", Decimal::ZERO), None);
    }
}
