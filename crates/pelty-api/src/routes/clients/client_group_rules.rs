//! Automatic client-group assignment rules (`client_groups.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::client_group_rules::{self as rules, KINDS};
use pelty_database::{audit, client_groups};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

type Cg = RequirePermission<perms::ClientGroupsManage>;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/client-group-rules", get(list).post(create))
        .route("/admin/client-group-rules/{id}", put(update).delete(remove))
        .route("/admin/client-group-rules/apply", post(apply))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleBody {
    pub group_id: Uuid,
    #[serde(default = "default_priority")]
    pub priority: i32,
    pub kind: String,
    pub value: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn default_priority() -> i32 {
    100
}
fn yes() -> bool {
    true
}

async fn validate(s: &AppState, b: &RuleBody) -> Result<String, AppError> {
    if !(0..=10000).contains(&b.priority) {
        return Err(AppError::Validation("error.invalid_priority"));
    }
    client_groups::get(&s.pool, b.group_id).await?.ok_or(AppError::Validation("error.invalid_group"))?;
    rules::normalize(&b.kind, &b.value)
}

#[utoipa::path(get, path = "/api/v1/admin/client-group-rules", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: Cg) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": rules::list(&s.pool).await?, "kinds": KINDS })))
}

#[utoipa::path(post, path = "/api/v1/admin/client-group-rules", request_body = RuleBody, responses((status = 200), (status = 422)))]
pub async fn create(State(s): State<AppState>, RequirePermission(u, _): Cg, Json(b): Json<RuleBody>) -> ApiResult<Json<Value>> {
    let v = validate(&s, &b).await?;
    let r = rules::create(&s.pool, b.group_id, b.priority, &b.kind, &v, b.enabled).await?;
    audit::record(&s.pool, Some(u.user_id), "client_group_rule.created", Some(&r.id.to_string()), u.ip.as_deref(), json!({ "kind": r.kind, "group_id": r.group_id })).await?;
    Ok(Json(json!({ "rule": r })))
}

#[utoipa::path(put, path = "/api/v1/admin/client-group-rules/{id}", request_body = RuleBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(State(s): State<AppState>, RequirePermission(u, _): Cg, Path(id): Path<Uuid>, Json(b): Json<RuleBody>) -> ApiResult<Json<Value>> {
    let v = validate(&s, &b).await?;
    let r = rules::update(&s.pool, id, b.group_id, b.priority, &b.kind, &v, b.enabled).await?.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "client_group_rule.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "kind": r.kind })).await?;
    Ok(Json(json!({ "rule": r })))
}

#[utoipa::path(delete, path = "/api/v1/admin/client-group-rules/{id}", responses((status = 204), (status = 404)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): Cg, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    if !rules::delete(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "client_group_rule.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyBody {
    #[serde(default = "yes")]
    pub dry_run: bool,
    #[serde(default)]
    pub overwrite: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/client-group-rules/apply", request_body = ApplyBody, responses((status = 200)))]
pub async fn apply(State(s): State<AppState>, RequirePermission(u, _): Cg, Json(b): Json<ApplyBody>) -> ApiResult<Json<Value>> {
    let changes = rules::apply(&s.pool, b.dry_run, b.overwrite).await?;
    if !b.dry_run {
        audit::record(&s.pool, Some(u.user_id), "client_group_rules.applied", None, u.ip.as_deref(), json!({ "changed": changes.len(), "overwrite": b.overwrite })).await?;
    }
    Ok(Json(json!({ "dry_run": b.dry_run, "count": changes.len(), "changes": changes })))
}
