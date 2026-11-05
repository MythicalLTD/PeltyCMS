//! Dashboard aggregates: each figure is one indexed aggregate query, never a
//! per-row loop.
use crate::db_err;
use pelty_core::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct CurrencyTotal {
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct UpcomingService {
    pub id: Uuid,
    pub product_name: String,
    #[schema(value_type = String)]
    pub next_due_date: Date,
    pub currency: String,
    #[schema(value_type = String)]
    pub recurring_amount: Decimal,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ClientDashboard {
    pub active_services: i64,
    pub unpaid_invoices: i64,
    pub overdue_invoices: i64,
    /// Balance due per currency (total - paid of unpaid invoices).
    pub due: Vec<CurrencyTotal>,
    pub upcoming: Vec<UpcomingService>,
    /// This customer's succeeded payments, last 6 months, oldest first.
    pub spend_series: Vec<MonthRevenue>,
}

pub async fn client(pool: &PgPool, customer_id: Uuid) -> AppResult<ClientDashboard> {
    let (active, unpaid, overdue): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM services WHERE customer_id = $1 AND status = 'active'),
                (SELECT count(*) FROM invoices WHERE customer_id = $1 AND status = 'unpaid'),
                (SELECT count(*) FROM invoices WHERE customer_id = $1 AND status = 'unpaid' AND due_at < now())",
    )
    .bind(customer_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let due = sqlx::query_as(
        "SELECT currency, sum(total - amount_paid) AS amount, count(*) AS count FROM invoices
         WHERE customer_id = $1 AND status = 'unpaid' GROUP BY currency ORDER BY currency",
    )
    .bind(customer_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let upcoming = sqlx::query_as(
        "SELECT s.id, p.name AS product_name, s.next_due_date, s.currency, s.recurring_amount
         FROM services s JOIN products p ON p.id = s.product_id
         WHERE s.customer_id = $1 AND s.status IN ('active','suspended') AND s.next_due_date IS NOT NULL
         ORDER BY s.next_due_date LIMIT 5",
    )
    .bind(customer_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let month_start = OffsetDateTime::now_utc()
        .replace_day(1)
        .expect("day 1")
        .replace_time(time::Time::MIDNIGHT);
    let spend_series = sqlx::query_as(
        "SELECT to_char(date_trunc('month', p.applied_at AT TIME ZONE 'UTC'), 'YYYY-MM') AS month, p.currency, sum(p.amount) AS amount
         FROM payments p JOIN invoices i ON i.id = p.invoice_id
         WHERE i.customer_id = $1 AND p.status = 'succeeded' AND p.applied_at >= $2 - interval '5 months'
         GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .bind(customer_id)
    .bind(month_start)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(ClientDashboard {
        active_services: active,
        unpaid_invoices: unpaid,
        overdue_invoices: overdue,
        due,
        upcoming,
        spend_series,
    })
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AdminDashboard {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub month_start: OffsetDateTime,
    /// Applied payments since the start of the current month (UTC), per currency.
    pub revenue_month: Vec<CurrencyTotal>,
    pub unpaid: Vec<CurrencyTotal>,
    pub overdue: Vec<CurrencyTotal>,
    pub active_services: i64,
    pub pending_orders: i64,
    pub failed_jobs: i64,
    /// Succeeded payments for the last 6 calendar months (UTC), oldest first.
    pub revenue_series: Vec<MonthRevenue>,
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct MonthRevenue {
    /// `YYYY-MM`
    pub month: String,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
}

pub async fn admin(pool: &PgPool) -> AppResult<AdminDashboard> {
    let now = OffsetDateTime::now_utc();
    let month_start = now
        .replace_day(1)
        .expect("day 1")
        .replace_time(time::Time::MIDNIGHT);
    let revenue_month = sqlx::query_as(
        "SELECT currency, sum(amount) AS amount, count(*) AS count FROM payments
         WHERE applied_at >= $1 AND status = 'succeeded' GROUP BY currency ORDER BY currency",
    )
    .bind(month_start)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let rows: Vec<(bool, String, Decimal, i64)> = sqlx::query_as(
        "SELECT COALESCE(due_at < now(), false) AS overdue, currency, sum(total - amount_paid), count(*) FROM invoices
         WHERE status = 'unpaid' GROUP BY 1, 2 ORDER BY 2",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut unpaid: Vec<CurrencyTotal> = Vec::new();
    let mut overdue = Vec::new();
    for (od, currency, amount, count) in rows {
        match unpaid.iter_mut().find(|c| c.currency == currency) {
            Some(c) => {
                c.amount += amount;
                c.count += count;
            }
            None => unpaid.push(CurrencyTotal {
                currency: currency.clone(),
                amount,
                count,
            }),
        }
        if od {
            overdue.push(CurrencyTotal {
                currency,
                amount,
                count,
            });
        }
    }
    let (active_services, pending_orders, failed_jobs): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM services WHERE status = 'active'),
                (SELECT count(*) FROM orders WHERE status = 'pending'),
                (SELECT count(*) FROM jobs WHERE status IN ('failed','dead'))",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let revenue_series = sqlx::query_as(
        "SELECT to_char(date_trunc('month', applied_at AT TIME ZONE 'UTC'), 'YYYY-MM') AS month, currency, sum(amount) AS amount
         FROM payments WHERE status = 'succeeded' AND applied_at >= $1 - interval '5 months'
         GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .bind(month_start)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(AdminDashboard {
        month_start,
        revenue_month,
        unpaid,
        overdue,
        active_services,
        pending_orders,
        failed_jobs,
        revenue_series,
    })
}
