//! WHMCS-style report library. Every report is a fixed SQL query over a
//! `[from, to)` time window returning text cells (money stays exact: NUMERIC
//! is rendered by Postgres, never through floats).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;

pub struct Def {
    pub key: &'static str,
    pub category: &'static str,
    pub columns: &'static [&'static str],
    /// (label column, value column) for a bar chart, when meaningful.
    pub chart: Option<(usize, usize)>,
    /// Optional feature module the report belongs to: hidden (and 404) while
    /// that module is off.
    pub module: Option<&'static str>,
    sql: &'static str,
}

pub const REPORTS: &[Def] = &[
    Def { key: "annual_income", category: "income", columns: &["year", "currency", "income", "payments"], chart: Some((0, 2)),
        module: None, sql: "SELECT to_char(applied_at,'YYYY'), currency, sum(amount)::text, count(*)::text FROM payments
              WHERE status='succeeded' AND applied_at >= $1 AND applied_at < $2 GROUP BY 1,2 ORDER BY 1,2" },
    Def { key: "monthly_transactions", category: "income", columns: &["month", "currency", "amount_in", "transactions"], chart: Some((0, 2)),
        module: None, sql: "SELECT to_char(applied_at,'YYYY-MM'), currency, sum(amount)::text, count(*)::text FROM payments
              WHERE status='succeeded' AND applied_at >= $1 AND applied_at < $2 GROUP BY 1,2 ORDER BY 1,2" },
    Def { key: "income_by_product", category: "income", columns: &["product", "currency", "amount", "items"], chart: Some((0, 2)),
        module: None, sql: "SELECT COALESCE(p.name, '-'), i.currency, sum(ii.amount)::text, count(*)::text FROM invoice_items ii
              JOIN invoices i ON i.id=ii.invoice_id LEFT JOIN services s ON s.id=ii.service_id LEFT JOIN products p ON p.id=s.product_id
              WHERE i.status='paid' AND i.kind='invoice' AND i.paid_at >= $1 AND i.paid_at < $2 GROUP BY 1,2 ORDER BY 3 DESC" },
    Def { key: "sales_tax_liability", category: "income", columns: &["month", "currency", "taxable", "tax", "invoices"], chart: Some((0, 3)),
        module: None, sql: "SELECT to_char(paid_at,'YYYY-MM'), currency, sum(subtotal)::text, sum(tax_total)::text, count(*)::text FROM invoices
              WHERE status='paid' AND kind='invoice' AND paid_at >= $1 AND paid_at < $2 GROUP BY 1,2 ORDER BY 1,2" },
    Def { key: "tax_by_rate", category: "income", columns: &["country", "rate", "currency", "taxable", "tax", "invoices"], chart: Some((0, 4)),
        module: None, sql: "SELECT COALESCE(NULLIF(i.customer_snapshot->>'country',''), '-'), ii.tax_rate::text, i.currency, sum(ii.amount)::text, sum(ii.tax_amount)::text, count(DISTINCT i.id)::text
              FROM invoice_items ii JOIN invoices i ON i.id=ii.invoice_id
              WHERE i.status='paid' AND i.kind='invoice' AND i.paid_at >= $1 AND i.paid_at < $2 GROUP BY 1,2,3 ORDER BY 1,2,3" },
    Def { key: "eu_oss", category: "income", columns: &["country", "rate", "currency", "taxable", "tax", "invoices"], chart: Some((0, 4)),
        module: None, sql: "SELECT i.customer_snapshot->>'country', ii.tax_rate::text, i.currency, sum(ii.amount)::text, sum(ii.tax_amount)::text, count(DISTINCT i.id)::text
              FROM invoice_items ii JOIN invoices i ON i.id=ii.invoice_id
              WHERE i.status='paid' AND i.kind='invoice' AND NOT i.reverse_charge AND i.paid_at >= $1 AND i.paid_at < $2
                AND i.customer_snapshot->>'country' IN ('AT','BE','BG','HR','CY','CZ','DK','EE','FI','FR','DE','GR','HU','IE','IT','LV','LT','LU','MT','NL','PL','PT','RO','SK','SI','ES','SE')
                AND i.customer_snapshot->>'country' IS DISTINCT FROM (SELECT value->>'home_country' FROM system_settings WHERE key='billing.tax')
                AND COALESCE(i.customer_snapshot->>'vat_number','') = ''
              GROUP BY 1,2,3 ORDER BY 1,2,3" },
    Def { key: "credit_usage", category: "income", columns: &["month", "currency", "added", "used", "entries"], chart: None,
        module: None, sql: "SELECT to_char(created_at,'YYYY-MM'), currency, sum(amount) FILTER (WHERE amount>0)::text, (-sum(amount) FILTER (WHERE amount<0))::text, count(*)::text
              FROM credit_ledger WHERE created_at >= $1 AND created_at < $2 GROUP BY 1,2 ORDER BY 1,2" },
    Def { key: "aging_invoices", category: "income", columns: &["bucket", "currency", "outstanding", "invoices"], chart: Some((0, 2)),
        module: None, sql: "SELECT CASE WHEN now()-due_at <= interval '0' THEN '0 not due' WHEN now()-due_at <= interval '30 days' THEN '1-30'
                   WHEN now()-due_at <= interval '60 days' THEN '31-60' WHEN now()-due_at <= interval '90 days' THEN '61-90' ELSE '90+' END,
                   currency, sum(total-amount_paid)::text, count(*)::text FROM invoices
              WHERE status='unpaid' AND kind='invoice' AND due_at IS NOT NULL AND $1::timestamptz <= now() AND $2::timestamptz IS NOT NULL GROUP BY 1,2 ORDER BY 1,2" },
    Def { key: "new_customers", category: "clients", columns: &["month", "new_customers", "with_orders"], chart: Some((0, 1)),
        module: None, sql: "SELECT to_char(c.created_at,'YYYY-MM'), count(*)::text, count(*) FILTER (WHERE EXISTS (SELECT 1 FROM orders o WHERE o.customer_id=c.id))::text
              FROM customers c WHERE c.created_at >= $1 AND c.created_at < $2 GROUP BY 1 ORDER BY 1" },
    Def { key: "clients_by_country", category: "clients", columns: &["country", "customers", "active"], chart: Some((0, 1)),
        module: None, sql: "SELECT COALESCE(NULLIF(country,''),'-'), count(*)::text, count(*) FILTER (WHERE status='active')::text FROM customers
              WHERE created_at < $2 AND $1::timestamptz IS NOT NULL GROUP BY 1 ORDER BY count(*) DESC" },
    Def { key: "customer_statistics", category: "clients", columns: &["status", "customers", "with_active_services"], chart: Some((0, 1)),
        module: None, sql: "SELECT c.status, count(*)::text, count(*) FILTER (WHERE EXISTS (SELECT 1 FROM services s WHERE s.customer_id=c.id AND s.status='active'))::text
              FROM customers c WHERE c.created_at < $2 AND $1::timestamptz IS NOT NULL GROUP BY 1 ORDER BY 1" },
    Def { key: "top_customers", category: "clients", columns: &["customer", "currency", "paid", "invoices"], chart: Some((0, 2)),
        module: None, sql: "SELECT c.email::text, i.currency, sum(i.amount_paid)::text, count(*)::text FROM invoices i JOIN customers c ON c.id=i.customer_id
              WHERE i.status='paid' AND i.kind='invoice' AND i.paid_at >= $1 AND i.paid_at < $2 GROUP BY 1,2 ORDER BY sum(i.amount_paid) DESC LIMIT 50" },
    Def { key: "ticket_stats", category: "support", columns: &["month", "opened", "closed", "urgent_or_high"], chart: Some((0, 1)),
        module: None, sql: "SELECT to_char(created_at,'YYYY-MM'), count(*)::text, count(*) FILTER (WHERE status='closed')::text, count(*) FILTER (WHERE priority IN ('urgent','high'))::text
              FROM tickets WHERE created_at >= $1 AND created_at < $2 GROUP BY 1 ORDER BY 1" },
    Def { key: "ticket_departments", category: "support", columns: &["department", "tickets", "open"], chart: Some((0, 1)),
        module: None, sql: "SELECT d.name, count(t.id)::text, count(t.id) FILTER (WHERE t.status<>'closed')::text FROM ticket_departments d
              LEFT JOIN tickets t ON t.department_id=d.id AND t.created_at >= $1 AND t.created_at < $2 GROUP BY 1 ORDER BY count(t.id) DESC" },
    Def { key: "product_suspensions", category: "services", columns: &["product", "suspended", "reason"], chart: Some((0, 1)),
        module: None, sql: "SELECT p.name, count(*)::text, string_agg(DISTINCT NULLIF(s.suspend_reason,''), '; ') FROM services s JOIN products p ON p.id=s.product_id
              WHERE s.status='suspended' AND s.updated_at >= $1 AND s.updated_at < $2 GROUP BY 1 ORDER BY count(*) DESC" },
    Def { key: "services_by_product", category: "services", columns: &["product", "active", "total"], chart: Some((0, 1)),
        module: None, sql: "SELECT p.name, count(*) FILTER (WHERE s.status='active')::text, count(*)::text FROM services s JOIN products p ON p.id=s.product_id
              WHERE s.created_at < $2 AND $1::timestamptz IS NOT NULL GROUP BY 1 ORDER BY count(*) DESC" },
    Def { key: "server_stats", category: "services", columns: &["server", "module", "active_services", "capacity"], chart: Some((0, 2)),
        module: None, sql: "SELECT sv.name, sv.module, count(s.id) FILTER (WHERE s.status='active')::text, COALESCE(sv.max_services::text,'∞') FROM servers sv
              LEFT JOIN services s ON s.server_id=sv.id AND s.created_at < $2 AND $1::timestamptz IS NOT NULL GROUP BY sv.id ORDER BY 1" },
    Def { key: "domain_renewals", category: "services", columns: &["month", "expiring", "auto_renew"], chart: Some((0, 1)),
        module: None, sql: "SELECT to_char(expires_on,'YYYY-MM'), count(*)::text, count(*) FILTER (WHERE auto_renew)::text FROM domains
              WHERE expires_on >= $1::date AND expires_on < $2::date GROUP BY 1 ORDER BY 1" },
    Def { key: "affiliates", category: "marketing", columns: &["affiliate", "currency", "commissions", "count"], chart: Some((0, 2)),
        module: None, sql: "SELECT a.code, ac.currency, sum(ac.amount)::text, count(*)::text FROM affiliate_commissions ac JOIN affiliates a ON a.id=ac.affiliate_id
              WHERE ac.created_at >= $1 AND ac.created_at < $2 GROUP BY 1,2 ORDER BY sum(ac.amount) DESC" },
    Def { key: "promotions", category: "marketing", columns: &["code", "uses", "discount_total"], chart: Some((0, 1)),
        module: None, sql: "SELECT c.code, count(r.id)::text, COALESCE(sum(r.amount),0)::text FROM coupons c JOIN coupon_redemptions r ON r.coupon_id=c.id
              WHERE r.created_at >= $1 AND r.created_at < $2 GROUP BY 1 ORDER BY count(r.id) DESC" },
    // Paid invoice lines (net of tax, discount lines included), traced to
    // their origin invoice through `copied_from` so merged/split replacement
    // invoices still count for the order they came from. Lines from an
    // order's invoice are shared out over its items by gross weight (first
    // invoices exist before provisioning, so they carry no service);
    // renewals are counted by their service line.
    Def { key: "game_server_sales", category: "gaming", columns: &["server", "currency", "amount", "items"], chart: Some((0, 2)), module: Some("game_profiles"),
        sql: "WITH paid AS (
                SELECT ii.id AS line_id, ii.amount, ii.service_id, i.currency, o.order_id AS origin_order
                FROM invoice_items ii JOIN invoices i ON i.id = ii.invoice_id
                JOIN invoice_items src ON src.id = COALESCE(ii.copied_from, ii.id)
                JOIN invoices o ON o.id = src.invoice_id
                WHERE i.status = 'paid' AND i.kind = 'invoice' AND i.paid_at >= $1 AND i.paid_at < $2),
              order_net AS (
                SELECT origin_order, currency, sum(amount) AS net FROM paid WHERE origin_order IS NOT NULL GROUP BY 1, 2),
              weights AS (
                SELECT oi.id, oi.order_id, oi.product_id,
                       oi.amount + oi.setup_fee + COALESCE((SELECT sum(a.amount + a.setup_fee) FROM order_item_addons a WHERE a.order_item_id = oi.id), 0) AS w
                FROM order_items oi WHERE oi.order_id IN (SELECT origin_order FROM order_net)),
              totals AS (SELECT order_id, sum(w) AS tw FROM weights GROUP BY 1),
              x AS (
                SELECT gp.server_id, n.currency, n.net * w.w / t.tw AS amt, 'o' || w.id::text AS k
                FROM order_net n JOIN weights w ON w.order_id = n.origin_order JOIN totals t ON t.order_id = n.origin_order
                JOIN game_server_products gp ON gp.product_id = w.product_id
                WHERE t.tw > 0
                UNION ALL
                SELECT gp.server_id, p.currency, p.amount, 'l' || p.line_id::text
                FROM paid p JOIN services s ON s.id = p.service_id JOIN game_server_products gp ON gp.product_id = s.product_id
                WHERE p.origin_order IS NULL)
              SELECT gs.name, x.currency, round(sum(x.amt), 2)::text, count(DISTINCT x.k)::text FROM x JOIN game_servers gs ON gs.id = x.server_id
              GROUP BY gs.id, x.currency ORDER BY sum(x.amt) DESC" },
    Def { key: "game_deliveries", category: "gaming", columns: &["server", "delivered", "failed", "pending", "waiting_identity"], chart: Some((0, 1)), module: Some("game_delivery"),
        sql: "SELECT gs.name, count(*) FILTER (WHERE d.status='delivered')::text, count(*) FILTER (WHERE d.status='failed')::text,
                     count(*) FILTER (WHERE d.status='pending')::text, count(*) FILTER (WHERE d.status='waiting_identity')::text
              FROM game_deliveries d JOIN game_servers gs ON gs.id=d.server_id
              WHERE d.created_at >= $1 AND d.created_at < $2 GROUP BY gs.id ORDER BY count(*) DESC" },
    Def { key: "creator_codes", category: "gaming", columns: &["code", "currency", "orders", "paid"], chart: Some((0, 3)), module: Some("creator_codes"),
        sql: "SELECT cc.code, o.currency, count(*)::text,
                     COALESCE(sum((SELECT sum(i.amount_paid) FROM invoices i WHERE i.order_id=o.id AND i.status='paid' AND i.kind='invoice')), 0)::text
              FROM creator_code_orders co JOIN creator_codes cc ON cc.id=co.code_id JOIN orders o ON o.id=co.order_id
              WHERE co.created_at >= $1 AND co.created_at < $2 GROUP BY cc.code, o.currency ORDER BY count(*) DESC" },
    Def { key: "marketplace_sales", category: "gaming", columns: &["creator", "currency", "gross", "commission", "net", "refunds"], chart: Some((0, 2)), module: Some("creator_marketplace"),
        sql: "SELECT c.display_name, l.currency, COALESCE(sum(l.gross) FILTER (WHERE l.kind='sale'),0)::text,
                     COALESCE(sum(l.commission) FILTER (WHERE l.kind='sale'),0)::text, COALESCE(sum(l.amount) FILTER (WHERE l.kind='sale'),0)::text,
                     COALESCE(sum(abs(l.amount)) FILTER (WHERE l.kind='refund'),0)::text
              FROM creator_ledger l JOIN creators c ON c.id=l.creator_id
              WHERE l.kind IN ('sale','refund') AND l.created_at >= $1 AND l.created_at < $2
              GROUP BY c.id, l.currency ORDER BY sum(l.gross) FILTER (WHERE l.kind='sale') DESC NULLS LAST" },
];

pub fn find(key: &str) -> Option<&'static Def> {
    REPORTS.iter().find(|d| d.key == key)
}

#[derive(Debug, Serialize)]
pub struct Table {
    pub key: &'static str,
    pub columns: &'static [&'static str],
    pub chart: Option<(usize, usize)>,
    pub rows: Vec<Vec<Option<String>>>,
}

pub async fn run(
    pool: &PgPool,
    key: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> AppResult<Table> {
    let d = find(key).ok_or(AppError::NotFound)?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(d.sql))
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let rows = rows
        .iter()
        .map(|r| {
            (0..r.len())
                .map(|i| r.try_get::<Option<String>, _>(i).ok().flatten())
                .collect()
        })
        .collect();
    Ok(Table {
        key: d.key,
        columns: d.columns,
        chart: d.chart,
        rows,
    })
}

/// RFC 4180 CSV with formula-injection guard (cells starting with = + - @ are quoted with a leading ').
pub fn to_csv(t: &Table) -> String {
    fn cell(s: &str) -> String {
        let s = if s.starts_with(['=', '+', '@'])
            || (s.starts_with('-') && s.parse::<f64>().is_err())
        {
            format!("'{s}")
        } else {
            s.to_string()
        };
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s
        }
    }
    let mut out = t
        .columns
        .iter()
        .map(|c| cell(c))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str("\r\n");
    for r in &t.rows {
        out.push_str(
            &r.iter()
                .map(|c| cell(c.as_deref().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_escapes() {
        let t = Table {
            key: "x",
            columns: &["a", "b"],
            chart: None,
            rows: vec![
                vec![Some("=1+1".into()), Some("x,\"y\"".into())],
                vec![Some("-12.50".into()), None],
            ],
        };
        assert_eq!(to_csv(&t), "a,b\r\n'=1+1,\"x,\"\"y\"\"\"\r\n-12.50,\r\n");
    }
    #[test]
    fn keys_unique_and_columns_match_chart() {
        let mut k: Vec<_> = REPORTS.iter().map(|d| d.key).collect();
        k.sort();
        k.dedup();
        assert_eq!(k.len(), REPORTS.len());
        for d in REPORTS {
            if let Some((a, b)) = d.chart {
                assert!(a < d.columns.len() && b < d.columns.len(), "{}", d.key);
            }
            if let Some(m) = d.module {
                assert!(crate::gaming::modules::def(m).is_some(), "{}", d.key);
            }
        }
    }
}
