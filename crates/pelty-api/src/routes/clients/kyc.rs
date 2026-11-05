//! KYC endpoints: customers upload identity documents (images/PDF, sniffed
//! by magic bytes), staff review them; ordering can be gated on the result.
use crate::routes::customers::customer_of;
use crate::routes::ticket_attachments::{clean_name, serve, sniff};
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, kyc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

/// Request cap: `PELTY_UPLOAD_KYC_MB` (default 20 MB); the admin per-file limit applies below it.
fn hard_max() -> usize {
    pelty_core::config::upload_limits().kyc
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/kyc/documents", post(client_upload))
        .layer(DefaultBodyLimit::max(hard_max() + 1024))
        .route("/client/kyc", get(client_get))
        .route("/client/kyc/documents/{id}", delete(client_delete))
        .route("/admin/kyc", get(admin_queue))
        .route("/admin/kyc/settings", get(settings_get).put(settings_put))
        .route("/admin/customers/{id}/kyc", get(admin_customer).put(admin_override))
        .route("/admin/kyc/documents/{id}/download", get(admin_download))
        .route("/admin/kyc/documents/{id}/review", post(admin_review))
}

fn public_settings(c: &kyc::KycSettings) -> Value {
    json!({ "enabled": c.enabled, "required_kinds": c.required_kinds, "gate_orders": c.gate_orders, "max_mb": c.max_mb })
}

#[utoipa::path(get, path = "/api/v1/client/kyc", responses((status = 200)))]
pub async fn client_get(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let cfg = kyc::settings(&s.pool).await?;
    Ok(Json(json!({ "settings": public_settings(&cfg), "status": kyc::status(&s.pool, c).await?, "documents": kyc::documents(&s.pool, c).await? })))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct UploadQuery {
    pub kind: String,
    pub filename: Option<String>,
}

/// Raw body upload. PNG/JPEG/WebP/PDF only.
#[utoipa::path(post, path = "/api/v1/client/kyc/documents", params(UploadQuery), request_body(content = Vec<u8>, content_type = "application/octet-stream"), responses((status = 200), (status = 409), (status = 422)))]
pub async fn client_upload(State(s): State<AppState>, u: CurrentUser, headers: HeaderMap, Query(q): Query<UploadQuery>, body: Bytes) -> ApiResult<Json<kyc::Document>> {
    let c = customer_of(&s, &u).await?;
    let cfg = kyc::settings(&s.pool).await?;
    if !cfg.enabled {
        return Err(AppError::Conflict("error.kyc_disabled").into());
    }
    if body.len() > (cfg.max_mb as usize * 1024 * 1024).min(hard_max()) {
        return Err(AppError::BadRequest("error.attachment_too_large").into());
    }
    let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok());
    let kind = sniff(ct, &body).map_err(AppError::Validation)?;
    if !matches!(kind, "image/png" | "image/jpeg" | "image/webp" | "application/pdf") {
        return Err(AppError::Validation("error.attachment_type").into());
    }
    let d = kyc::upload(&s.pool, c, &q.kind, &clean_name(q.filename.as_deref().unwrap_or("document")), kind, &body).await?;
    audit::record(&s.pool, Some(u.user_id), "kyc.document_uploaded", Some(&d.id.to_string()), u.ip.as_deref(), json!({ "kind": d.kind })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("kyc.document_uploaded", json!({ "customer_id": c, "document_id": d.id, "kind": d.kind })));
    Ok(Json(d))
}

#[utoipa::path(delete, path = "/api/v1/client/kyc/documents/{id}", responses((status = 200), (status = 404)))]
pub async fn client_delete(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    kyc::delete_pending(&s.pool, c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct QueueQuery {
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/kyc", params(QueueQuery), responses((status = 200)))]
pub async fn admin_queue(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>, Query(q): Query<QueueQuery>) -> ApiResult<Json<Value>> {
    let st = q.status.filter(|x| !x.is_empty() && x != "all");
    Ok(Json(json!({ "items": kyc::queue(&s.pool, st.as_deref()).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/kyc/settings", responses((status = 200)))]
pub async fn settings_get(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>) -> ApiResult<Json<kyc::KycSettings>> {
    Ok(Json(kyc::settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/kyc/settings", request_body = kyc::KycSettings, responses((status = 200), (status = 422)))]
pub async fn settings_put(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<kyc::KycSettings>) -> ApiResult<Json<kyc::KycSettings>> {
    kyc::set_settings(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "kyc.settings_updated", None, u.ip.as_deref(), json!(b)).await?;
    Ok(Json(kyc::settings(&s.pool).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/kyc", responses((status = 200), (status = 404)))]
pub async fn admin_customer(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "status": kyc::status(&s.pool, id).await?, "documents": kyc::documents(&s.pool, id).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct OverrideBody {
    pub status: String,
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(put, path = "/api/v1/admin/customers/{id}/kyc", request_body = OverrideBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_override(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>, Json(b): Json<OverrideBody>) -> ApiResult<Json<Value>> {
    kyc::set_status(&s.pool, id, &b.status, &b.note).await?;
    audit::record(&s.pool, Some(u.user_id), "kyc.status_overridden", Some(&id.to_string()), u.ip.as_deref(), json!({ "status": b.status })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("kyc.status_changed", json!({ "customer_id": id, "status": b.status, "by": u.user_id })));
    Ok(Json(json!({ "status": kyc::status(&s.pool, id).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/kyc/documents/{id}/download", responses((status = 200), (status = 404)))]
pub async fn admin_download(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let (_, name, ct, data) = kyc::file(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "kyc.document_viewed", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    serve(name, ct, data)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ReviewBody {
    pub approve: bool,
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/admin/kyc/documents/{id}/review", request_body = ReviewBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_review(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>, Json(b): Json<ReviewBody>) -> ApiResult<Json<Value>> {
    let (cid, st) = kyc::review(&s.pool, id, b.approve, &b.note, u.user_id).await?;
    audit::record(&s.pool, Some(u.user_id), "kyc.document_reviewed", Some(&id.to_string()), u.ip.as_deref(), json!({ "approve": b.approve, "customer_id": cid, "status": st })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("kyc.document_reviewed", json!({ "customer_id": cid, "document_id": id, "approved": b.approve, "status": st, "by": u.user_id })));
    Ok(Json(json!({ "customer_id": cid, "kyc_status": st })))
}

