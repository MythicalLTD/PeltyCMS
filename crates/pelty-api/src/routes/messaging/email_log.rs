//! WHMCS "Email Message Log" (staff, all mail with body and resend) and the
//! client area "Email History" (own mail only).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, email_log};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/email-log", get(admin_list))
        .route("/admin/email-log/{id}", get(admin_get))
        .route("/admin/email-log/{id}/resend", post(admin_resend))
        .route("/client/emails", get(client_list))
        .route("/client/emails/{id}", get(client_get))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub user_id: Option<Uuid>,
    pub q: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

fn q(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|x| !x.is_empty())
}

#[utoipa::path(get, path = "/api/v1/admin/email-log", responses((status = 200), (status = 403)))]
pub async fn admin_list(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>, Query(f): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let (items, total) = email_log::list(&s.pool, f.user_id, q(&f.q), f.limit, f.offset).await?;
    Ok(Json(json!({ "items": items, "total": total })))
}

#[utoipa::path(get, path = "/api/v1/admin/email-log/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_get(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = email_log::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!(d)))
}

#[utoipa::path(post, path = "/api/v1/admin/email-log/{id}/resend", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_resend(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = email_log::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    if d.body_text == crate::mail::REDACTED {
        return Err(AppError::Conflict("error.email_not_resendable").into());
    }
    crate::mail::resend_logged(&s, &d).await?;
    audit::record(&s.pool, Some(u.user_id), "email.resent", Some(&id.to_string()), u.ip.as_deref(), json!({ "template": d.entry.template })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/client/emails", responses((status = 200), (status = 401)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser, Query(f): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let (items, total) = email_log::list(&s.pool, Some(u.user_id), q(&f.q), f.limit, f.offset).await?;
    Ok(Json(json!({ "items": items, "total": total })))
}

#[utoipa::path(get, path = "/api/v1/client/emails/{id}", responses((status = 200), (status = 404)))]
pub async fn client_get(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = email_log::get(&s.pool, id).await?.filter(|d| d.entry.user_id == Some(u.user_id)).ok_or(AppError::NotFound)?;
    Ok(Json(json!(d)))
}
