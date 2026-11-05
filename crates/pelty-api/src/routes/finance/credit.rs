//! Customer credit balance: client view + apply to invoice, staff adjust.
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_database::add_funds::{self, AddFundsSettings};
use pelty_database::{audit, credit};
use pelty_extensions::events::DomainEvent;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/credit", get(client_credit))
        .route("/client/invoices/{id}/apply-credit", post(client_apply))
        .route("/admin/customers/{id}/credit", get(admin_credit).post(admin_adjust))
        .route("/admin/invoices/{id}/apply-credit", post(admin_apply))
        .route("/client/credit/add-funds", get(client_add_funds_info).post(client_add_funds))
        .route("/admin/settings/add-funds", get(get_add_funds).put(put_add_funds))
}

async fn view(s: &AppState, cid: Uuid) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "balances": credit::balances(&s.pool, cid).await?,
        "entries": credit::entries(&s.pool, cid, 100).await?,
    })))
}

#[utoipa::path(get, path = "/api/v1/client/credit", responses((status = 200), (status = 401)))]
pub async fn client_credit(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    view(&s, cid).await
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyBody {
    /// Defaults to as much as the balance and the invoice allow.
    #[schema(value_type = Option<String>)]
    pub amount: Option<Decimal>,
}

async fn apply(s: &AppState, cid: Uuid, id: Uuid, b: ApplyBody, actor: Uuid, ip: Option<&str>) -> ApiResult<Json<Value>> {
    let r = credit::apply_to_invoice(&s.pool, cid, id, b.amount, Some(actor)).await?;
    audit::record(&s.pool, Some(actor), "credit.applied", Some(&id.to_string()), ip, json!({ "amount": r.amount, "payment_id": r.applied.payment_id })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("credit.applied", json!({ "customer_id": cid, "invoice_id": id, "amount": r.amount, "payment_id": r.applied.payment_id, "auto": false })));
    s.emit(DomainEvent::new("payment.succeeded", json!({ "payment_id": r.applied.payment_id, "invoice_id": id, "gateway": credit::GATEWAY })));
    if r.applied.invoice_paid {
        s.emit(DomainEvent::new("invoice.paid", json!({ "invoice_id": id })));
        crate::changes::after_payment(s, id).await;
    }
    let inv = s.invoices().get(id).await?;
    Ok(Json(json!({ "amount": r.amount, "balance": r.balance, "invoice": inv })))
}

/// `invoice.created` hook: apply the customer's credit when auto-apply is on.
pub async fn auto_apply_on_event(s: &AppState, e: &DomainEvent) {
    if e.name != "invoice.created" {
        return;
    }
    let Some(id) = e.payload["invoice_id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) else { return };
    match add_funds::get_settings(&s.pool).await {
        Ok(c) if c.auto_apply_credit => {}
        _ => return,
    }
    let Ok(Some(inv)) = s.invoices().get(id).await else { return };
    // Expected refusals (no credit, deposit invoice, not payable) are silent.
    let Ok(r) = credit::apply_to_invoice(&s.pool, inv.customer_id, id, None, None).await else { return };
    let _ = audit::record(&s.pool, None, "credit.auto_applied", Some(&id.to_string()), None, json!({ "amount": r.amount, "payment_id": r.applied.payment_id })).await;
    s.emit(pelty_extensions::events::DomainEvent::new("credit.applied", json!({ "customer_id": inv.customer_id, "invoice_id": id, "amount": r.amount, "payment_id": r.applied.payment_id, "auto": true })));
    s.emit(DomainEvent::new("payment.succeeded", json!({ "payment_id": r.applied.payment_id, "invoice_id": id, "gateway": credit::GATEWAY })));
    if r.applied.invoice_paid {
        s.emit(DomainEvent::new("invoice.paid", json!({ "invoice_id": id })));
        crate::changes::after_payment(s, id).await;
    }
}

#[utoipa::path(post, path = "/api/v1/client/invoices/{id}/apply-credit", request_body = ApplyBody, responses((status = 200), (status = 404), (status = 409)))]
pub async fn client_apply(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, b: Option<Json<ApplyBody>>) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    apply(&s, cid, id, b.map(|j| j.0).unwrap_or_default(), u.user_id, u.ip.as_deref()).await
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/apply-credit", request_body = ApplyBody, responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_apply(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
    b: Option<Json<ApplyBody>>,
) -> ApiResult<Json<Value>> {
    let inv = s.invoices().get(id).await?.ok_or(pelty_core::AppError::NotFound)?;
    apply(&s, inv.customer_id, id, b.map(|j| j.0).unwrap_or_default(), u.user_id, u.ip.as_deref()).await
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/credit", responses((status = 200), (status = 403)))]
pub async fn admin_credit(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    view(&s, id).await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdjustBody {
    pub currency: String,
    /// Positive adds credit, negative removes it.
    #[schema(value_type = String)]
    pub amount: Decimal,
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/credit", request_body = AdjustBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_adjust(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<AdjustBody>,
) -> ApiResult<Json<Value>> {
    // Pre-event: addons may veto the adjustment or rewrite the note.
    let pre = crate::addon_ext::pre_event(&s, "credit.before_adjust", json!({ "customer_id": id, "amount": b.amount, "currency": b.currency, "note": b.note, "by": u.user_id })).await?;
    let note = pre.get("note").and_then(Value::as_str).filter(|n| n.len() <= 500).map(str::to_string).unwrap_or_else(|| b.note.clone());
    let b = AdjustBody { note, ..b };
    let bal = credit::adjust(&s.pool, id, &b.currency, b.amount, &b.note, Some(u.user_id)).await?;
    audit::record(&s.pool, Some(u.user_id), "credit.adjusted", Some(&id.to_string()), u.ip.as_deref(), json!({ "amount": b.amount, "currency": b.currency, "note": b.note })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("credit.adjusted", json!({ "customer_id": id, "amount": b.amount, "currency": b.currency, "balance": bal, "by": u.user_id })));
    Ok(Json(json!({ "balance": bal })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddFundsBody {
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
}

#[utoipa::path(get, path = "/api/v1/client/credit/add-funds", responses((status = 200, body = AddFundsSettings), (status = 401)))]
pub async fn client_add_funds_info(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<AddFundsSettings>> {
    customer_of(&s, &u).await?;
    Ok(Json(add_funds::get_settings(&s.pool).await?))
}

/// Create a deposit invoice; once paid (any gateway) the amount is credited.
#[utoipa::path(post, path = "/api/v1/client/credit/add-funds", request_body = AddFundsBody, responses((status = 200), (status = 403), (status = 409), (status = 422)))]
pub async fn client_add_funds(State(s): State<AppState>, u: CurrentUser, Json(b): Json<AddFundsBody>) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let locale = pelty_database::overdue::recipient(&s.pool, cid).await?.and_then(|r| r.2).filter(|l| s.i18n.has(l)).unwrap_or_else(|| s.cfg.default_locale.clone());
    let id = add_funds::create(&s.pool, cid, &b.currency, b.amount, s.i18n.t(&locale, "credit.add_funds_line")).await?;
    audit::record(&s.pool, Some(u.user_id), "credit.add_funds", Some(&id.to_string()), u.ip.as_deref(), json!({ "amount": b.amount, "currency": b.currency })).await?;
    s.emit(DomainEvent::new("invoice.created", json!({ "invoice_id": id, "customer_id": cid })));
    Ok(Json(json!({ "invoice_id": id })))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/add-funds", responses((status = 200, body = AddFundsSettings), (status = 403)))]
pub async fn get_add_funds(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<AddFundsSettings>> {
    Ok(Json(add_funds::get_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/add-funds", request_body = AddFundsSettings, responses((status = 200, body = AddFundsSettings), (status = 422)))]
pub async fn put_add_funds(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<AddFundsSettings>,
) -> ApiResult<Json<AddFundsSettings>> {
    let b = b.normalized().map_err(pelty_core::AppError::Validation)?;
    add_funds::put_settings(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.add_funds", None, u.ip.as_deref(), json!(b)).await?;
    Ok(Json(b))
}
