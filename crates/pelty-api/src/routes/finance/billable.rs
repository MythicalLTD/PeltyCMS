//! Billable items, one-off draft invoice lines, and the per-product
//! "generate invoice N days before due" setting.
use crate::routes::billing::{ItemBody, DUE_DAYS};
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::billable::{self as bi, BillableInput};
use pelty_database::invoices::NewItem;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/billable-items", get(list).post(create))
        .route("/admin/billable-items/{id}", delete(remove))
        .route("/admin/customers/{id}/billable-items/invoice", post(invoice_pending))
        .route("/admin/invoices/{id}/items", post(add_lines))
        .route("/admin/catalog/products/{id}/billing", get(product_billing).put(set_product_billing))
        .route("/admin/catalog/products/{id}/fixed-term", get(product_fixed_term).put(set_product_fixed_term))
        .route("/admin/catalog/products/{id}/stock", axum::routing::put(set_product_stock))
}

type Im = RequirePermission<perms::InvoicesManage>;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub customer_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/admin/billable-items", params(ListQuery), responses((status = 200)))]
pub async fn list(State(s): State<AppState>, _p: Im, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": bi::list(&s.pool, q.customer_id, 200).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/billable-items", request_body = BillableInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn create(State(s): State<AppState>, RequirePermission(u, _): Im, Json(b): Json<BillableInput>) -> ApiResult<Json<Value>> {
    let (id, inv) = bi::create(&s.pool, u.user_id, &b, DUE_DAYS).await?;
    audit::record(&s.pool, Some(u.user_id), "billable_item.created", Some(&id.to_string()), u.ip.as_deref(), json!({ "customer_id": b.customer_id, "action": b.invoice_action, "invoice_id": inv })).await?;
    Ok(Json(json!({ "id": id, "invoice_id": inv })))
}

#[utoipa::path(delete, path = "/api/v1/admin/billable-items/{id}", responses((status = 200), (status = 404)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): Im, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    bi::delete(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "billable_item.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/billable-items/invoice", responses((status = 200), (status = 404)))]
pub async fn invoice_pending(State(s): State<AppState>, RequirePermission(u, _): Im, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let inv = bi::invoice_pending(&s.pool, id, DUE_DAYS).await?;
    audit::record(&s.pool, Some(u.user_id), "billable_item.invoiced", Some(&id.to_string()), u.ip.as_deref(), json!({ "invoice_id": inv })).await?;
    Ok(Json(json!({ "invoice_id": inv })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LinesBody {
    pub items: Vec<ItemBody>,
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/items", request_body = LinesBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn add_lines(State(s): State<AppState>, RequirePermission(u, _): Im, Path(id): Path<Uuid>, Json(b): Json<LinesBody>) -> ApiResult<Json<Value>> {
    let lines: Vec<NewItem> = b
        .items
        .iter()
        .map(|i| NewItem {
            description: i.description.clone(),
            quantity: i.quantity,
            unit_price: i.unit_price,
            tax_rate: i.tax_rate.unwrap_or_default(),
            service_id: None,
            period: None,
            auto_tax: i.tax_rate.is_none(),
        })
        .collect();
    bi::add_draft_lines(&s.pool, id, &lines).await?;
    audit::record(&s.pool, Some(u.user_id), "invoice.items_added", Some(&id.to_string()), u.ip.as_deref(), json!({ "count": lines.len() })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ProductBilling {
    /// Days before the due date to generate the renewal invoice; null = default.
    pub invoice_days_before: Option<i32>,
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/products/{id}/billing", responses((status = 200), (status = 404)))]
pub async fn product_billing(State(s): State<AppState>, _p: RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "invoice_days_before": bi::product_days(&s.pool, id).await?, "default_days": crate::worker::RENEW_AHEAD_DAYS })))
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/products/{id}/billing", request_body = ProductBilling, responses((status = 200), (status = 404), (status = 422)))]
pub async fn set_product_billing(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>, Json(b): Json<ProductBilling>) -> ApiResult<Json<Value>> {
    bi::set_product_days(&s.pool, id, b.invoice_days_before).await?;
    audit::record(&s.pool, Some(u.user_id), "product.billing_updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "invoice_days_before": b.invoice_days_before })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FixedTerm {
    /// Terminate services this many days after activation; null = no fixed term.
    pub auto_terminate_days: Option<i32>,
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/products/{id}/fixed-term", responses((status = 200), (status = 404)))]
pub async fn product_fixed_term(State(s): State<AppState>, _p: RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "auto_terminate_days": bi::product_fixed_term(&s.pool, id).await? })))
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/products/{id}/fixed-term", request_body = FixedTerm, responses((status = 200), (status = 404), (status = 422)))]
pub async fn set_product_fixed_term(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>, Json(b): Json<FixedTerm>) -> ApiResult<Json<Value>> {
    bi::set_product_fixed_term(&s.pool, id, b.auto_terminate_days).await?;
    audit::record(&s.pool, Some(u.user_id), "product.fixed_term_updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "auto_terminate_days": b.auto_terminate_days })).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Terminate services whose fixed term elapsed: transition, provisioning hook, event.
pub async fn run_fixed_term(s: &AppState) -> usize {
    let Ok(ids) = bi::fixed_term_due(&s.pool, 200).await else { return 0 };
    let mut n = 0;
    for id in ids {
        let Ok((from, svc)) = s.hosting().transition(id, "terminate", "fixed_term").await else { continue };
        let _ = audit::record(&s.pool, None, "service.transition", Some(&id.to_string()), None, json!({ "from": from, "to": svc.status, "reason": "fixed_term" })).await;
        crate::provision::on_transition(s, id, &from, &svc.status).await;
        s.emit(pelty_extensions::events::DomainEvent::new("service.terminated", json!({ "service_id": id, "from": from, "reason": "fixed_term", "provisioned": true })));
        n += 1;
    }
    n
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ProductStock {
    pub stock_enabled: bool,
    pub stock_qty: i32,
    pub allow_backorder: bool,
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/products/{id}/stock", request_body = ProductStock, responses((status = 200), (status = 404), (status = 422)))]
pub async fn set_product_stock(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>, Json(b): Json<ProductStock>) -> ApiResult<Json<Value>> {
    pelty_database::catalog::set_stock(&s.pool, id, b.stock_enabled, b.stock_qty, b.allow_backorder).await?;
    audit::record(&s.pool, Some(u.user_id), "product.stock_updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "enabled": b.stock_enabled, "qty": b.stock_qty, "backorder": b.allow_backorder })).await?;
    Ok(Json(json!({ "ok": true })))
}
