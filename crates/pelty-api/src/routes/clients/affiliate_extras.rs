//! Affiliate extras: per-product rates, tiers, promotional materials.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::affiliate_extras::{self as db, MaterialInput, ProductRate, TierInput};
use pelty_database::{affiliates, audit};
use serde_json::{json, Value};
use uuid::Uuid;

type Perm = RequirePermission<perms::AffiliatesManage>;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/affiliates/product-rates",
            get(rates_list).put(rates_set),
        )
        .route(
            "/admin/affiliates/product-rates/{product_id}",
            delete(rates_delete),
        )
        .route(
            "/admin/affiliates/tiers",
            get(tiers_list).post(tiers_create),
        )
        .route("/admin/affiliates/tiers/{id}", delete(tiers_delete))
        .route(
            "/admin/affiliates/materials",
            get(materials_list).post(materials_create),
        )
        .route(
            "/admin/affiliates/materials/{id}",
            put(materials_update).delete(materials_delete),
        )
        .route("/client/affiliate/materials", get(client_materials))
}

#[utoipa::path(get, path = "/api/v1/admin/affiliates/product-rates", responses((status = 200)))]
pub async fn rates_list(State(s): State<AppState>, _p: Perm) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": db::rates(&s.pool).await? })))
}

#[utoipa::path(put, path = "/api/v1/admin/affiliates/product-rates", request_body = ProductRate, responses((status = 204), (status = 422)))]
pub async fn rates_set(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Json(b): Json<ProductRate>,
) -> ApiResult<StatusCode> {
    db::set_rate(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.product_rate_set",
        Some(&b.product_id.to_string()),
        u.ip.as_deref(),
        json!({ "kind": b.kind, "value": b.value }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/admin/affiliates/product-rates/{product_id}", responses((status = 204), (status = 404)))]
pub async fn rates_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(pid): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !db::delete_rate(&s.pool, pid).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.product_rate_deleted",
        Some(&pid.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/affiliates/tiers", responses((status = 200)))]
pub async fn tiers_list(State(s): State<AppState>, _p: Perm) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": db::tiers(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/affiliates/tiers", request_body = TierInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn tiers_create(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Json(b): Json<TierInput>,
) -> ApiResult<Json<db::Tier>> {
    let t = db::create_tier(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.tier_created",
        Some(&t.id.to_string()),
        u.ip.as_deref(),
        json!({ "min_referrals": t.min_referrals }),
    )
    .await?;
    Ok(Json(t))
}

#[utoipa::path(delete, path = "/api/v1/admin/affiliates/tiers/{id}", responses((status = 204), (status = 404)))]
pub async fn tiers_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !db::delete_tier(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.tier_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/affiliates/materials", responses((status = 200)))]
pub async fn materials_list(State(s): State<AppState>, _p: Perm) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": db::materials(&s.pool, false).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/affiliates/materials", request_body = MaterialInput, responses((status = 200), (status = 422)))]
pub async fn materials_create(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Json(b): Json<MaterialInput>,
) -> ApiResult<Json<db::Material>> {
    let m = db::save_material(&s.pool, None, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.material_saved",
        Some(&m.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(put, path = "/api/v1/admin/affiliates/materials/{id}", request_body = MaterialInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn materials_update(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(id): Path<Uuid>,
    Json(b): Json<MaterialInput>,
) -> ApiResult<Json<db::Material>> {
    let m = db::save_material(&s.pool, Some(id), &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.material_saved",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(delete, path = "/api/v1/admin/affiliates/materials/{id}", responses((status = 204), (status = 404)))]
pub async fn materials_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !db::delete_material(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.material_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Active materials plus the caller's effective commission percent and tiers.
#[utoipa::path(get, path = "/api/v1/client/affiliate/materials", responses((status = 200), (status = 404)))]
pub async fn client_materials(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let a = affiliates::for_user(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(json!({
        "code": a.code,
        "effective_percent": db::effective_percent(&s.pool, a.id).await?,
        "tiers": db::tiers(&s.pool).await?,
        "items": db::materials(&s.pool, true).await?,
    })))
}
