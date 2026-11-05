//! Admin tax configuration: global settings and rules per country/region.
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{
    audit,
    tax::{self, RuleInput, TaxRule, TaxSettings},
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::ApiResult,
    extract::{perms, RequirePermission},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/settings/tax", get(get_settings).put(put_settings))
        .route("/admin/tax-rules", get(list_rules).post(create_rule))
        .route(
            "/admin/tax-rules/{id}",
            put(update_rule).delete(delete_rule),
        )
        .route("/admin/customers/{id}/vat/verify", post(admin_verify_vat))
        .route("/client/vat/verify", post(client_verify_vat))
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/vat/verify", responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_verify_vat(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let r = crate::vies::verify_customer(&s, id).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.vat_verified", Some(&id.to_string()), u.ip.as_deref(), json!({ "valid": r.valid })).await?;
    Ok(Json(json!(r)))
}

#[utoipa::path(post, path = "/api/v1/client/vat/verify", responses((status = 200), (status = 409), (status = 422)))]
pub async fn client_verify_vat(State(s): State<AppState>, u: crate::extract::CurrentUser) -> ApiResult<Json<Value>> {
    let cid = crate::routes::customers::customer_of(&s, &u).await?;
    let r = crate::vies::verify_customer(&s, cid).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.vat_verified", Some(&cid.to_string()), u.ip.as_deref(), json!({ "valid": r.valid })).await?;
    Ok(Json(json!(r)))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/tax", responses((status = 200, body = TaxSettings), (status = 403)))]
pub async fn get_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<TaxSettings>> {
    Ok(Json(tax::get_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/tax", request_body = TaxSettings, responses((status = 200, body = TaxSettings), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<TaxSettings>,
) -> ApiResult<Json<TaxSettings>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    tax::put_settings(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.tax",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(b))
}

#[utoipa::path(get, path = "/api/v1/admin/tax-rules", responses((status = 200), (status = 403)))]
pub async fn list_rules(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": tax::list_rules(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/tax-rules", request_body = RuleInput, responses((status = 200, body = TaxRule), (status = 422)))]
pub async fn create_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<RuleInput>,
) -> ApiResult<Json<TaxRule>> {
    let r = tax::create_rule(&s.pool, b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tax_rule.created",
        Some(&r.id.to_string()),
        u.ip.as_deref(),
        json!({ "rate": r.rate.to_string() }),
    )
    .await?;
    Ok(Json(r))
}

#[utoipa::path(put, path = "/api/v1/admin/tax-rules/{id}", request_body = RuleInput, responses((status = 200, body = TaxRule), (status = 404), (status = 422)))]
pub async fn update_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RuleInput>,
) -> ApiResult<Json<TaxRule>> {
    let r = tax::update_rule(&s.pool, id, b)
        .await?
        .ok_or(AppError::NotFound)?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tax_rule.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(r))
}

#[utoipa::path(delete, path = "/api/v1/admin/tax-rules/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    if !tax::delete_rule(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tax_rule.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
