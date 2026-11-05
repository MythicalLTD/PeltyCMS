//! Project management (WHMCS Project Management addon).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::billing::DUE_DAYS;
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::projects::{self as pj, ProjectInput, TaskInput, TimeInput};
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/projects", get(list).post(create))
        .route(
            "/admin/projects/{id}",
            get(detail).put(update).delete(remove),
        )
        .route("/admin/projects/{id}/tasks", post(add_task))
        .route(
            "/admin/projects/{id}/tasks/{task}",
            put(set_task).delete(delete_task),
        )
        .route("/admin/projects/{id}/time", post(add_time))
        .route("/admin/projects/{id}/time/{entry}", delete(delete_time))
        .route("/admin/projects/{id}/links", post(link).delete(unlink))
        .route("/admin/projects/{id}/invoice-time", post(invoice_time))
        .route("/client/projects", get(client_list))
        .route("/client/projects/{id}", get(client_detail))
}

type Tm = RequirePermission<perms::TodosManage>;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub customer_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/admin/projects", params(ListQuery), responses((status = 200)))]
pub async fn list(
    State(s): State<AppState>,
    _p: Tm,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": pj::list(&s.pool, q.customer_id, false).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/projects", request_body = ProjectInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Json(b): Json<ProjectInput>,
) -> ApiResult<Json<Value>> {
    let id = pj::create(&s.pool, u.user_id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "title": b.title, "customer_id": b.customer_id }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(get, path = "/api/v1/admin/projects/{id}", responses((status = 200), (status = 404)))]
pub async fn detail(
    State(s): State<AppState>,
    _p: Tm,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(pj::detail(&s.pool, id, None).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/projects/{id}", request_body = ProjectInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<ProjectInput>,
) -> ApiResult<Json<Value>> {
    pj::update(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/projects/{id}", responses((status = 200), (status = 404)))]
pub async fn remove(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    pj::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/projects/{id}/tasks", request_body = TaskInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn add_task(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskInput>,
) -> ApiResult<Json<Value>> {
    let task = pj::add_task(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.task_added",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "task": task }),
    )
    .await?;
    Ok(Json(json!({ "id": task })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TaskState {
    pub done: bool,
}

#[utoipa::path(put, path = "/api/v1/admin/projects/{id}/tasks/{task}", request_body = TaskState, responses((status = 200), (status = 404)))]
pub async fn set_task(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path((id, task)): Path<(Uuid, Uuid)>,
    Json(b): Json<TaskState>,
) -> ApiResult<Json<Value>> {
    pj::set_task_done(&s.pool, id, task, b.done).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.task_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "task": task, "done": b.done }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/projects/{id}/tasks/{task}", responses((status = 200), (status = 404)))]
pub async fn delete_task(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path((id, task)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    pj::delete_task(&s.pool, id, task).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.task_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "task": task }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/projects/{id}/time", request_body = TimeInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn add_time(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<TimeInput>,
) -> ApiResult<Json<Value>> {
    let entry = pj::add_time(&s.pool, id, u.user_id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.time_added",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "entry": entry }),
    )
    .await?;
    Ok(Json(json!({ "id": entry })))
}

#[utoipa::path(delete, path = "/api/v1/admin/projects/{id}/time/{entry}", responses((status = 200), (status = 409)))]
pub async fn delete_time(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path((id, entry)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    pj::delete_time(&s.pool, id, entry).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.time_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "entry": entry }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LinkBody {
    /// `ticket` or `invoice`.
    pub kind: String,
    pub target_id: Uuid,
}

#[utoipa::path(post, path = "/api/v1/admin/projects/{id}/links", request_body = LinkBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn link(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<LinkBody>,
) -> ApiResult<Json<Value>> {
    pj::link(&s.pool, id, &b.kind, b.target_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.linked",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "kind": b.kind, "target_id": b.target_id }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/projects/{id}/links", request_body = LinkBody, responses((status = 200)))]
pub async fn unlink(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<LinkBody>,
) -> ApiResult<Json<Value>> {
    pj::unlink(&s.pool, id, &b.kind, b.target_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.unlinked",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "kind": b.kind, "target_id": b.target_id }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Bill all unbilled billable time on one invoice.
#[utoipa::path(post, path = "/api/v1/admin/projects/{id}/invoice-time", responses((status = 200), (status = 404), (status = 409)))]
pub async fn invoice_time(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let inv = pj::invoice_time(&s.pool, id, DUE_DAYS).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "project.time_invoiced",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "invoice_id": inv }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "invoice.created",
        json!({ "invoice_id": inv, "project_id": id }),
    ));
    Ok(Json(json!({ "invoice_id": inv })))
}

#[utoipa::path(get, path = "/api/v1/client/projects", responses((status = 200)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    Ok(Json(
        json!({ "items": pj::list(&s.pool, Some(c), true).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/projects/{id}", responses((status = 200), (status = 404)))]
pub async fn client_detail(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    Ok(Json(pj::detail(&s.pool, id, Some(c)).await?))
}
