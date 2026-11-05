//! Quote PDF download (client: own sent/decided quotes; staff: any), rendered
//! with the same branded layout as invoices.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::pdf::{self, InvoiceDoc, Line};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use pelty_core::AppError;
use pelty_database::{branding, overdue, quotes};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/quotes/{id}/pdf", get(client_pdf))
        .route("/admin/quotes/{id}/pdf", get(admin_pdf))
}

pub async fn build(s: &AppState, q: &quotes::Quote, locale: &str) -> ApiResult<Vec<u8>> {
    let t = |k: &str| s.i18n.t(locale, k).to_string();
    let b = branding::get(&s.pool).await?;
    let exp = branding::exponent(&s.pool, &q.currency).await?;
    let cur = q.currency.as_str();
    let money = |v: rust_decimal::Decimal| format!("{} {cur}", v.round_dp(exp));
    let mut seller = vec![if b.company_name.is_empty() {
        t("app.name")
    } else {
        b.company_name.clone()
    }];
    seller.extend(
        b.address
            .lines()
            .map(str::to_string)
            .filter(|l| !l.trim().is_empty()),
    );
    if !b.tax_id.is_empty() {
        seller.push(format!("{}: {}", t("pdf.tax_id"), b.tax_id));
    }
    let cust = s
        .clients()
        .get(q.customer_id)
        .await?
        .map(|c| serde_json::to_value(c).unwrap_or_default())
        .unwrap_or_default();
    let doc = InvoiceDoc {
        title: t("pdf.quote"),
        number: q.number.clone(),
        meta: vec![
            (t("pdf.issued"), q.created_at.date().to_string()),
            (t("pdf.valid_until"), q.valid_until.to_string()),
            (t("pdf.status"), t(&format!("quote_status.{}", q.status))),
        ],
        seller,
        bill_to_label: t("pdf.bill_to"),
        bill_to: crate::routes::invoice_pdf::bill_to(&cust),
        headers: [
            t("pdf.description"),
            t("pdf.qty"),
            t("pdf.unit_price"),
            t("pdf.tax"),
            t("pdf.amount"),
        ],
        lines: q
            .items
            .iter()
            .map(|i| Line {
                description: i.description.clone(),
                quantity: i.quantity.normalize().to_string(),
                unit_price: i.unit_price.round_dp(exp).to_string(),
                tax: format!("{}%", i.tax_rate.normalize()),
                amount: money(i.amount),
            })
            .collect(),
        totals: vec![
            (t("pdf.subtotal"), money(q.subtotal), false),
            (t("pdf.tax"), money(q.tax_total), false),
            (t("pdf.total"), money(q.total), true),
        ],
        notes: [q.subject.clone(), q.notes.clone()]
            .into_iter()
            .filter(|n| !n.trim().is_empty())
            .collect(),
        footer: b.footer.lines().map(str::to_string).collect(),
        accent: b.rgb(),
        page_label: t("pdf.page"),
        logo: crate::routes::invoice_pdf::logo(s).await,
    };
    Ok(pdf::render(&doc))
}

fn respond(q: &quotes::Quote, bytes: Vec<u8>) -> Response {
    let name: String = q
        .number
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"quote-{name}.pdf\""),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        bytes,
    )
        .into_response()
}

async fn locale_for(s: &AppState, cid: Uuid) -> ApiResult<String> {
    Ok(overdue::recipient(&s.pool, cid)
        .await?
        .and_then(|r| r.2)
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone()))
}

#[utoipa::path(get, path = "/api/v1/client/quotes/{id}/pdf", responses((status = 200, content_type = "application/pdf"), (status = 404)))]
pub async fn client_pdf(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let cid = customer_of(&s, &u).await?;
    let q = quotes::get(&s.pool, id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    if q.status == "draft" {
        return Err(AppError::NotFound.into());
    }
    let bytes = build(&s, &q, &locale_for(&s, cid).await?).await?;
    Ok(respond(&q, bytes))
}

#[utoipa::path(get, path = "/api/v1/admin/quotes/{id}/pdf", responses((status = 200, content_type = "application/pdf"), (status = 404)))]
pub async fn admin_pdf(
    State(s): State<AppState>,
    _p: RequirePermission<perms::QuotesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let q = quotes::get(&s.pool, id, None)
        .await?
        .ok_or(AppError::NotFound)?;
    let bytes = build(&s, &q, &locale_for(&s, q.customer_id).await?).await?;
    Ok(respond(&q, bytes))
}
