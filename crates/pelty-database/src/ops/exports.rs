//! Tax report and accounting exports. All money stays `Decimal`; credit
//! notes count negative so a period nets out correctly.
use crate::db_err;
use pelty_core::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// Issued, non-void documents in `[from, to)` (by issue date, UTC).
const ISSUED: &str = "i.number IS NOT NULL AND i.status NOT IN ('draft','void') AND i.issued_at >= $1::date AND i.issued_at < $2::date";

#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TaxRow {
    /// `YYYY-MM`
    pub period: String,
    pub country: String,
    pub currency: String,
    #[schema(value_type = String)]
    pub rate: Decimal,
    pub reverse_charge: bool,
    pub documents: i64,
    #[schema(value_type = String)]
    pub net: Decimal,
    #[schema(value_type = String)]
    pub tax: Decimal,
    #[schema(value_type = String)]
    pub gross: Decimal,
}

/// Rates are rounded to 2 dp: partial-refund credit notes carry an effective (derived) rate.
pub async fn tax_report(pool: &PgPool, from: Date, to: Date) -> AppResult<Vec<TaxRow>> {
    sqlx::query_as(&format!(
        "SELECT to_char(i.issued_at AT TIME ZONE 'UTC', 'YYYY-MM') AS period,
                COALESCE(NULLIF(i.customer_snapshot->>'country', ''), NULLIF(c.country, ''), '--') AS country,
                i.currency::text AS currency, round(it.tax_rate, 2) AS rate, i.reverse_charge,
                count(DISTINCT i.id) AS documents,
                sum(CASE WHEN i.kind = 'credit_note' THEN -it.amount ELSE it.amount END) AS net,
                sum(CASE WHEN i.kind = 'credit_note' THEN -it.tax_amount ELSE it.tax_amount END) AS tax,
                sum(CASE WHEN i.kind = 'credit_note' THEN -(it.amount + it.tax_amount) ELSE it.amount + it.tax_amount END) AS gross
         FROM invoices i JOIN invoice_items it ON it.invoice_id = i.id LEFT JOIN customers c ON c.id = i.customer_id
         WHERE {ISSUED}
         GROUP BY 1, 2, 3, 4, 5 ORDER BY 1, 2, 3, 4, 5"
    ))
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InvoiceRow {
    pub id: Uuid,
    pub number: String,
    pub kind: String,
    pub status: String,
    pub issued_at: OffsetDateTime,
    pub due_at: Option<OffsetDateTime>,
    pub paid_at: Option<OffsetDateTime>,
    pub customer_email: String,
    pub customer_name: String,
    pub country: String,
    pub vat_number: String,
    pub currency: String,
    pub subtotal: Decimal,
    pub tax_total: Decimal,
    pub total: Decimal,
    pub amount_paid: Decimal,
    pub reverse_charge: bool,
}

pub async fn invoices(pool: &PgPool, from: Date, to: Date) -> AppResult<Vec<InvoiceRow>> {
    sqlx::query_as(&format!(
        "SELECT i.id, i.number, i.kind, i.status, i.issued_at, i.due_at, i.paid_at,
                COALESCE(i.customer_snapshot->>'email', c.email::text, '') AS customer_email,
                COALESCE(NULLIF(i.customer_snapshot->>'organization', ''), NULLIF(trim(COALESCE(i.customer_snapshot->>'first_name', c.first_name, '') || ' ' || COALESCE(i.customer_snapshot->>'last_name', c.last_name, '')), ''), '') AS customer_name,
                COALESCE(NULLIF(i.customer_snapshot->>'country', ''), c.country, '') AS country,
                COALESCE(i.customer_snapshot->>'vat_number', c.vat_number, '') AS vat_number,
                i.currency::text AS currency, i.subtotal, i.tax_total, i.total, i.amount_paid, i.reverse_charge
         FROM invoices i LEFT JOIN customers c ON c.id = i.customer_id
         WHERE {ISSUED} ORDER BY i.issued_at, i.number LIMIT 100000"
    ))
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PaymentRow {
    pub id: Uuid,
    pub created_at: OffsetDateTime,
    pub applied_at: Option<OffsetDateTime>,
    pub invoice_number: String,
    pub customer_email: String,
    pub gateway: String,
    pub provider_ref: String,
    pub status: String,
    pub currency: String,
    pub amount: Decimal,
    pub refunded: Decimal,
}

/// Payments created in `[from, to)`, with refunded totals.
pub async fn payments(pool: &PgPool, from: Date, to: Date) -> AppResult<Vec<PaymentRow>> {
    sqlx::query_as(
        "SELECT p.id, p.created_at, p.applied_at, COALESCE(i.number, '') AS invoice_number, COALESCE(c.email::text, '') AS customer_email,
                p.gateway, COALESCE(p.provider_ref, '') AS provider_ref, p.status, p.currency::text AS currency, p.amount,
                COALESCE((SELECT sum(r.amount) FROM refunds r WHERE r.payment_id = p.id AND r.status = 'succeeded'), 0) AS refunded
         FROM payments p LEFT JOIN invoices i ON i.id = p.invoice_id LEFT JOIN customers c ON c.id = COALESCE(p.customer_id, i.customer_id)
         WHERE p.invoice_id IS NOT NULL AND p.created_at >= $1::date AND p.created_at < $2::date
         ORDER BY p.created_at, p.id LIMIT 100000",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}
