//! Branded invoice PDF download (client: own invoices; staff: any) and the
//! branding settings.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::pdf::{self, InvoiceDoc, Line};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::branding::{self, InvoiceBranding};
use pelty_database::{audit, invoices, overdue};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/invoices/{id}/pdf", get(client_pdf))
        .route("/admin/invoices/{id}/pdf", get(admin_pdf))
        .route(
            "/admin/settings/invoice-branding",
            get(get_branding).put(put_branding),
        )
}

/// Logo for PDFs: the general-settings logo when it is a local path under the
/// frontend dir (no network fetches), else the bundled brand logo.
pub(crate) async fn logo(s: &AppState) -> Option<pdf::Logo> {
    let dir = s.cfg.frontend_dir.clone();
    let url = pelty_database::general_settings::load(&s.pool)
        .await
        .ok()
        .map(|g| g.general.logo_url)
        .unwrap_or_default();
    let mut candidates = Vec::new();
    if let Some(id) = url
        .strip_prefix("/api/v1/media/")
        .or_else(|| {
            url.split_once("/api/v1/media/")
                .map(|(_, rest)| rest.trim_end_matches('/'))
        })
        .map(|id| id.split(['?', '#']).next().unwrap_or(id).trim())
        .filter(|id| !id.is_empty() && !id.contains('/') && !id.contains(".."))
    {
        if let Some(p) = crate::routes::media::file_for(id) {
            candidates.push(p);
        }
    }
    let local = url.strip_prefix('/').filter(|p| {
        !p.is_empty()
            && !p.starts_with('/')
            && !p.starts_with("api/")
            && !p.split('/').any(|c| c == ".." || c.is_empty())
    });
    if let Some(p) = local {
        candidates.push(dir.join(p));
    }
    candidates.push(dir.join("brand/pelty-logo-128.png"));
    candidates.push(std::path::PathBuf::from(
        "frontend/public/brand/pelty-logo-128.png",
    ));
    for c in candidates {
        if let Ok(bytes) = tokio::fs::read(&c).await {
            if let Some(l) = pdf::Logo::decode(&bytes) {
                return Some(l);
            }
        }
    }
    if let Some(l) =
        crate::static_files::embedded_file("brand/pelty-logo-128.png").and_then(pdf::Logo::decode)
    {
        return Some(l);
    }
    None
}

fn money(v: Decimal, exp: u32, cur: &str) -> String {
    format!("{} {cur}", v.round_dp(exp))
}

fn opt(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Bill-to lines from the frozen snapshot (or the live customer record).
pub(crate) fn bill_to(c: &Value) -> Vec<String> {
    let name = format!("{} {}", opt(c, "first_name"), opt(c, "last_name"))
        .trim()
        .to_string();
    let city = [opt(c, "postal_code"), opt(c, "city")]
        .join(" ")
        .trim()
        .to_string();
    let region = [opt(c, "region"), opt(c, "country")]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let mut out: Vec<String> = [
        opt(c, "organization"),
        name,
        opt(c, "address_line1"),
        opt(c, "address_line2"),
        city,
        region,
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    if out.is_empty() {
        out.push(opt(c, "email"));
    }
    for (k, label) in [("vat_number", "VAT"), ("tax_id", "Tax ID")] {
        let v = opt(c, k);
        if !v.is_empty() {
            out.push(format!("{label}: {v}"));
        }
    }
    out
}

/// Addon filter `invoice.pdf` over the free-text parts of the PDF.
async fn pdf_filter(
    s: &AppState,
    inv: &invoices::Invoice,
    locale: &str,
    notes: Vec<String>,
    footer: Vec<String>,
) -> (Vec<String>, Vec<String>) {
    if !crate::addon_filters::has(s, "invoice.pdf") {
        return (notes, footer);
    }
    let out = crate::addon_filters::apply_with(
        s,
        "invoice.pdf",
        serde_json::json!({ "notes": notes, "footer": footer }),
        serde_json::json!({ "invoice_id": inv.id, "customer_id": inv.customer_id, "locale": locale }),
        false,
    )
    .await;
    (
        clamp_lines(&out["notes"], 20).unwrap_or(notes),
        clamp_lines(&out["footer"], 6).unwrap_or(footer),
    )
}

/// String array → at most `max` lines of 300 chars without control chars.
fn clamp_lines(v: &serde_json::Value, max: usize) -> Option<Vec<String>> {
    let a = v.as_array()?;
    Some(
        a.iter()
            .filter_map(serde_json::Value::as_str)
            .map(|l| l.chars().filter(|c| !c.is_control()).take(300).collect())
            .take(max)
            .collect(),
    )
}

pub async fn build(s: &AppState, inv: &invoices::Invoice, locale: &str) -> ApiResult<Vec<u8>> {
    let t = |k: &str| s.i18n.t(locale, k).to_string();
    let b = branding::get(&s.pool).await?;
    let exp = branding::exponent(&s.pool, &inv.currency).await?;
    let cur = inv.currency.as_str();
    let items = s.invoices().items(inv.id).await?;
    let snapshot = match &inv.customer_snapshot {
        Some(v) => v.clone(),
        None => s
            .clients()
            .get(inv.customer_id)
            .await?
            .map(|c| serde_json::to_value(c).unwrap_or_default())
            .unwrap_or_default(),
    };
    // Issued documents use the frozen legal-entity snapshot; else branding.
    let entity = pelty_database::legal_entities::seller_snapshot(&s.pool, inv.id).await?;
    let (name, address, tax_id) = match &entity {
        Some(e) => (e.name.clone(), e.address.clone(), e.tax_id.clone()),
        None => (b.company_name.clone(), b.address.clone(), b.tax_id.clone()),
    };
    let company = if name.is_empty() { t("app.name") } else { name };
    let mut seller = vec![company];
    seller.extend(
        address
            .lines()
            .map(str::to_string)
            .filter(|l| !l.trim().is_empty()),
    );
    if !tax_id.is_empty() {
        seller.push(format!("{}: {}", t("pdf.tax_id"), tax_id));
    }
    let mut footer: Vec<String> = vec![];
    match &entity {
        Some(e) => {
            let bank: Vec<String> = [
                (t("pdf.bank"), &e.bank_name),
                ("IBAN".to_string(), &e.iban),
                ("SWIFT".to_string(), &e.swift),
            ]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
            if !bank.is_empty() {
                footer.push(bank.join(" · "));
            }
            footer.extend(
                e.footer
                    .lines()
                    .map(str::to_string)
                    .filter(|l| !l.trim().is_empty()),
            );
        }
        None => footer.extend(b.footer.lines().map(str::to_string)),
    }
    let date = |d: Option<time::OffsetDateTime>| {
        d.map(|d| d.date().to_string())
            .unwrap_or_else(|| "-".into())
    };
    let mut meta = vec![
        (t("pdf.issued"), date(inv.issued_at)),
        (t("pdf.due"), date(inv.due_at)),
        (t("pdf.status"), t(&format!("status.{}", inv.status))),
    ];
    if let Some(p) = inv.paid_at {
        meta.push((t("pdf.paid_on"), p.date().to_string()));
    }
    let title = t(match inv.kind.as_str() {
        "credit_note" => "pdf.credit_note",
        "debit_note" => "pdf.debit_note",
        _ => "pdf.invoice",
    });
    if let Some(r) = inv.credit_for {
        if let Some(orig) = s.invoices().get(r).await? {
            meta.push((t("pdf.related_invoice"), orig.number.unwrap_or_default()));
        }
        let reason = s.invoices().note_reason(inv.id).await?;
        if !reason.is_empty() {
            meta.push((t("pdf.reason"), reason));
        }
    }
    let lines = items
        .iter()
        .map(|i| Line {
            description: i.description.clone(),
            quantity: i.quantity.normalize().to_string(),
            unit_price: i.unit_price.round_dp(exp).to_string(),
            tax: format!("{}%", i.tax_rate.normalize()),
            amount: money(i.amount, exp, cur),
        })
        .collect();
    let mut totals = vec![(t("pdf.subtotal"), money(inv.subtotal, exp, cur), false)];
    match inv.tax_breakdown.as_array().filter(|a| !a.is_empty()) {
        Some(parts) => {
            for p in parts {
                let rate = p["rate"]
                    .as_str()
                    .and_then(|r| r.parse::<Decimal>().ok())
                    .map(|r| r.normalize().to_string())
                    .unwrap_or_default();
                let amt = p["amount"]
                    .as_str()
                    .and_then(|r| r.parse::<Decimal>().ok())
                    .unwrap_or_default();
                totals.push((
                    format!("{} {rate}%", p["name"].as_str().unwrap_or_default()),
                    money(amt, exp, cur),
                    false,
                ));
            }
        }
        None => totals.push((t("pdf.tax"), money(inv.tax_total, exp, cur), false)),
    }
    totals.push((t("pdf.total"), money(inv.total, exp, cur), true));
    if inv.amount_paid > Decimal::ZERO {
        totals.push((t("pdf.paid"), money(inv.amount_paid, exp, cur), false));
        totals.push((t("pdf.balance_due"), money(inv.balance(), exp, cur), true));
    }
    let mut notes = Vec::new();
    if inv.reverse_charge {
        notes.push(t("pdf.reverse_charge"));
    }
    if inv.tax_inclusive {
        notes.push(t("pdf.tax_inclusive"));
    }
    if let Some(e) = entity.as_ref().filter(|e| !e.terms.is_empty()) {
        notes.extend(
            e.terms
                .lines()
                .map(str::to_string)
                .filter(|l| !l.trim().is_empty()),
        );
    }
    let unnumbered = if inv.finalized_at.is_some() {
        t("pdf.proforma")
    } else {
        t("pdf.draft")
    };
    if inv.number.is_none() {
        notes.push(unnumbered.clone());
    }
    let (notes, footer) = pdf_filter(s, inv, locale, notes, footer).await;
    let doc = InvoiceDoc {
        title,
        number: inv.number.clone().unwrap_or(unnumbered),
        meta,
        seller,
        bill_to_label: t("pdf.bill_to"),
        bill_to: bill_to(&snapshot),
        headers: [
            t("pdf.description"),
            t("pdf.qty"),
            t("pdf.unit_price"),
            t("pdf.tax"),
            t("pdf.amount"),
        ],
        lines,
        totals,
        notes,
        footer,
        accent: b.rgb(),
        page_label: t("pdf.page"),
        logo: logo(s).await,
    };
    Ok(pdf::render(&doc))
}

fn respond(inv: &invoices::Invoice, bytes: Vec<u8>) -> Response {
    let name: String = inv
        .number
        .clone()
        .unwrap_or_else(|| {
            if inv.finalized_at.is_some() {
                format!("proforma-{}", inv.id)
            } else {
                "draft".into()
            }
        })
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"invoice-{name}.pdf\""),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        bytes,
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/v1/client/invoices/{id}/pdf", responses((status = 200, content_type = "application/pdf"), (status = 404)))]
pub async fn client_pdf(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let cid = customer_of(&s, &u).await?;
    let inv = s
        .invoices()
        .get_owned(id, cid)
        .await?
        .ok_or(AppError::NotFound)?;
    if inv.finalized_at.is_none() {
        return Err(AppError::NotFound.into());
    }
    let locale = overdue::recipient(&s.pool, cid)
        .await?
        .and_then(|r| r.2)
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let bytes = build(&s, &inv, &locale).await?;
    Ok(respond(&inv, bytes))
}

#[utoipa::path(get, path = "/api/v1/admin/invoices/{id}/pdf", responses((status = 200, content_type = "application/pdf"), (status = 404)))]
pub async fn admin_pdf(
    State(s): State<AppState>,
    _p: RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    let locale = overdue::recipient(&s.pool, inv.customer_id)
        .await?
        .and_then(|r| r.2)
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let bytes = build(&s, &inv, &locale).await?;
    Ok(respond(&inv, bytes))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/invoice-branding", responses((status = 200, body = InvoiceBranding), (status = 403)))]
pub async fn get_branding(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<InvoiceBranding>> {
    Ok(Json(branding::get(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/invoice-branding", request_body = InvoiceBranding, responses((status = 200, body = InvoiceBranding), (status = 422)))]
pub async fn put_branding(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<InvoiceBranding>,
) -> ApiResult<Json<InvoiceBranding>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    branding::put(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.invoice_branding",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(b))
}

#[cfg(test)]
mod pdf_filter_tests {
    use super::clamp_lines;
    use serde_json::json;

    #[test]
    fn clamps_addon_lines() {
        let v = json!(["a\nb", "x".repeat(400), 3, "c"]);
        let out = clamp_lines(&v, 2).unwrap();
        assert_eq!(out, vec!["ab".to_string(), "x".repeat(300)]);
        assert!(clamp_lines(&json!("nope"), 2).is_none());
    }
}
