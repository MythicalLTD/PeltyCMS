//! Link tracking (WHMCS Products > Link Tracking): admin CRUD + public redirect.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::link_tracking as lt;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/tracked-links", get(list).post(create))
        .route("/admin/tracked-links/{id}", delete(remove))
        .route("/go/{slug}", get(follow))
}

#[utoipa::path(get, path = "/api/v1/admin/tracked-links", responses((status = 200), (status = 403)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": lt::list(&s.pool).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewLink {
    pub slug: String,
    pub name: String,
    pub target_url: String,
}

#[utoipa::path(post, path = "/api/v1/admin/tracked-links", request_body = NewLink, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<NewLink>,
) -> ApiResult<Json<Value>> {
    let id = lt::create(&s.pool, &b.slug, &b.name, &b.target_url).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tracked_link.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": b.slug.trim().to_ascii_lowercase() }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(delete, path = "/api/v1/admin/tracked-links/{id}", responses((status = 200), (status = 404)))]
pub async fn remove(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    lt::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "tracked_link.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Public: count the click and 303 to the stored target.
#[utoipa::path(get, path = "/api/v1/go/{slug}", responses((status = 303), (status = 404)))]
pub async fn follow(State(s): State<AppState>, Path(slug): Path<String>) -> ApiResult<Response> {
    Ok(match lt::hit(&s.pool, &slug).await? {
        Some(url) => Redirect::to(&url).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}
