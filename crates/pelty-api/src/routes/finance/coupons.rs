//! Coupons: admin management (`coupons.manage`) and a client-side code check.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::coupons::{self, CouponInput};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/coupons", get(admin_list).post(admin_create))
        .route(
            "/admin/coupons/{id}",
            put(admin_update).delete(admin_delete),
        )
        .route("/client/coupons/check", post(client_check))
}

#[utoipa::path(get, path = "/api/v1/admin/coupons", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CouponsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": coupons::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/coupons", request_body = CouponInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CouponsManage>,
    Json(b): Json<CouponInput>,
) -> ApiResult<Json<Value>> {
    let c = coupons::save(&s.pool, None, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "coupon.created",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "code": c.code }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "coupon.created",
        json!({ "coupon_id": c.id, "code": c.code }),
    ));
    Ok(Json(json!(c)))
}

#[utoipa::path(put, path = "/api/v1/admin/coupons/{id}", request_body = CouponInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CouponsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CouponInput>,
) -> ApiResult<Json<Value>> {
    let c = coupons::save(&s.pool, Some(id), &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "coupon.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "code": c.code }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "coupon.updated",
        json!({ "coupon_id": id, "code": c.code }),
    ));
    Ok(Json(json!(c)))
}

#[utoipa::path(delete, path = "/api/v1/admin/coupons/{id}", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CouponsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    coupons::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "coupon.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "coupon.deleted",
        json!({ "coupon_id": id }),
    ));
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckBody {
    pub code: String,
    pub currency: String,
}

/// Validate a code for the order form. Returns only what the customer needs.
#[utoipa::path(post, path = "/api/v1/client/coupons/check", request_body = CheckBody, responses((status = 200), (status = 422)))]
pub async fn client_check(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<CheckBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    // Same limiter as auth: stops brute-forcing promo codes.
    if !s.auth_limiter.check(&format!("coupon:{cid}")) {
        return Err(pelty_core::AppError::RateLimited.into());
    }
    let c = coupons::check(&s.pool, &b.code, cid, &b.currency).await?;
    Ok(Json(
        json!({ "code": c.code, "kind": c.kind, "value": c.value, "currency": c.currency, "recurring": c.recurring, "product_ids": c.product_ids }),
    ))
}
