//! Client notes and custom client field values (`customers.manage`), field
//! definitions (`customers.manage`), the client's own editable fields, and
//! the admin to-do list (`todos.manage`).
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::client_extras::{self as cx, FieldInput, NoteInput, TodoInput};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/customers/{id}/notes", get(list_notes).post(add_note))
        .route("/admin/customers/{id}/notes/{note}", put(update_note).delete(delete_note))
        .route("/admin/customers/{id}/fields", get(admin_values).put(admin_set_values))
        .route("/admin/client-fields", get(list_fields).post(create_field))
        .route("/admin/client-fields/{id}", put(update_field).delete(delete_field))
        .route("/client/fields", get(client_values).put(client_set_values))
        .route("/admin/todos", get(list_todos).post(create_todo))
        .route("/admin/todos/{id}", put(update_todo).delete(delete_todo))
}

type Cm = RequirePermission<perms::CustomersManage>;
type Tm = RequirePermission<perms::TodosManage>;

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/notes", responses((status = 200)))]
pub async fn list_notes(State(s): State<AppState>, _p: Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": cx::notes(&s.pool, id).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/notes", request_body = NoteInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn add_note(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>, Json(b): Json<NoteInput>) -> ApiResult<Json<Value>> {
    let nid = cx::add_note(&s.pool, id, u.user_id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.note_added", Some(&id.to_string()), u.ip.as_deref(), json!({ "note": nid })).await?;
    Ok(Json(json!({ "id": nid })))
}

#[utoipa::path(put, path = "/api/v1/admin/customers/{id}/notes/{note}", request_body = NoteInput, responses((status = 200), (status = 404)))]
pub async fn update_note(State(s): State<AppState>, RequirePermission(u, _): Cm, Path((id, note)): Path<(Uuid, Uuid)>, Json(b): Json<NoteInput>) -> ApiResult<Json<Value>> {
    cx::update_note(&s.pool, id, note, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.note_updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "note": note })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/customers/{id}/notes/{note}", responses((status = 200), (status = 404)))]
pub async fn delete_note(State(s): State<AppState>, RequirePermission(u, _): Cm, Path((id, note)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    cx::delete_note(&s.pool, id, note).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.note_deleted", Some(&id.to_string()), u.ip.as_deref(), json!({ "note": note })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/client-fields", responses((status = 200)))]
pub async fn list_fields(State(s): State<AppState>, _p: Cm) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": cx::fields(&s.pool, true).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/client-fields", request_body = FieldInput, responses((status = 200), (status = 422)))]
pub async fn create_field(State(s): State<AppState>, RequirePermission(u, _): Cm, Json(b): Json<FieldInput>) -> ApiResult<Json<Value>> {
    let id = cx::create_field(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "client_field.created", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name })).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(put, path = "/api/v1/admin/client-fields/{id}", request_body = FieldInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_field(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>, Json(b): Json<FieldInput>) -> ApiResult<Json<Value>> {
    cx::update_field(&s.pool, id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "client_field.updated", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/client-fields/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_field(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    cx::delete_field(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "client_field.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ValuesBody {
    #[schema(value_type = Object)]
    pub values: HashMap<Uuid, String>,
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/fields", responses((status = 200)))]
pub async fn admin_values(State(s): State<AppState>, _p: Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "fields": cx::fields(&s.pool, true).await?, "values": cx::values(&s.pool, id).await? })))
}

#[utoipa::path(put, path = "/api/v1/admin/customers/{id}/fields", request_body = ValuesBody, responses((status = 200), (status = 422)))]
pub async fn admin_set_values(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>, Json(b): Json<ValuesBody>) -> ApiResult<Json<Value>> {
    cx::set_values(&s.pool, id, true, &b.values).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.fields_updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "count": b.values.len() })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/client/fields", responses((status = 200), (status = 401)))]
pub async fn client_values(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let fields = cx::fields(&s.pool, false).await?;
    let mut vals = cx::values(&s.pool, cid).await?;
    vals.retain(|k, _| fields.iter().any(|f| f.id == *k));
    Ok(Json(json!({ "fields": fields, "values": vals })))
}

#[utoipa::path(put, path = "/api/v1/client/fields", request_body = ValuesBody, responses((status = 200), (status = 422)))]
pub async fn client_set_values(State(s): State<AppState>, u: CurrentUser, Json(b): Json<ValuesBody>) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    cx::set_values(&s.pool, cid, false, &b.values).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TodoQuery {
    /// open | pending | in_progress | done (omit for all)
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/todos", params(TodoQuery), responses((status = 200)))]
pub async fn list_todos(State(s): State<AppState>, _p: Tm, Query(q): Query<TodoQuery>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": cx::todos(&s.pool, q.status.as_deref().filter(|x| !x.is_empty())).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/todos", request_body = TodoInput, responses((status = 200), (status = 422)))]
pub async fn create_todo(State(s): State<AppState>, RequirePermission(u, _): Tm, Json(b): Json<TodoInput>) -> ApiResult<Json<Value>> {
    let id = cx::create_todo(&s.pool, u.user_id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "todo.created", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(put, path = "/api/v1/admin/todos/{id}", request_body = TodoInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_todo(State(s): State<AppState>, RequirePermission(u, _): Tm, Path(id): Path<Uuid>, Json(b): Json<TodoInput>) -> ApiResult<Json<Value>> {
    cx::update_todo(&s.pool, id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "todo.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "status": b.status })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/todos/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_todo(State(s): State<AppState>, RequirePermission(u, _): Tm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    cx::delete_todo(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "todo.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
