//! Bundles and upsell / cross-sell: public catalog reads, admin management
//! (`catalog.manage`), and the order-time bundle check.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::bundles::{self, BundleInput, Recommendations};
use pelty_database::orders::{BundleDiscount, CartItem};
use pelty_database::audit;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/catalog/bundles", get(public_bundles))
        .route("/catalog/products/{id}/recommendations", get(public_recs))
        .route("/admin/bundles", get(admin_list).post(admin_create))
        .route("/admin/bundles/{id}", put(admin_update).delete(admin_delete))
        .route("/admin/catalog/products/{id}/recommendations", get(admin_recs).put(admin_set_recs))
}

/// Discount for an order claiming `bundle`; 422 unless the cart has it all.
pub async fn discount_for(s: &AppState, bundle: Uuid, cart: &[CartItem]) -> ApiResult<BundleDiscount> {
    let b = bundles::get(&s.pool, bundle, true).await?.ok_or(AppError::Validation("error.bundle_unavailable"))?;
    let items = bundles::match_cart(&b, cart).ok_or(AppError::Validation("error.bundle_incomplete"))?;
    Ok(BundleDiscount { bundle_id: b.id, name: b.name, percent: b.discount_percent, items })
}

#[utoipa::path(get, path = "/api/v1/catalog/bundles", responses((status = 200)))]
pub async fn public_bundles(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": bundles::list(&s.pool, true).await? })))
}

#[utoipa::path(get, path = "/api/v1/catalog/products/{id}/recommendations", responses((status = 200)))]
pub async fn public_recs(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Json<Recommendations>> {
    Ok(Json(bundles::recommendations(&s.pool, id, true).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/bundles", responses((status = 200), (status = 403)))]
pub async fn admin_list(State(s): State<AppState>, _p: RequirePermission<perms::CatalogManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": bundles::list(&s.pool, false).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/bundles", request_body = BundleInput, responses((status = 200), (status = 422)))]
pub async fn admin_create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Json(b): Json<BundleInput>) -> ApiResult<Json<Value>> {
    let id = bundles::create(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "bundle.created", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name })).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(put, path = "/api/v1/admin/bundles/{id}", request_body = BundleInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>, Json(b): Json<BundleInput>) -> ApiResult<Json<Value>> {
    bundles::update(&s.pool, id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "bundle.updated", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(delete, path = "/api/v1/admin/bundles/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_delete(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    bundles::delete(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "bundle.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/products/{id}/recommendations", responses((status = 200)))]
pub async fn admin_recs(State(s): State<AppState>, _p: RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Recommendations>> {
    Ok(Json(bundles::recommendations(&s.pool, id, false).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/products/{id}/recommendations", request_body = Recommendations, responses((status = 200), (status = 422)))]
pub async fn admin_set_recs(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CatalogManage>, Path(id): Path<Uuid>, Json(r): Json<Recommendations>) -> ApiResult<Json<Recommendations>> {
    bundles::set_recommendations(&s.pool, id, &r).await?;
    audit::record(&s.pool, Some(u.user_id), "product.recommendations_set", Some(&id.to_string()), u.ip.as_deref(), json!(r)).await?;
    Ok(Json(bundles::recommendations(&s.pool, id, false).await?))
}
