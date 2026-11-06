//! Catalog: public listing and admin management (`catalog.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::addons::{self, AddonInput, FieldInput};
use pelty_database::catalog::{self, OptionInput, Price, ProductInput};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/catalog", get(public_catalog))
        .route("/currencies", get(list_currencies))
        .route(
            "/admin/catalog/groups",
            get(admin_groups).post(create_group),
        )
        .route(
            "/admin/catalog/products",
            get(admin_products).post(create_product),
        )
        .route("/admin/catalog/products/{id}", patch(update_product))
        .route("/admin/catalog/products/{id}/prices", put(set_prices))
        .route("/admin/catalog/products/{id}/options", post(add_option))
        .route(
            "/admin/catalog/products/{id}/options/{option}",
            axum::routing::delete(delete_option),
        )
        .route("/catalog/products/{id}/extras", get(product_extras))
        .route(
            "/catalog/products/{id}/checkout-config",
            post(checkout_config),
        )
        .route(
            "/admin/catalog/addons",
            get(admin_addons).post(create_addon),
        )
        .route("/admin/catalog/addons/{id}", put(update_addon))
        .route(
            "/admin/catalog/products/{id}/fields",
            get(admin_fields).post(create_field),
        )
        .route(
            "/admin/catalog/products/{id}/fields/{field}",
            put(update_field).delete(delete_field),
        )
}

#[utoipa::path(get, path = "/api/v1/catalog", responses((status = 200)))]
pub async fn public_catalog(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let groups: Vec<_> = catalog::groups(&s.pool)
        .await?
        .into_iter()
        .filter(|g| !g.hidden)
        .collect();
    let mut products = catalog::products(&s.pool, true, None).await?;
    for p in &mut products {
        p.product.module_config = json!({}); // provisioning config is internal
    }
    let v = json!({ "groups": groups, "products": products });
    if !crate::addon_filters::has(&s, "catalog.public") {
        return Ok(Json(v));
    }
    Ok(Json(
        crate::addon_filters::apply(&s, "catalog.public", v, json!({})).await,
    ))
}

#[utoipa::path(get, path = "/api/v1/currencies", responses((status = 200)))]
pub async fn list_currencies(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let c: Vec<_> = pelty_database::currencies::list(&s.pool)
        .await?
        .into_iter()
        .filter(|c| c.enabled)
        .map(|c| json!({ "code": c.code, "exponent": c.exponent, "rate": c.rate.to_string(), "is_default": c.is_default, "prefix": c.prefix, "suffix": c.suffix }))
        .collect();
    Ok(Json(json!({ "items": c })))
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/groups", responses((status = 200), (status = 403)))]
pub async fn admin_groups(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": catalog::groups(&s.pool).await? })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupBody {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub hidden: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/catalog/groups", request_body = GroupBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create_group(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<GroupBody>,
) -> ApiResult<Json<Value>> {
    let g = catalog::create_group(&s.pool, &b.name, &b.slug, b.sort_order, b.hidden).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.group_created",
        Some(&g.id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": g.slug }),
    )
    .await?;
    Ok(Json(json!(g)))
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/products", responses((status = 200), (status = 403)))]
pub async fn admin_products(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": catalog::products(&s.pool, false, None).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/catalog/products", request_body = ProductInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create_product(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<ProductInput>,
) -> ApiResult<Json<Value>> {
    let p = catalog::create_product(&s.pool, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.product_created",
        Some(&p.id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": p.slug }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "catalog.product_created",
        json!({ "product_id": p.id, "slug": p.slug }),
    ));
    Ok(Json(json!(p)))
}

#[utoipa::path(patch, path = "/api/v1/admin/catalog/products/{id}", request_body = ProductInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_product(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ProductInput>,
) -> ApiResult<Json<Value>> {
    let p = catalog::update_product(&s.pool, id, &b)
        .await?
        .ok_or(AppError::NotFound)?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.product_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "catalog.product_updated",
        json!({ "product_id": id }),
    ));
    Ok(Json(json!(p)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PricesBody {
    pub prices: Vec<Price>,
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/products/{id}/prices", request_body = PricesBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn set_prices(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<PricesBody>,
) -> ApiResult<Json<Value>> {
    if b.prices.len() > 200 {
        return Err(AppError::Validation("error.validation").into());
    }
    catalog::set_prices(&s.pool, id, &b.prices).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.prices_set",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "count": b.prices.len() }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "catalog.prices_changed",
        json!({ "product_id": id, "count": b.prices.len() }),
    ));
    let p = catalog::products(&s.pool, false, Some(id)).await?;
    Ok(Json(json!(p.first())))
}

#[utoipa::path(post, path = "/api/v1/admin/catalog/products/{id}/options", request_body = OptionInput, responses((status = 200), (status = 422)))]
pub async fn add_option(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<OptionInput>,
) -> ApiResult<Json<Value>> {
    let oid = catalog::add_option(&s.pool, id, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.option_added",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "option": oid }),
    )
    .await?;
    Ok(Json(json!({ "id": oid })))
}

/// Addons and customer-visible custom fields offered for one product.
#[utoipa::path(get, path = "/api/v1/catalog/products/{id}/extras", responses((status = 200)))]
pub async fn product_extras(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "addons": addons::list(&s.pool, Some(id), true).await?, "fields": addons::fields(&s.pool, id, true).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckoutConfigBody {
    /// Current answers keyed by custom-field id (UUID string) or module key.
    #[serde(default)]
    pub selected: Value,
}

/// Module-driven checkout form for the current selection (filters / reshapes
/// module fields via [`ProvisioningModule::checkout_config`]). Non-module
/// product fields are always included.
#[utoipa::path(post, path = "/api/v1/catalog/products/{id}/checkout-config", request_body = CheckoutConfigBody, responses((status = 200), (status = 404)))]
pub async fn checkout_config(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<CheckoutConfigBody>,
) -> ApiResult<Json<Value>> {
    let fields = addons::fields(&s.pool, id, true).await?;
    let meta: Option<(Option<String>, Value)> = sqlx::query_as(
        "SELECT module, module_config FROM products WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .fetch_optional(&s.pool)
    .await
    .map_err(pelty_database::db_err)?;
    let Some((module, product_config)) = meta else {
        return Err(AppError::NotFound.into());
    };
    // Map field id → module_key for resolving selected answers.
    let keys: Vec<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT id, module_key FROM product_custom_fields WHERE product_id = $1")
            .bind(id)
            .fetch_all(&s.pool)
            .await
            .map_err(pelty_database::db_err)?;
    let id_to_key: std::collections::HashMap<Uuid, String> = keys
        .iter()
        .filter_map(|(fid, k)| k.as_ref().map(|k| (*fid, k.clone())))
        .collect();
    let mut selected_by_key = serde_json::Map::new();
    if let Some(obj) = b.selected.as_object() {
        for (k, v) in obj {
            if let Ok(uid) = Uuid::parse_str(k) {
                if let Some(mk) = id_to_key.get(&uid) {
                    selected_by_key.insert(mk.clone(), v.clone());
                    continue;
                }
            }
            selected_by_key.insert(k.clone(), v.clone());
        }
    }
    let selected = Value::Object(selected_by_key);
    let allowed: Option<std::collections::HashSet<String>> = module
        .as_deref()
        .and_then(|m| s.provisioning.get(m))
        .map(|m| {
            m.checkout_config(&product_config, &selected)
                .into_iter()
                .map(|f| f.key)
                .collect()
        });
    let visible: Vec<_> = fields
        .into_iter()
        .filter(|f| match &allowed {
            None => true, // no module → keep all admin fields
            Some(keys) => {
                // Non-module fields (no key mapping) always show; module ones filter.
                match id_to_key.get(&f.id) {
                    Some(mk) => keys.contains(mk),
                    None => true,
                }
            }
        })
        .collect();
    Ok(Json(json!({ "fields": visible, "selected": selected })))
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/addons", responses((status = 200), (status = 403)))]
pub async fn admin_addons(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": addons::list(&s.pool, None, false).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/catalog/addons", request_body = AddonInput, responses((status = 200), (status = 422)))]
pub async fn create_addon(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<AddonInput>,
) -> ApiResult<Json<Value>> {
    let a = addons::upsert(&s.pool, None, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.addon_saved",
        Some(&a.addon.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(a)))
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/addons/{id}", request_body = AddonInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_addon(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<AddonInput>,
) -> ApiResult<Json<Value>> {
    let a = addons::upsert(&s.pool, Some(id), &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.addon_saved",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(a)))
}

#[utoipa::path(get, path = "/api/v1/admin/catalog/products/{id}/fields", responses((status = 200), (status = 403)))]
pub async fn admin_fields(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": addons::fields(&s.pool, id, false).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/catalog/products/{id}/fields", request_body = FieldInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn create_field(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<FieldInput>,
) -> ApiResult<Json<Value>> {
    let f = addons::upsert_field(&s.pool, id, None, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.field_saved",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "field": f.id }),
    )
    .await?;
    Ok(Json(json!(f)))
}

#[utoipa::path(put, path = "/api/v1/admin/catalog/products/{id}/fields/{field}", request_body = FieldInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_field(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path((id, field)): Path<(Uuid, Uuid)>,
    Json(b): Json<FieldInput>,
) -> ApiResult<Json<Value>> {
    let f = addons::upsert_field(&s.pool, id, Some(field), &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.field_saved",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "field": f.id }),
    )
    .await?;
    Ok(Json(json!(f)))
}

#[utoipa::path(delete, path = "/api/v1/admin/catalog/products/{id}/fields/{field}", responses((status = 200), (status = 404)))]
pub async fn delete_field(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path((id, field)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    if !addons::delete_field(&s.pool, id, field).await? {
        return Err(AppError::NotFound.into());
    }
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.field_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "field": field }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/catalog/products/{id}/options/{option}", responses((status = 200), (status = 404)))]
pub async fn delete_option(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path((id, option)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    catalog::delete_option(&s.pool, id, option).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "catalog.option_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "option": option }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
