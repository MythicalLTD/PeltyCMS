//! Custom ticket statuses: everyone signed in can read them (for labels),
//! staff with `tickets.manage` manage them.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::ticket_statuses::{self as ts, StatusInput};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ticket-statuses", get(list))
        .route("/admin/ticket-statuses", axum::routing::post(create))
        .route("/admin/ticket-statuses/{slug}", put(update).delete(delete))
}

#[utoipa::path(get, path = "/api/v1/ticket-statuses", responses((status = 200), (status = 401)))]
pub async fn list(State(s): State<AppState>, _u: CurrentUser) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": ts::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/ticket-statuses", request_body = StatusInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::TicketsManage>, Json(b): Json<StatusInput>) -> ApiResult<Json<Value>> {
    ts::validate(&b, true)?;
    ts::create(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "ticket.status_created", Some(&b.slug), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "slug": b.slug })))
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-statuses/{slug}", request_body = StatusInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::TicketsManage>, Path(slug): Path<String>, Json(b): Json<StatusInput>) -> ApiResult<Json<Value>> {
    ts::validate(&b, false)?;
    ts::update(&s.pool, &slug, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "ticket.status_updated", Some(&slug), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "slug": slug })))
}

#[utoipa::path(delete, path = "/api/v1/admin/ticket-statuses/{slug}", responses((status = 200), (status = 404), (status = 409)))]
pub async fn delete(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::TicketsManage>, Path(slug): Path<String>) -> ApiResult<Json<Value>> {
    ts::delete(&s.pool, &slug).await?;
    audit::record(&s.pool, Some(u.user_id), "ticket.status_deleted", Some(&slug), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
