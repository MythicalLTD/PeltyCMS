//! Order form templates (admin CRUD, per-group template) and product
//! free-domain rules; the public form lookup for the client order page.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::order_forms::{self as db, FormInput, FreeDomain};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/order-forms", get(admin_list).post(admin_create))
        .route(
            "/admin/order-forms/{id}",
            put(admin_update).delete(admin_delete),
        )
        .route(
            "/admin/product-groups/{id}/order-form",
            put(admin_set_group),
        )
        .route(
            "/admin/products/{id}/free-domain",
            get(admin_free_get).put(admin_free_put),
        )
        .route("/store/order-forms/{slug}", get(public_form))
        .route("/store/groups/{id}/order-form", get(public_group_form))
}

#[utoipa::path(get, path = "/api/v1/admin/order-forms", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    let groups = db::group_forms(&s.pool).await?;
    let groups: Vec<Value> = groups
        .into_iter()
        .map(|(id, name, f)| json!({ "id": id, "name": name, "order_form_id": f }))
        .collect();
    Ok(Json(
        json!({ "items": db::list(&s.pool).await?, "groups": groups, "layouts": db::LAYOUTS, "steps": db::STEPS, "field_kinds": db::FIELD_KINDS }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/order-forms", request_body = FormInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<FormInput>,
) -> ApiResult<Json<db::OrderForm>> {
    let f = db::save(&s.pool, None, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order_form.created",
        Some(&f.id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": f.slug }),
    )
    .await?;
    Ok(Json(f))
}

#[utoipa::path(put, path = "/api/v1/admin/order-forms/{id}", request_body = FormInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<FormInput>,
) -> ApiResult<Json<db::OrderForm>> {
    let f = db::save(&s.pool, Some(id), &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order_form.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": f.slug }),
    )
    .await?;
    Ok(Json(f))
}

#[utoipa::path(delete, path = "/api/v1/admin/order-forms/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order_form.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupForm {
    pub order_form_id: Option<Uuid>,
}

#[utoipa::path(put, path = "/api/v1/admin/product-groups/{id}/order-form", request_body = GroupForm, responses((status = 200), (status = 404)))]
pub async fn admin_set_group(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<GroupForm>,
) -> ApiResult<Json<Value>> {
    if let Some(f) = b.order_form_id {
        db::get(&s.pool, f).await?.ok_or(AppError::NotFound)?;
    }
    db::set_group_form(&s.pool, id, b.order_form_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "product_group.order_form",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "order_form_id": b.order_form_id }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/products/{id}/free-domain", responses((status = 200), (status = 404)))]
pub async fn admin_free_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FreeDomain>> {
    Ok(Json(
        db::free_domain(&s.pool, id)
            .await?
            .ok_or(AppError::NotFound)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/products/{id}/free-domain", request_body = FreeDomain, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_free_put(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<FreeDomain>,
) -> ApiResult<Json<FreeDomain>> {
    let r = db::set_free_domain(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "product.free_domain",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!(r),
    )
    .await?;
    Ok(Json(r))
}

async fn render(s: &AppState, f: db::OrderForm) -> ApiResult<Json<Value>> {
    let products = db::products(&s.pool, &f).await?;
    Ok(Json(json!({ "form": f, "products": products })))
}

#[utoipa::path(get, path = "/api/v1/store/order-forms/{slug}", responses((status = 200), (status = 404)))]
pub async fn public_form(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let f = db::by_slug(&s.pool, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    render(&s, f).await
}

#[utoipa::path(get, path = "/api/v1/store/groups/{id}/order-form", responses((status = 200), (status = 404)))]
pub async fn public_group_form(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let f = db::for_group(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    render(&s, f).await
}
