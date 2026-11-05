//! Network / service status issues: public status page and staff editor
//! (`status.manage`). Every status change is recorded as a timeline update.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::network_status::{self as ns, IssueBody, UpdateBody};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/network-status", get(public_status))
        .route("/admin/network-issues", get(admin_list).post(create))
        .route("/admin/network-issues/{id}", put(update).delete(remove))
        .route("/admin/network-issues/{id}/updates", post(add_update))
}

/// Public: open issues plus anything resolved in the last 14 days.
#[utoipa::path(get, path = "/api/v1/public/network-status", responses((status = 200)))]
pub async fn public_status(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let items = ns::list(&s.pool, true, 100).await?;
    let open = items
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|i| i["status"] != "resolved" && i["status"] != "scheduled")
                .count()
        })
        .unwrap_or(0);
    Ok(Json(json!({ "operational": open == 0, "items": items })))
}

#[utoipa::path(get, path = "/api/v1/admin/network-issues", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::StatusManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": ns::list(&s.pool, false, 200).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/network-issues", request_body = IssueBody, responses((status = 200), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::StatusManage>,
    Json(b): Json<IssueBody>,
) -> ApiResult<Json<Value>> {
    let id = ns::create(&s.pool, &b, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "network_issue.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(put, path = "/api/v1/admin/network-issues/{id}", request_body = IssueBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::StatusManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<IssueBody>,
) -> ApiResult<Json<Value>> {
    ns::update(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "network_issue.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/network-issues/{id}/updates", request_body = UpdateBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn add_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::StatusManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateBody>,
) -> ApiResult<Json<Value>> {
    ns::add_update(&s.pool, id, &b, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "network_issue.update_posted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/network-issues/{id}", responses((status = 200), (status = 404)))]
pub async fn remove(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::StatusManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    ns::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "network_issue.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
