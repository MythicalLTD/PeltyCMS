//! Tier upgrades (module `tier_upgrades`): admin tier groups and the public
//! upgrade-credit preview for the store.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::tiers::{self, TierGroup, TierGroupInput};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/tier-upgrades", get(public_upgrades))
        .route(
            "/admin/tier-groups",
            get(admin_groups).post(admin_create_group),
        )
        .route(
            "/admin/tier-groups/{id}",
            put(admin_update_group).delete(admin_delete_group),
        )
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct UpgradeQuery {
    /// Currency of the credit preview (default EUR).
    #[serde(default)]
    pub currency: Option<String>,
    /// Preview a gift: credit from this player's tiers instead.
    #[serde(default)]
    pub gift_to: Option<String>,
}

/// Tier groups, and for a signed-in customer the upgrade credit per package
/// (`credits[product_id] = { amount, from_name }`).
#[utoipa::path(get, path = "/api/v1/public/tier-upgrades", params(UpgradeQuery), responses((status = 200), (status = 404)))]
pub async fn public_upgrades(
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<UpgradeQuery>,
) -> ApiResult<Json<Value>> {
    let groups: Vec<Value> = tiers::public(&s.pool)
        .await?
        .into_iter()
        .map(|g| json!({ "id": g.id, "name": g.name, "tiers": g.tiers }))
        .collect();
    let currency = q
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|c| c.len() == 3 && c.chars().all(|ch| ch.is_ascii_alphabetic()))
        .unwrap_or("EUR")
        .to_ascii_uppercase();
    let gift = q
        .gift_to
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty());
    if let Some(g) = gift {
        pelty_database::store_rules::gift_name(g)?;
    }
    let credits = match crate::routes::store::maybe_customer(&s, &headers).await {
        Some(c) => match tiers::preview_for(&s.pool, c, gift, &currency).await {
            Ok(m) => json!(m),
            // Unknown or disabled currency: no preview rather than an error.
            Err(AppError::Validation(_)) => json!({}),
            Err(e) => return Err(e.into()),
        },
        None => json!({}),
    };
    Ok(Json(json!({ "groups": groups, "credits": credits })))
}

#[utoipa::path(get, path = "/api/v1/admin/tier-groups", responses((status = 200), (status = 403)))]
pub async fn admin_groups(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": tiers::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/tier-groups", request_body = TierGroupInput, responses((status = 200, body = TierGroup), (status = 403), (status = 422)))]
pub async fn admin_create_group(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<TierGroupInput>,
) -> ApiResult<Json<TierGroup>> {
    let g = tiers::save(&s.pool, None, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tier_group.created",
        Some(&g.id.to_string()),
        u.ip.as_deref(),
        json!({ "name": g.name, "products": b.product_ids, "mode": g.credit_mode }),
    )
    .await?;
    Ok(Json(g))
}

#[utoipa::path(put, path = "/api/v1/admin/tier-groups/{id}", request_body = TierGroupInput, params(("id" = Uuid, Path)), responses((status = 200, body = TierGroup), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_update_group(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TierGroupInput>,
) -> ApiResult<Json<TierGroup>> {
    let g = tiers::save(&s.pool, Some(id), &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tier_group.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": g.name, "products": b.product_ids, "mode": g.credit_mode }),
    )
    .await?;
    Ok(Json(g))
}

#[utoipa::path(delete, path = "/api/v1/admin/tier-groups/{id}", params(("id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404)))]
pub async fn admin_delete_group(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    tiers::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tier_group.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
