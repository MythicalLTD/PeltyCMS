//! E-invoicing: UBL 2.1 / Peppol BIS 3.0 (optionally CIUS-RO for RO
//! e-Factura) download for issued invoices, credit notes and debit notes,
//! plus the legal-field settings. See `crate::einvoice` for the builder.
use crate::einvoice::{self, Doc, Line, Party, Settings};
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, branding, invoices, settings};
use serde_json::{json, Value};
use uuid::Uuid;

const KEY: &str = "einvoice";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/invoices/{id}/ubl", get(client_ubl))
        .route("/admin/invoices/{id}/ubl", get(admin_ubl))
        .route(
            "/admin/settings/einvoice",
            get(get_settings).put(put_settings),
        )
}

pub async fn load(s: &AppState) -> ApiResult<Settings> {
    let v = settings::get(&s.pool, KEY).await?;
    Ok(v.and_then(|v| serde_json::from_value::<Settings>(v).ok())
        .unwrap_or_default()
        .normalized()
        .unwrap_or_default())
}

fn txt(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Build the UBL XML for an issued document; 409 with the missing fields
/// when legal data required by EN 16931 is absent.
pub async fn build(s: &AppState, inv: &invoices::Invoice) -> ApiResult<String> {
    let number = inv.number.clone().ok_or(AppError::NotFound)?;
    let mut cfg = load(s).await?;
    // Fall back to the frozen legal entity / invoice branding for the seller.
    let entity = pelty_database::legal_entities::seller_snapshot(&s.pool, inv.id).await?;
    let b = branding::get(&s.pool).await?;
    if cfg.seller_name.is_empty() {
        cfg.seller_name = entity
            .as_ref()
            .map(|e| e.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or(b.company_name.clone());
    }
    if cfg.seller_vat_id.is_empty() {
        cfg.seller_vat_id = entity
            .as_ref()
            .map(|e| e.tax_id.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or(b.tax_id.clone());
    }
    if cfg.iban.is_empty() {
        if let Some(e) = &entity {
            cfg.iban = e.iban.replace(' ', "");
            cfg.bic = e.swift.clone();
        }
    }
    // Snapshot fields win; empty ones fall back to the live profile (as exports do).
    let live = s
        .clients()
        .get(inv.customer_id)
        .await?
        .map(|c| serde_json::to_value(c).unwrap_or_default())
        .unwrap_or_default();
    let mut snap = inv
        .customer_snapshot
        .clone()
        .unwrap_or_else(|| live.clone());
    if let (Some(m), Some(l)) = (snap.as_object_mut(), live.as_object()) {
        for (k, v) in l {
            let empty = m
                .get(k)
                .map(|x| x.is_null() || x.as_str() == Some(""))
                .unwrap_or(true);
            if empty {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    let person = format!("{} {}", txt(&snap, "first_name"), txt(&snap, "last_name"))
        .trim()
        .to_string();
    let org = txt(&snap, "organization");
    let street = [txt(&snap, "address_line1"), txt(&snap, "address_line2")]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let buyer = Party {
        name: if org.is_empty() { person } else { org },
        vat_id: txt(&snap, "vat_number"),
        company_id: txt(&snap, "tax_id"),
        street,
        city: txt(&snap, "city"),
        subdivision: txt(&snap, "region"),
        postal_code: txt(&snap, "postal_code"),
        country: txt(&snap, "country"),
        email: txt(&snap, "email"),
    };
    let missing = einvoice::missing(&cfg, &buyer);
    if !missing.is_empty() {
        return Err(AppError::Conflict(missing[0]).into());
    }
    let reference = match inv.credit_for {
        Some(r) => s.invoices().get(r).await?.and_then(|o| o.number),
        None => None,
    };
    let note = if inv.kind == "invoice" {
        String::new()
    } else {
        s.invoices().note_reason(inv.id).await?
    };
    let lines = s
        .invoices()
        .items(inv.id)
        .await?
        .into_iter()
        .map(|i| Line {
            description: i.description,
            quantity: i.quantity,
            unit_price: i.unit_price,
            net: i.amount,
            rate: i.tax_rate,
        })
        .collect();
    let date = |d: Option<time::OffsetDateTime>| d.map(|d| d.date().to_string());
    let doc = Doc {
        kind: inv.kind.clone(),
        number,
        issue_date: date(inv.issued_at).unwrap_or_default(),
        due_date: date(inv.due_at),
        currency: inv.currency.clone(),
        exponent: branding::exponent(&s.pool, &inv.currency).await?,
        reverse_charge: inv.reverse_charge,
        prepaid: if inv.kind == "credit_note" {
            rust_decimal::Decimal::ZERO
        } else {
            inv.amount_paid
        },
        billing_reference: reference,
        note,
        lines,
    };
    Ok(einvoice::render(&doc, &cfg, &buyer))
}

fn respond(inv: &invoices::Invoice, xml: String) -> Response {
    let name: String = inv
        .number
        .clone()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (
        [
            (
                header::CONTENT_TYPE,
                "application/xml; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}.xml\""),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        xml,
    )
        .into_response()
}

/// UBL 2.1 (Peppol BIS 3.0 / CIUS-RO) XML of one of the caller's issued invoices.
#[utoipa::path(get, path = "/api/v1/client/invoices/{id}/ubl", responses((status = 200, content_type = "application/xml"), (status = 404), (status = 409)))]
pub async fn client_ubl(
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
    let xml = build(&s, &inv).await?;
    Ok(respond(&inv, xml))
}

/// UBL 2.1 (Peppol BIS 3.0 / CIUS-RO) XML of any issued invoice.
#[utoipa::path(get, path = "/api/v1/admin/invoices/{id}/ubl", responses((status = 200, content_type = "application/xml"), (status = 404), (status = 409)))]
pub async fn admin_ubl(
    State(s): State<AppState>,
    _p: RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    let xml = build(&s, &inv).await?;
    Ok(respond(&inv, xml))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/einvoice", responses((status = 200, body = Settings), (status = 403)))]
pub async fn get_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Settings>> {
    Ok(Json(load(&s).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/einvoice", request_body = Settings, responses((status = 200, body = Settings), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<Settings>,
) -> ApiResult<Json<Settings>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    settings::set(&s.pool, KEY, json!(b)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.einvoice",
        None,
        u.ip.as_deref(),
        json!({ "profile": b.profile }),
    )
    .await?;
    Ok(Json(b))
}
