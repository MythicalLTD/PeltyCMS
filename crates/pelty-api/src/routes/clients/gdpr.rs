//! GDPR: self-service data export and erasure requests; admin review.
//! `/account/*` is session-only (API keys are refused by the extractor).
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_auth::password;
use pelty_core::AppError;
use pelty_database::{audit, gdpr, users};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::ApiResult,
    extract::{perms, CurrentUser, RequirePermission},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/account/privacy/export", get(export_me))
        .route(
            "/account/privacy/erasure",
            get(my_request).post(request_erasure).delete(cancel_erasure),
        )
        .route("/admin/gdpr/requests", get(list))
        .route("/admin/gdpr/requests/{id}/approve", post(approve))
        .route("/admin/gdpr/requests/{id}/reject", post(reject))
        .route("/admin/gdpr/users/{id}/export", get(export_user))
}

fn download(v: &Value, user_id: Uuid) -> Response {
    let body = serde_json::to_vec_pretty(v).unwrap_or_default();
    (
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"peltycms-data-{}.json\"",
                    user_id.simple()
                ),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/v1/account/privacy/export", responses((status = 200, description = "JSON download of all personal data"), (status = 401)))]
pub async fn export_me(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Response> {
    let v = gdpr::export(&s.pool, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gdpr.exported",
        Some(&u.user_id.to_string()),
        u.ip.as_deref(),
        json!({ "by": "self" }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gdpr.exported",
        json!({ "user_id": u.user_id, "by_staff": false }),
    ));
    Ok(download(&v, u.user_id))
}

#[utoipa::path(get, path = "/api/v1/account/privacy/erasure", responses((status = 200), (status = 401)))]
pub async fn my_request(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let r = gdpr::latest_for(&s.pool, u.user_id).await?;
    let blocker = gdpr::blocker(&s.pool, u.user_id).await?;
    Ok(Json(json!({ "request": r, "blocker": blocker })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ErasureBody {
    pub password: String,
    #[serde(default)]
    pub reason: String,
}

#[utoipa::path(post, path = "/api/v1/account/privacy/erasure", request_body = ErasureBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn request_erasure(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<ErasureBody>,
) -> ApiResult<Json<Value>> {
    let user = users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    if !password::verify(&b.password, &user.password_hash) {
        return Err(AppError::Validation("error.invalid_password").into());
    }
    let reason = b.reason.trim();
    if reason.chars().count() > 2000 {
        return Err(AppError::Validation("error.too_long").into());
    }
    let r = gdpr::create(&s.pool, u.user_id, &user.email, reason)
        .await
        .map_err(|e| match e {
            AppError::Conflict(_) => AppError::Conflict("error.gdpr_pending"),
            e => e,
        })?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gdpr.erasure_requested",
        Some(&r.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gdpr.erasure_requested",
        json!({ "request_id": r.id, "user_id": u.user_id }),
    ));
    Ok(Json(json!({ "request": r })))
}

#[utoipa::path(delete, path = "/api/v1/account/privacy/erasure", responses((status = 204), (status = 404)))]
pub async fn cancel_erasure(State(s): State<AppState>, u: CurrentUser) -> ApiResult<StatusCode> {
    if !gdpr::cancel(&s.pool, u.user_id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gdpr.erasure_cancelled",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gdpr.erasure_cancelled",
        json!({ "user_id": u.user_id }),
    ));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub status: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/gdpr/requests", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::GdprManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = crate::routes::Page {
        page: q.page,
        per_page: q.per_page,
    }
    .bounds();
    let status = q
        .status
        .as_deref()
        .filter(|x| ["pending", "completed", "rejected", "cancelled"].contains(x));
    let (items, total) = gdpr::list(&s.pool, status, limit, offset).await?;
    let mut out = Vec::with_capacity(items.len());
    for r in items {
        let blocker = match (r.status.as_str(), r.user_id) {
            ("pending", Some(uid)) => gdpr::blocker(&s.pool, uid).await?,
            _ => None,
        };
        let mut v = serde_json::to_value(&r).map_err(|e| AppError::Internal(e.to_string()))?;
        v["blocker"] = json!(blocker);
        out.push(v);
    }
    Ok(Json(crate::routes::paged(out, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/admin/gdpr/requests/{id}/approve", responses((status = 204), (status = 404), (status = 409)))]
pub async fn approve(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GdprManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let r = gdpr::get(&s.pool, id)
        .await?
        .filter(|r| r.status == "pending")
        .ok_or(AppError::NotFound)?;
    let uid = r.user_id.ok_or(AppError::NotFound)?;
    if uid == u.user_id {
        return Err(AppError::Conflict("error.gdpr_staff_account").into());
    }
    if let Some(code) = gdpr::blocker(&s.pool, uid).await? {
        return Err(AppError::Conflict(code).into());
    }
    // Cancel-only pre-event, e.g. a legal hold kept by an addon.
    crate::addon_ext::pre_event(
        &s,
        "gdpr.before_erase",
        json!({ "request_id": id, "user_id": uid, "by": u.user_id }),
    )
    .await?;
    gdpr::erase(&s.pool, id, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gdpr.erased",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "user": uid }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gdpr.erased",
        json!({ "request_id": id, "user_id": uid, "by": u.user_id }),
    ));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RejectBody {
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/admin/gdpr/requests/{id}/reject", request_body = RejectBody, responses((status = 204), (status = 404)))]
pub async fn reject(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GdprManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RejectBody>,
) -> ApiResult<StatusCode> {
    let note = b.note.trim();
    if note.chars().count() > 2000 {
        return Err(AppError::Validation("error.too_long").into());
    }
    if !gdpr::reject(&s.pool, id, u.user_id, note).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gdpr.erasure_rejected",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gdpr.erasure_rejected",
        json!({ "request_id": id, "by": u.user_id }),
    ));
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/gdpr/users/{id}/export", responses((status = 200), (status = 403), (status = 404)))]
pub async fn export_user(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GdprManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    users::find(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let v = gdpr::export(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gdpr.exported",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "by": "admin" }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gdpr.exported",
        json!({ "user_id": id, "by_staff": true }),
    ));
    Ok(download(&v, id))
}
