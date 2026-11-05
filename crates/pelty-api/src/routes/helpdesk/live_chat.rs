//! Live chat: public widget endpoints (bearer token per session) and the
//! staff inbox. Both sides poll `?after=<last id>`.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
#[path = "live_chat_admin.rs"]
pub mod admin;
#[path = "live_chat_ws.rs"]
pub mod ws;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::live_chat as db;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(ws::router())
        .merge(admin::router())
        .route("/chat/sessions", post(start))
        .route(
            "/chat/sessions/{id}/messages",
            get(visitor_messages).post(visitor_post),
        )
        .route("/chat/sessions/{id}/close", post(visitor_close))
        .route("/admin/chats", get(inbox))
        .route(
            "/admin/chats/{id}/messages",
            get(staff_messages).post(staff_post),
        )
        .route("/admin/chats/{id}/close", post(staff_close))
}

type Tm = RequirePermission<perms::TicketsManage>;

async fn enabled(s: &AppState) -> ApiResult<()> {
    if !pelty_database::general_settings::load(&s.pool)
        .await?
        .support
        .live_chat
        || !db::settings(&s.pool).await?.enabled
    {
        return Err(AppError::NotFound.into());
    }
    Ok(())
}

fn ip_of(ext: &axum::http::Extensions) -> String {
    ext.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn token(h: &HeaderMap) -> ApiResult<&str> {
    h.get("x-chat-token")
        .and_then(|v| v.to_str().ok())
        .filter(|t| t.len() == 64)
        .ok_or_else(|| AppError::NotFound.into())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatStart {
    pub name: String,
    pub email: String,
    pub message: String,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub department_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/chat/sessions", request_body = ChatStart, responses((status = 200), (status = 404), (status = 422), (status = 429)))]
pub async fn start(
    State(s): State<AppState>,
    ext: axum::http::Extensions,
    Json(b): Json<ChatStart>,
) -> ApiResult<Json<Value>> {
    enabled(&s).await?;
    if !s.auth_limiter.check(&format!("chat:start:{}", ip_of(&ext))) {
        return Err(AppError::RateLimited.into());
    }
    let settings = db::settings(&s.pool).await?;
    if !settings.is_open(time::OffsetDateTime::now_utc()) {
        return Err(AppError::Conflict("error.chat_unavailable").into());
    }
    let departments = s.tickets().departments(true).await?;
    if b.department_id.is_some_and(|d| {
        !departments.iter().any(|v| v.id == d)
            || (!settings.departments.is_empty() && !settings.departments.contains(&d))
    }) || (!settings.departments.is_empty() && b.department_id.is_none())
    {
        return Err(AppError::Validation("error.invalid_department").into());
    }
    let (id, token) = db::start_in_department(
        &s.pool,
        &b.name,
        &b.email,
        &b.page,
        None,
        &b.message,
        b.department_id,
    )
    .await?;
    crate::chat_bus::messages_changed(&s, id).await;
    s.emit(pelty_extensions::events::DomainEvent::new("chat.started", json!({ "chat_id": id, "name": b.name.trim(), "email": b.email.trim().to_ascii_lowercase() })));
    Ok(Json(json!({ "id": id, "token": token })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct After {
    #[serde(default)]
    pub after: i64,
}

#[utoipa::path(get, path = "/api/v1/chat/sessions/{id}/messages", params(After), responses((status = 200), (status = 404)))]
pub async fn visitor_messages(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<After>,
) -> ApiResult<Json<Value>> {
    let status = db::authorize(&s.pool, id, token(&h)?).await?;
    let msgs = db::messages(&s.pool, id, q.after, "visitor").await?;
    Ok(Json(json!({ "status": status, "messages": msgs })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatPost {
    pub body: String,
}

#[utoipa::path(post, path = "/api/v1/chat/sessions/{id}/messages", request_body = ChatPost, responses((status = 200), (status = 404), (status = 409), (status = 422), (status = 429)))]
pub async fn visitor_post(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<ChatPost>,
) -> ApiResult<Json<Value>> {
    db::authorize(&s.pool, id, token(&h)?).await?;
    if !s.auth_limiter.check(&format!("chat:msg:{id}")) {
        return Err(AppError::RateLimited.into());
    }
    let mid = db::post(&s.pool, id, "visitor", None, &b.body).await?;
    crate::chat_bus::messages_changed(&s, id).await;
    Ok(Json(json!({ "id": mid })))
}

#[utoipa::path(post, path = "/api/v1/chat/sessions/{id}/close", responses((status = 200), (status = 404)))]
pub async fn visitor_close(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::authorize(&s.pool, id, token(&h)?).await?;
    db::close(&s.pool, id, "visitor").await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct InboxQuery {
    /// open | closed (default: all)
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/chats", params(InboxQuery), responses((status = 200)))]
pub async fn inbox(
    State(s): State<AppState>,
    _p: Tm,
    Query(q): Query<InboxQuery>,
) -> ApiResult<Json<Value>> {
    let st = q
        .status
        .as_deref()
        .filter(|v| matches!(*v, "open" | "closed"));
    Ok(Json(
        json!({ "sessions": db::list(&s.pool, st).await?, "unread": db::unread_total(&s.pool).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/chats/{id}/messages", params(After), responses((status = 200), (status = 404)))]
pub async fn staff_messages(
    State(s): State<AppState>,
    _p: Tm,
    Path(id): Path<Uuid>,
    Query(q): Query<After>,
) -> ApiResult<Json<Value>> {
    let status = db::status(&s.pool, id).await?;
    Ok(Json(
        json!({ "status": status, "messages": db::messages(&s.pool, id, q.after, "staff").await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/chats/{id}/messages", request_body = ChatPost, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn staff_post(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
    Json(b): Json<ChatPost>,
) -> ApiResult<Json<Value>> {
    db::status(&s.pool, id).await?;
    if !s
        .auth_limiter
        .check(&format!("chat:staff-message:{}", u.user_id))
    {
        return Err(AppError::RateLimited.into());
    }
    let mid = db::post(&s.pool, id, "staff", Some(u.user_id), &b.body).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "chat.message_sent",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({"message_id":mid}),
    )
    .await?;
    crate::chat_bus::messages_changed(&s, id).await;
    Ok(Json(json!({"id":mid})))
}

#[utoipa::path(post, path = "/api/v1/admin/chats/{id}/close", responses((status = 200), (status = 404)))]
pub async fn staff_close(
    State(s): State<AppState>,
    RequirePermission(u, _): Tm,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::status(&s.pool, id).await?;
    db::close(&s.pool, id, "staff").await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "chat.closed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
