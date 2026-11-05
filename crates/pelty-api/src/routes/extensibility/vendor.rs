//! Vendor marketplace (MarketConnect-style resale): sync a vendor server's
//! catalog through its provisioning module, import items as products with
//! a markup, and re-price them. Ordering/provisioning use the normal flow.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, provisioning as pdb, vendor};
use pelty_provisioning::ServerConn;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/servers/{id}/vendor-catalog/sync", post(sync))
        .route("/admin/vendor-catalog", get(list))
        .route("/admin/vendor-catalog/{id}/import", post(import))
        .route("/admin/vendor-catalog/{id}/markup", put(reprice))
}

/// Pull the vendor catalog through the server's module.
#[utoipa::path(post, path = "/api/v1/admin/servers/{id}/vendor-catalog/sync", responses((status = 200), (status = 409)))]
pub async fn sync(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let (module, base_url, sealed, config, _) = pdb::server_secret(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let m = s
        .provisioning
        .get(&module)
        .ok_or(AppError::Validation("error.unknown_module"))?;
    let b = s
        .secret_box
        .as_ref()
        .ok_or(AppError::Conflict("error.encryption_not_configured"))?;
    let api_key = b
        .open(&sealed)
        .and_then(|k| String::from_utf8(k).ok())
        .ok_or(AppError::Conflict("error.server_config"))?;
    let conn = ServerConn {
        base_url,
        api_key,
        config,
    };
    let items =
        match tokio::time::timeout(std::time::Duration::from_secs(30), m.catalog(&conn)).await {
            Ok(Ok(Some(c))) => c,
            Ok(Ok(None)) => return Err(AppError::Conflict("error.module_no_catalog").into()),
            Ok(Err(e)) => {
                tracing::info!(server = %id, error = %e, "vendor catalog failed");
                return Err(AppError::Conflict("error.module_unavailable").into());
            }
            Err(_) => return Err(AppError::Conflict("error.module_unavailable").into()),
        };
    let rows: Vec<vendor::SyncItem> = items
        .into_iter()
        .filter_map(|p| {
            Some(vendor::SyncItem {
                cost: p.cost.parse::<Decimal>().ok()?,
                sku: p.sku,
                name: p.name,
                description: p.description,
                currency: p.currency.to_ascii_uppercase(),
                cycle: p.cycle,
                config: p.product_config,
            })
        })
        .collect();
    let (n, retired) = vendor::sync(&s.pool, id, &rows).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vendor.catalog_synced",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "items": n, "retired": retired }),
    )
    .await?;
    Ok(Json(json!({ "synced": n, "retired": retired })))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ListQ {
    pub server_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/admin/vendor-catalog", params(ListQ), responses((status = 200)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Query(q): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": vendor::list(&s.pool, q.server_id).await? }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ImportBody {
    pub group_id: Uuid,
    #[schema(value_type = String)]
    pub markup_percent: Decimal,
    pub name: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/vendor-catalog/{id}/import", request_body = ImportBody, responses((status = 200), (status = 409)))]
pub async fn import(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ImportBody>,
) -> ApiResult<Json<Value>> {
    let pid = vendor::import(&s.pool, id, b.group_id, b.markup_percent, b.name.as_deref()).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vendor.product_imported",
        Some(&pid.to_string()),
        u.ip.as_deref(),
        json!({ "catalog_id": id, "markup_percent": b.markup_percent.to_string() }),
    )
    .await?;
    Ok(Json(json!({ "product_id": pid })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MarkupBody {
    #[schema(value_type = String)]
    pub markup_percent: Decimal,
}

#[utoipa::path(put, path = "/api/v1/admin/vendor-catalog/{id}/markup", request_body = MarkupBody, responses((status = 200), (status = 409)))]
pub async fn reprice(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<MarkupBody>,
) -> ApiResult<Json<Value>> {
    let price = vendor::reprice(&s.pool, id, b.markup_percent).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vendor.repriced",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "markup_percent": b.markup_percent.to_string(), "price": price.to_string() }),
    )
    .await?;
    Ok(Json(json!({ "price": price.to_string() })))
}
