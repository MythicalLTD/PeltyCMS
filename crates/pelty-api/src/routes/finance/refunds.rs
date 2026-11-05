//! Refunds against payments (partial or full) through the payment gateway,
//! and the credit notes they produce.
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::money::{Currency, Money};
use pelty_core::AppError;
use pelty_database::refunds::{self, Refund, Reserved};
use pelty_database::{audit, invoices};
use pelty_extensions::events::DomainEvent;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/payments/{id}/refunds", post(admin_refund))
        .route("/admin/settings/reversal", get(get_reversal).put(put_reversal))
        .route("/admin/invoices/{id}/refunds", get(admin_list))
        .route("/client/invoices/{id}/refunds", get(client_list))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefundBody {
    /// Omit for a full refund of what is left on the payment.
    #[schema(value_type = Option<String>)]
    pub amount: Option<Decimal>,
    #[serde(default)]
    pub reason: String,
    /// Client-chosen; replays with the same key return the same refund.
    pub idempotency_key: String,
    /// Add the refund to the customer's credit balance instead of the gateway.
    #[serde(default)]
    pub to_credit: bool,
    /// Record a chargeback/reversal: the money is already gone, so the
    /// gateway is not called. Not combinable with `to_credit`.
    #[serde(default)]
    pub reversal: bool,
    /// Reversal only: reopen the invoice as unpaid instead of issuing a
    /// credit note. Defaults to the `billing.reversal` setting.
    pub reopen_invoice: Option<bool>,
    /// Reversal only, with reopen: suspend the invoice's services.
    pub suspend_services: Option<bool>,
}

pub const REVERSAL_KEY: &str = "billing.reversal";

#[derive(Debug, Default, Clone, Copy, serde::Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReversalSettings {
    #[serde(default)]
    pub reopen_invoice: bool,
    #[serde(default)]
    pub suspend_services: bool,
}

async fn reversal_settings(s: &AppState) -> ApiResult<ReversalSettings> {
    Ok(pelty_database::settings::get(&s.pool, REVERSAL_KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

#[utoipa::path(get, path = "/api/v1/admin/settings/reversal", responses((status = 200, body = ReversalSettings)))]
pub async fn get_reversal(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<ReversalSettings>> {
    Ok(Json(reversal_settings(&s).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/reversal", request_body = ReversalSettings, responses((status = 200, body = ReversalSettings)))]
pub async fn put_reversal(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<ReversalSettings>) -> ApiResult<Json<ReversalSettings>> {
    pelty_database::settings::set(&s.pool, REVERSAL_KEY, json!(b)).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.reversal", None, u.ip.as_deref(), json!(b)).await?;
    Ok(Json(b))
}

/// Execute a reserved refund with its gateway and settle the outcome.
pub async fn execute(s: &AppState, r: Reserved) -> ApiResult<Refund> {
    let refund = r.refund;
    if r.replay || refund.status != "pending" {
        return Ok(refund);
    }
    if refund.to_credit || refund.gateway == "credit" {
        return Ok(refunds::complete(&s.pool, refund.id, None).await?);
    }
    let fail = |msg: String| async move {
        refunds::fail(&s.pool, refund.id, &msg).await?;
        Ok::<(), AppError>(())
    };
    let Some(desc) = s.gateways.descriptor(&refund.gateway).filter(|d| d.capabilities.refunds) else {
        fail("gateway does not support refunds".into()).await?;
        return Err(AppError::Conflict("error.refund_unsupported").into());
    };
    let row = pelty_database::extensions::gateway(&s.pool, desc.id).await?.ok_or(AppError::Conflict("error.refund_unsupported"))?;
    let gw = match s.gateways.build(desc.id, &crate::routes::webhooks::resolve_config(&row)) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!(gateway = %refund.gateway, error = %e, "gateway misconfigured");
            fail(e.to_string()).await?;
            return Err(AppError::BadRequest("error.gateway").into());
        }
    };
    let currency = Currency::parse(&refund.currency).ok_or(AppError::Validation("error.invalid_currency"))?;
    let target = r.payment_ref.unwrap_or_else(|| refund.payment_id.to_string());
    let key = format!("pelty-refund-{}", refund.id);
    match gw.refund(&target, Money::new(refund.amount, currency), &key).await {
        Ok(res) => match res.status.as_str() {
            "succeeded" | "manual_pending" => Ok(refunds::complete(&s.pool, refund.id, res.provider_ref.as_deref()).await?),
            "failed" | "canceled" => {
                fail(format!("gateway status {}", res.status)).await?;
                Err(AppError::BadRequest("error.refund_failed").into())
            }
            _ => {
                refunds::mark_pending_ref(&s.pool, refund.id, res.provider_ref.as_deref()).await?;
                Ok(refunds::get(&s.pool, refund.id).await?.ok_or(AppError::NotFound)?)
            }
        },
        Err(e) => {
            tracing::warn!(gateway = %refund.gateway, error = %e, "refund failed");
            fail(e.to_string()).await?;
            Err(AppError::BadRequest("error.refund_failed").into())
        }
    }
}

#[utoipa::path(post, path = "/api/v1/admin/payments/{id}/refunds", request_body = RefundBody, responses((status = 200, body = Refund), (status = 400), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_refund(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RefundBody>,
) -> ApiResult<Json<Refund>> {
    if b.reversal {
        let gw: Option<String> = sqlx::query_scalar("SELECT gateway FROM payments WHERE id = $1").bind(id).fetch_optional(&s.pool).await.map_err(pelty_database::db_err)?;
        if b.to_credit || gw.as_deref() == Some("credit") {
            return Err(AppError::Validation("error.reversal_invalid").into());
        }
    }
    let reserved = refunds::reserve(&s.pool, id, b.amount, &b.reason, &b.idempotency_key, Some(u.user_id), b.to_credit).await?;
    let replay = reserved.replay;
    let r = if b.reversal && !replay && reserved.refund.status == "pending" {
        refunds::mark_reversal(&s.pool, reserved.refund.id).await?;
        let cfg = reversal_settings(&s).await?;
        if b.reopen_invoice.unwrap_or(cfg.reopen_invoice) {
            let opts = refunds::Reopen { suspend: b.suspend_services.unwrap_or(cfg.suspend_services) };
            let (r, suspended) = refunds::complete_reopen(&s.pool, reserved.refund.id, opts).await?;
            for sid in suspended {
                crate::provision::on_transition(&s, sid, "active", "suspended").await;
                s.emit(DomainEvent::new("service.suspended", json!({ "service_id": sid, "from": "active", "reason": "payment_reversed" })));
            }
            r
        } else {
            refunds::complete(&s.pool, reserved.refund.id, None).await?
        }
    } else {
        execute(&s, reserved).await?
    };
    if !replay {
        audit::record(&s.pool, Some(u.user_id), if r.reversal { "payment.reversed" } else { "payment.refunded" }, Some(&id.to_string()), u.ip.as_deref(),
            json!({ "refund_id": r.id, "amount": r.amount, "status": r.status })).await?;
        if r.status == "succeeded" {
            s.emit(DomainEvent::new("invoice.refunded", json!({ "invoice_id": r.invoice_id, "refund_id": r.id, "credit_note_id": r.credit_note_id, "amount": r.amount })));
        }
    }
    Ok(Json(r))
}

async fn listing(s: &AppState, inv: &invoices::Invoice) -> ApiResult<Value> {
    let list = refunds::for_invoice(&s.pool, inv.id).await?;
    let notes = s.invoices().credit_notes_for(inv.id).await?;
    let mut refundable = serde_json::Map::new();
    for p in s.invoices().payments(inv.id).await? {
        refundable.insert(p.id.to_string(), json!(refunds::refundable(&s.pool, p.id).await?));
    }
    Ok(json!({ "refunds": list, "credit_notes": notes, "refundable": refundable }))
}

#[utoipa::path(get, path = "/api/v1/admin/invoices/{id}/refunds", responses((status = 200), (status = 404)))]
pub async fn admin_list(State(s): State<AppState>, _p: RequirePermission<perms::InvoicesManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(listing(&s, &inv).await?))
}

#[utoipa::path(get, path = "/api/v1/client/invoices/{id}/refunds", responses((status = 200), (status = 404)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let inv = s.invoices().get_owned(id, cid).await?.ok_or(AppError::NotFound)?;
    let mut v = listing(&s, &inv).await?;
    // Clients don't need internal gateway errors or per-payment capacity.
    if let Some(list) = v["refunds"].as_array_mut() {
        for r in list {
            r["error"] = Value::Null;
        }
    }
    v["refundable"] = json!({});
    Ok(Json(v))
}
