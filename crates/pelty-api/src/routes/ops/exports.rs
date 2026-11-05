//! Tax report (JSON or CSV) and invoice / payment CSV exports.
use crate::csv::{Cell, Csv};
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::routes::reports::{range, ReportQuery};
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use pelty_database::exports::{self, TaxRow};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, Date, OffsetDateTime};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/reports/tax", get(tax))
        .route("/admin/reports/tax.csv", get(tax_csv))
        .route("/admin/exports/invoices.csv", get(invoices_csv))
        .route("/admin/exports/payments.csv", get(payments_csv))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ExportQuery {
    /// First month, `YYYY-MM`.
    pub from: Option<String>,
    /// Last month (inclusive), `YYYY-MM`.
    pub to: Option<String>,
}

/// `[first day of from, first day after to)`.
fn period(q: ExportQuery) -> ApiResult<(Date, Date)> {
    let (from, to) = range(
        &ReportQuery {
            from: q.from,
            to: q.to,
        },
        OffsetDateTime::now_utc().date(),
    )?;
    let end = to
        .replace_day(1)
        .ok()
        .and_then(|d| d.checked_add(time::Duration::days(32)))
        .and_then(|d| d.replace_day(1).ok())
        .ok_or(pelty_core::AppError::Validation("error.invalid_range"))?;
    Ok((from, end))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TaxTotal {
    pub currency: String,
    #[schema(value_type = String)]
    pub net: Decimal,
    #[schema(value_type = String)]
    pub tax: Decimal,
    #[schema(value_type = String)]
    pub gross: Decimal,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TaxReport {
    pub from: String,
    pub to: String,
    pub rows: Vec<TaxRow>,
    /// Per-currency totals (currencies are never summed together).
    pub totals: Vec<TaxTotal>,
}

pub fn totals(rows: &[TaxRow]) -> Vec<TaxTotal> {
    let mut m: BTreeMap<&str, (Decimal, Decimal, Decimal)> = BTreeMap::new();
    for r in rows {
        let e = m.entry(&r.currency).or_default();
        e.0 += r.net;
        e.1 += r.tax;
        e.2 += r.gross;
    }
    m.into_iter()
        .map(|(c, (net, tax, gross))| TaxTotal {
            currency: c.into(),
            net,
            tax,
            gross,
        })
        .collect()
}

#[utoipa::path(get, path = "/api/v1/admin/reports/tax", params(ExportQuery), responses((status = 200, body = TaxReport), (status = 403), (status = 422)))]
pub async fn tax(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ReportsRead>,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Json<TaxReport>> {
    let (from, to) = period(q)?;
    let rows = exports::tax_report(&s.pool, from, to).await?;
    Ok(Json(TaxReport {
        from: from.to_string(),
        to: to.to_string(),
        totals: totals(&rows),
        rows,
    }))
}

fn csv_response(name: &str, from: Date, to: Date, body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}-{from}-{to}.csv\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        // BOM so spreadsheet apps read UTF-8 (diacritics) correctly.
        format!("\u{feff}{body}"),
    )
        .into_response()
}

fn ts(t: Option<OffsetDateTime>) -> String {
    t.and_then(|t| t.format(&Rfc3339).ok()).unwrap_or_default()
}

#[utoipa::path(get, path = "/api/v1/admin/reports/tax.csv", params(ExportQuery), responses((status = 200, content_type = "text/csv"), (status = 403)))]
pub async fn tax_csv(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ReportsRead>,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    let (from, to) = period(q)?;
    let rows = exports::tax_report(&s.pool, from, to).await?;
    let mut c = Csv::new(&[
        "period",
        "country",
        "currency",
        "tax_rate",
        "reverse_charge",
        "documents",
        "net",
        "tax",
        "gross",
    ]);
    for r in &rows {
        let docs = r.documents.to_string();
        c.row([
            Cell::Text(&r.period),
            Cell::Text(&r.country),
            Cell::Text(&r.currency),
            Cell::Dec(r.rate),
            Cell::Text(if r.reverse_charge { "yes" } else { "no" }),
            Cell::Text(&docs),
            Cell::Dec(r.net),
            Cell::Dec(r.tax),
            Cell::Dec(r.gross),
        ]);
    }
    Ok(csv_response("tax-report", from, to, c.finish()))
}

#[utoipa::path(get, path = "/api/v1/admin/exports/invoices.csv", params(ExportQuery), responses((status = 200, content_type = "text/csv"), (status = 403)))]
pub async fn invoices_csv(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ReportsRead>,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    let (from, to) = period(q)?;
    let mut c = Csv::new(&[
        "number",
        "kind",
        "status",
        "issued_at",
        "due_at",
        "paid_at",
        "customer",
        "email",
        "country",
        "vat_number",
        "reverse_charge",
        "currency",
        "subtotal",
        "tax",
        "total",
        "paid",
        "balance",
        "id",
    ]);
    for r in exports::invoices(&s.pool, from, to).await? {
        let (issued, due, paid, id) = (
            ts(Some(r.issued_at)),
            ts(r.due_at),
            ts(r.paid_at),
            r.id.to_string(),
        );
        let bal = if r.kind == "invoice" {
            r.total - r.amount_paid
        } else {
            Decimal::ZERO
        };
        c.row([
            Cell::Text(&r.number),
            Cell::Text(&r.kind),
            Cell::Text(&r.status),
            Cell::Text(&issued),
            Cell::Text(&due),
            Cell::Text(&paid),
            Cell::Text(&r.customer_name),
            Cell::Text(&r.customer_email),
            Cell::Text(&r.country),
            Cell::Text(&r.vat_number),
            Cell::Text(if r.reverse_charge { "yes" } else { "no" }),
            Cell::Text(&r.currency),
            Cell::Dec(r.subtotal),
            Cell::Dec(r.tax_total),
            Cell::Dec(r.total),
            Cell::Dec(r.amount_paid),
            Cell::Dec(bal),
            Cell::Text(&id),
        ]);
    }
    Ok(csv_response("invoices", from, to, c.finish()))
}

#[utoipa::path(get, path = "/api/v1/admin/exports/payments.csv", params(ExportQuery), responses((status = 200, content_type = "text/csv"), (status = 403)))]
pub async fn payments_csv(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ReportsRead>,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    let (from, to) = period(q)?;
    let mut c = Csv::new(&[
        "created_at",
        "applied_at",
        "invoice",
        "email",
        "gateway",
        "provider_ref",
        "status",
        "currency",
        "amount",
        "refunded",
        "net",
        "id",
    ]);
    for r in exports::payments(&s.pool, from, to).await? {
        let (created, applied, id) = (ts(Some(r.created_at)), ts(r.applied_at), r.id.to_string());
        c.row([
            Cell::Text(&created),
            Cell::Text(&applied),
            Cell::Text(&r.invoice_number),
            Cell::Text(&r.customer_email),
            Cell::Text(&r.gateway),
            Cell::Text(&r.provider_ref),
            Cell::Text(&r.status),
            Cell::Text(&r.currency),
            Cell::Dec(r.amount),
            Cell::Dec(r.refunded),
            Cell::Dec(r.amount - r.refunded),
            Cell::Text(&id),
        ]);
    }
    Ok(csv_response("payments", from, to, c.finish()))
}
