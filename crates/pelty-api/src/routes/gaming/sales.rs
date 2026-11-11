//! Timed sales (module `timed_sales`): admin CRUD, the public list of running
//! sales for the store, and the checkout discounts.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::orders::{AddonDiscount, CartItem};
use pelty_database::timed_sales::{self as ts, Sale, SaleInput};
use pelty_database::{audit, feature_modules};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/sales", get(public_list))
        .route("/admin/sales", get(admin_list).post(admin_create))
        .route("/admin/sales/{id}", put(admin_update).delete(admin_delete))
}

/// First-invoice discounts from running sales for these cart lines, or none
/// while the module is off. `place_full` re-quotes and clamps them.
pub async fn checkout_discounts(
    s: &AppState,
    customer: Uuid,
    currency: &str,
    items: &[CartItem],
) -> ApiResult<Vec<AddonDiscount>> {
    if items.is_empty() || !feature_modules::is_enabled(&s.pool, ts::MODULE).await? {
        return Ok(vec![]);
    }
    let sales = ts::active(&s.pool).await?;
    if sales.is_empty() {
        return Ok(vec![]);
    }
    let Ok(quotes) = s
        .orders()
        .line_amounts(Some(customer), currency, items)
        .await
    else {
        // Invalid carts fail later with the precise error.
        return Ok(vec![]);
    };
    let ids: Vec<Uuid> = quotes.iter().map(|q| q.product_id).collect();
    let groups: HashMap<Uuid, Uuid> = ts::groups(&s.pool, &ids).await?.into_iter().collect();
    let mut out = vec![];
    for (item, q) in quotes.iter().enumerate() {
        let Some(group) = groups.get(&q.product_id) else {
            continue;
        };
        if let Some(sale) = ts::best(&sales, q.product_id, *group) {
            let amount = q.amount * sale.percent / Decimal::from(100);
            if amount > Decimal::ZERO {
                out.push(AddonDiscount {
                    item,
                    amount,
                    label: format!("Sale: {} (-{}%)", sale.name, sale.percent.normalize()),
                });
            }
        }
    }
    Ok(out)
}

/// Running sales (for strike-through prices and a countdown on the store).
#[utoipa::path(get, path = "/api/v1/public/sales", responses((status = 200), (status = 404)))]
pub async fn public_list(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = ts::active(&s.pool)
        .await?
        .into_iter()
        .map(|x| json!({
            "id": x.id, "name": x.name, "percent": x.percent.normalize().to_string(),
            "product_ids": x.product_ids, "group_ids": x.group_ids,
            "ends_at": x.ends_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        }))
        .collect();
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(get, path = "/api/v1/admin/sales", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": ts::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/sales", request_body = SaleInput, responses((status = 200, body = Sale), (status = 403), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(i): Json<SaleInput>,
) -> ApiResult<Json<Sale>> {
    let sale = ts::create(&s.pool, &i).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "sale.created",
        Some(&sale.id.to_string()),
        u.ip.as_deref(),
        json!({ "name": sale.name, "percent": sale.percent.to_string() }),
    )
    .await?;
    Ok(Json(sale))
}

#[utoipa::path(put, path = "/api/v1/admin/sales/{id}", request_body = SaleInput, params(("id" = Uuid, Path)), responses((status = 200, body = Sale), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(i): Json<SaleInput>,
) -> ApiResult<Json<Sale>> {
    let sale = ts::update(&s.pool, id, &i).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "sale.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": sale.name, "percent": sale.percent.to_string(), "enabled": sale.enabled }),
    )
    .await?;
    Ok(Json(sale))
}

#[utoipa::path(delete, path = "/api/v1/admin/sales/{id}", params(("id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    if !ts::delete(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "sale.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
