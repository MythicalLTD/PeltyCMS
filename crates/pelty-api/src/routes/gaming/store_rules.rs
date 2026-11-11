//! Game store rules (module `gaming_storefront`): package rules (limits,
//! required packages, pay what you want, gifting) and the store ban list.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::store_rules::{self as sr, Ban, Rule};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/storefront/package-rules", get(public_rules))
        .route("/admin/storefront/package-rules", get(admin_rules))
        .route(
            "/admin/storefront/package-rules/{product_id}",
            put(admin_save_rule),
        )
        .route(
            "/admin/storefront/bans",
            get(admin_bans).post(admin_add_ban),
        )
        .route("/admin/storefront/bans/{id}", delete(admin_remove_ban))
}

/// Limits left, requirements, pay-what-you-want and gifting per package.
/// Signed-in customers also get their own remaining limit and whether they
/// meet the requirement.
#[utoipa::path(get, path = "/api/v1/public/storefront/package-rules", responses((status = 200), (status = 404)))]
pub async fn public_rules(
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
) -> ApiResult<Json<Value>> {
    let who = crate::routes::store::maybe_customer(&s, &headers).await;
    Ok(Json(json!({ "items": sr::public(&s.pool, who).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/storefront/package-rules", responses((status = 200), (status = 403)))]
pub async fn admin_rules(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": sr::list(&s.pool).await? })))
}

/// Set a package's rule; a rule with every default removes it.
#[utoipa::path(put, path = "/api/v1/admin/storefront/package-rules/{product_id}", request_body = Rule, params(("product_id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_save_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(product_id): Path<Uuid>,
    Json(mut r): Json<Rule>,
) -> ApiResult<Json<Value>> {
    r.product_id = product_id;
    sr::save(&s.pool, &r).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "package_rule.updated",
        Some(&product_id.to_string()),
        u.ip.as_deref(),
        json!(r),
    )
    .await?;
    Ok(Json(json!(r)))
}

#[utoipa::path(get, path = "/api/v1/admin/storefront/bans", responses((status = 200), (status = 403)))]
pub async fn admin_bans(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": sr::bans(&s.pool).await? })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BanBody {
    /// `username` (in-game name) or `email`.
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub reason: String,
}

#[utoipa::path(post, path = "/api/v1/admin/storefront/bans", request_body = BanBody, responses((status = 200, body = Ban), (status = 403), (status = 422)))]
pub async fn admin_add_ban(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<BanBody>,
) -> ApiResult<Json<Ban>> {
    let ban = sr::add_ban(&s.pool, &b.kind, &b.value, &b.reason).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "store_ban.added",
        Some(&ban.id.to_string()),
        u.ip.as_deref(),
        json!({ "kind": ban.kind, "value": ban.value }),
    )
    .await?;
    Ok(Json(ban))
}

#[utoipa::path(delete, path = "/api/v1/admin/storefront/bans/{id}", params(("id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404)))]
pub async fn admin_remove_ban(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    sr::remove_ban(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "store_ban.removed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "deleted": true })))
}
