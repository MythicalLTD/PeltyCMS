//! Monthly financial reports. Every figure is computed in SQL with NUMERIC
//! (exact decimals); months are UTC calendar months.
//!
//! * Revenue: succeeded payments by `applied_at`; refunds: credit notes by
//!   `finalized_at`.
//! * MRR at month end: sum of `recurring_amount / months(cycle)` for
//!   recurring services live at the end of the month.
//! * Churn: customers with ≥1 live recurring service at the start of the
//!   month and none at its end.
//!
//! A service is live from `activated_at` until `terminated_at`, or until
//! `updated_at` when it is cancelled/terminated/fraud without a
//! termination time.
use crate::db_err;
use pelty_core::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::Date;

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct MoneyPoint {
    pub month: String,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct ChurnPoint {
    pub month: String,
    /// Customers with a live service at the start of the month.
    pub customers_start: i64,
    /// Of those, customers with no live service at the end.
    pub churned: i64,
    /// Customers live at the end who were not at the start.
    pub gained: i64,
    /// churned / customers_start, 4 decimals (0 when start is 0).
    #[schema(value_type = String)]
    pub churn_rate: Decimal,
    pub new_signups: i64,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Report {
    pub from: String,
    pub to: String,
    pub revenue: Vec<MoneyPoint>,
    pub refunds: Vec<MoneyPoint>,
    pub mrr: Vec<MoneyPoint>,
    pub churn: Vec<ChurnPoint>,
}

const SERVICES: &str = "SELECT customer_id, currency, recurring_amount,
        CASE cycle WHEN 'monthly' THEN 1 WHEN 'quarterly' THEN 3 WHEN 'semiannual' THEN 6
                   WHEN 'annual' THEN 12 WHEN 'biennial' THEN 24 WHEN 'triennial' THEN 36 END AS months,
        activated_at AS a,
        COALESCE(terminated_at, CASE WHEN status IN ('cancelled','terminated','fraud') THEN updated_at END) AS e
     FROM services WHERE activated_at IS NOT NULL AND cycle <> 'one_time'";

/// `from`/`to` are first days of months (inclusive).
pub async fn build(pool: &PgPool, from: Date, to: Date) -> AppResult<Report> {
    let months = "SELECT generate_series($1::date, $2::date, interval '1 month')::timestamp AT TIME ZONE 'UTC' AS ms";
    let revenue = sqlx::query_as(
        "SELECT to_char(date_trunc('month', applied_at AT TIME ZONE 'UTC'), 'YYYY-MM') AS month, currency,
                sum(amount) AS amount, count(*) AS count
         FROM payments
         WHERE status = 'succeeded' AND applied_at >= ($1::date)::timestamp AT TIME ZONE 'UTC'
           AND applied_at < (($2::date + interval '1 month')::timestamp) AT TIME ZONE 'UTC'
         GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let refunds = sqlx::query_as(
        "SELECT to_char(date_trunc('month', finalized_at AT TIME ZONE 'UTC'), 'YYYY-MM') AS month, currency,
                sum(abs(total)) AS amount, count(*) AS count
         FROM invoices
         WHERE kind = 'credit_note' AND status <> 'void' AND finalized_at >= ($1::date)::timestamp AT TIME ZONE 'UTC'
           AND finalized_at < (($2::date + interval '1 month')::timestamp) AT TIME ZONE 'UTC'
         GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mrr = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH m AS ({months}), s AS ({SERVICES}), x AS (SELECT ms + interval '1 month' AS me, ms FROM m)
         SELECT to_char(x.ms AT TIME ZONE 'UTC', 'YYYY-MM') AS month, s.currency,
                round(sum(s.recurring_amount / s.months), 4) AS amount, count(*) AS count
         FROM x JOIN s ON s.a < x.me AND (s.e IS NULL OR s.e >= x.me)
         GROUP BY 1, 2 ORDER BY 1, 2"
    )))
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let churn = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH m AS ({months}), s AS ({SERVICES}),
         live AS (
           SELECT m.ms, s.customer_id,
                  bool_or(s.a < m.ms AND (s.e IS NULL OR s.e >= m.ms)) AS at_start,
                  bool_or(s.a < m.ms + interval '1 month' AND (s.e IS NULL OR s.e >= m.ms + interval '1 month')) AS at_end
           FROM m JOIN s ON s.a < m.ms + interval '1 month' AND (s.e IS NULL OR s.e >= m.ms)
           GROUP BY m.ms, s.customer_id),
         agg AS (
           SELECT m.ms,
                  count(l.customer_id) FILTER (WHERE l.at_start) AS customers_start,
                  count(l.customer_id) FILTER (WHERE l.at_start AND NOT l.at_end) AS churned,
                  count(l.customer_id) FILTER (WHERE l.at_end AND NOT l.at_start) AS gained
           FROM m LEFT JOIN live l ON l.ms = m.ms GROUP BY m.ms)
         SELECT to_char(a.ms AT TIME ZONE 'UTC', 'YYYY-MM') AS month, a.customers_start, a.churned, a.gained,
                CASE WHEN a.customers_start = 0 THEN 0::numeric
                     ELSE round(a.churned::numeric / a.customers_start, 4) END AS churn_rate,
                (SELECT count(*) FROM customers c WHERE c.created_at >= a.ms AND c.created_at < a.ms + interval '1 month') AS new_signups
         FROM agg a ORDER BY a.ms"
    )))
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(Report {
        from: format!("{:04}-{:02}", from.year(), u8::from(from.month())),
        to: format!("{:04}-{:02}", to.year(), u8::from(to.month())),
        revenue,
        refunds,
        mrr,
        churn,
    })
}
