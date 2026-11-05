//! Staff side of live chat: list, claim and answer chats. Child of `live_chat`.
use super::{db, Tm};
use crate::{error::ApiResult, extract::RequirePermission, AppState};
use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use pelty_core::AppError;
use pelty_database::audit;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat/availability", get(availability))
        .route("/admin/chats/settings", get(settings_get).put(settings_put))
        .route("/admin/chats/{id}/assignment", put(assign))
        .route("/admin/chats/{id}/ticket", post(convert))
        .route("/admin/chats/{id}/actions", post(action))
}
#[utoipa::path(get, path = "/api/v1/chat/availability", responses((status = 200)))]
pub async fn availability(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let settings = db::settings(&s.pool).await?;
    let legacy = pelty_database::general_settings::load(&s.pool)
        .await?
        .support
        .live_chat;
    let departments = s
        .tickets()
        .departments(true)
        .await?
        .into_iter()
        .filter(|d| settings.departments.is_empty() || settings.departments.contains(&d.id))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"enabled": legacy && settings.enabled, "open": legacy && settings.is_open(time::OffsetDateTime::now_utc()), "departments": departments}),
    ))
}
#[utoipa::path(get, path = "/api/v1/admin/chats/settings", responses((status = 200, body = db::ChatSettings), (status = 403)))]
pub async fn settings_get(State(s): State<AppState>, _u: Tm) -> ApiResult<Json<db::ChatSettings>> {
    Ok(Json(db::settings(&s.pool).await?))
}
#[utoipa::path(put, path = "/api/v1/admin/chats/settings", request_body = db::ChatSettings, responses((status = 200), (status = 403), (status = 422)))]
pub async fn settings_put(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Json(b): Json<db::ChatSettings>,
) -> ApiResult<Json<Value>> {
    db::save_settings(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "chat.settings_updated",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub assigned_to: Option<Uuid>,
}
#[utoipa::path(put, path = "/api/v1/admin/chats/{id}/assignment", request_body = Assignment, responses((status = 200), (status = 403), (status = 409), (status = 422)))]
pub async fn assign(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<Assignment>,
) -> ApiResult<Json<Value>> {
    db::status(&s.pool, id).await?;
    db::assign(&s.pool, id, b.assigned_to).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "chat.assigned",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({"assigned_to": b.assigned_to}),
    )
    .await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Conversion {
    pub customer_id: Uuid,
    pub department_id: Uuid,
    pub subject: String,
}
#[utoipa::path(post, path = "/api/v1/admin/chats/{id}/ticket", request_body = Conversion, responses((status = 200), (status = 403), (status = 404), (status = 422)))]
pub async fn convert(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<Conversion>,
) -> ApiResult<Json<Value>> {
    let (ticket, created) = db::convert(
        &s.pool,
        id,
        u.user_id,
        b.customer_id,
        b.department_id,
        &b.subject,
    )
    .await?;
    if created {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "chat.converted",
            json!({"chat_id":id,"ticket_id":ticket,"by":u.user_id}),
        ));
    }
    Ok(Json(json!({"ticket_id":ticket})))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatAction {
    /// open_page | open_product
    pub kind: String,
    pub href: String,
    pub label: String,
}

fn safe_chat_href(href: &str) -> bool {
    let h = href.trim();
    if !h.starts_with('/') || h.starts_with("//") || h.contains('\\') {
        return false;
    }
    h.bytes()
        .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'_' | b'-' | b'.' | b'~' | b'%'))
}

#[utoipa::path(post, path = "/api/v1/admin/chats/{id}/actions", request_body = ChatAction, responses((status = 200), (status = 403), (status = 409), (status = 422)))]
pub async fn action(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<ChatAction>,
) -> ApiResult<Json<Value>> {
    db::status(&s.pool, id).await?;
    let kind = b.kind.trim();
    let href = b.href.trim();
    let label = b.label.trim();
    if label.is_empty() || label.chars().count() > 120 || !safe_chat_href(href) {
        return Err(AppError::Validation("error.chat_action_invalid").into());
    }
    match kind {
        "open_page" => {}
        "open_product" => {
            let Some(rest) = href.strip_prefix("/store/p/") else {
                return Err(AppError::Validation("error.chat_action_invalid").into());
            };
            let Ok(pid) = Uuid::parse_str(rest) else {
                return Err(AppError::Validation("error.chat_action_invalid").into());
            };
            if !db::product_exists(&s.pool, pid).await? {
                return Err(AppError::Validation("error.chat_action_invalid").into());
            }
        }
        _ => return Err(AppError::Validation("error.chat_action_invalid").into()),
    }
    let body = serde_json::to_string(&json!({"kind": kind, "href": href, "label": label}))
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let mid = db::post_action(&s.pool, id, u.user_id, &body).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "chat.action_sent",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({"message_id": mid, "kind": kind, "href": href}),
    )
    .await?;
    crate::chat_bus::messages_changed(&s, id).await;
    Ok(Json(json!({"id": mid})))
}
