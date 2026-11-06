//! Admin productivity: global search (command palette), System Health,
//! personal "My Notes" and staff notification rules (Slack/Discord/Teams/webhook).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_database::admin_tools::{self as at, RuleBody};
use pelty_database::audit;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/search", get(search))
        .route("/client/search", get(client_search))
        .route("/admin/customers/{id}/summary", get(customer_summary))
        .route("/admin/system-health", get(health))
        .route("/admin/my-notes", get(get_notes).put(put_notes))
        .route("/admin/noticeboard", get(list_notices).post(create_notice))
        .route(
            "/admin/noticeboard/{id}",
            axum::routing::delete(delete_notice),
        )
        .route(
            "/admin/notification-rules",
            get(list_rules).post(create_rule),
        )
        .route(
            "/admin/notification-rules/{id}",
            put(update_rule).delete(delete_rule),
        )
        .route("/admin/notification-rules/preview", post(preview))
        .route(
            "/admin/notification-rules/{id}/secret",
            post(rotate_secret).delete(clear_secret),
        )
        .route(
            "/admin/notification-rules/{id}/deliveries",
            get(rule_deliveries),
        )
        .route("/admin/notification-rules/{id}/test", post(test_rule))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct SearchQ {
    pub q: String,
}

/// Global search across clients, invoices, services, tickets, domains, orders.
#[utoipa::path(get, path = "/api/v1/admin/search", params(SearchQ), responses((status = 200), (status = 403), (status = 422)))]
pub async fn search(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::CustomersManage>,
    Query(q): Query<SearchQ>,
) -> ApiResult<Json<Value>> {
    Ok(Json(at::search(&s.pool, &q.q, 6).await?))
}

/// Client-area search over the caller's own invoices, services, tickets, domains, plus public KB.
#[utoipa::path(get, path = "/api/v1/client/search", params(SearchQ), responses((status = 200), (status = 401), (status = 422)))]
pub async fn client_search(
    State(s): State<AppState>,
    u: crate::extract::CurrentUser,
    Query(q): Query<SearchQ>,
) -> ApiResult<Json<Value>> {
    let cid = crate::routes::customers::customer_of(&s, &u).await?;
    Ok(Json(at::client_search(&s.pool, cid, &q.q, 6).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/system-health", responses((status = 200), (status = 403)))]
pub async fn health(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    let mut v = at::health(&s.pool).await?;
    v["app_version"] = json!(env!("CARGO_PKG_VERSION"));
    v["mail_configured"] = json!(true);
    Ok(Json(v))
}

#[utoipa::path(get, path = "/api/v1/admin/my-notes", responses((status = 200), (status = 403)))]
pub async fn get_notes(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TodosManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(at::my_notes(&s.pool, u.user_id).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct NotesBody {
    pub body: String,
}

#[utoipa::path(put, path = "/api/v1/admin/my-notes", request_body = NotesBody, responses((status = 200), (status = 422)))]
pub async fn put_notes(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TodosManage>,
    Json(b): Json<NotesBody>,
) -> ApiResult<Json<Value>> {
    at::save_my_notes(&s.pool, u.user_id, &b.body).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/notification-rules", responses((status = 200), (status = 403)))]
pub async fn list_rules(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::NotificationsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": at::list_rules(&s.pool).await?, "events": at::EVENTS, "critical": at::CRITICAL_EVENTS, "providers": at::PROVIDERS }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/notification-rules", request_body = RuleBody, responses((status = 200), (status = 422)))]
pub async fn create_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::NotificationsManage>,
    Json(b): Json<RuleBody>,
) -> ApiResult<Json<Value>> {
    let id = at::create_rule(&s.pool, &b, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "notification_rule.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "provider": b.provider }),
    )
    .await?;
    // Generic webhooks are signed by default when an encryption key exists;
    // the secret is shown once.
    let secret = if b.provider == "webhook" && s.secret_box.is_some() {
        Some(set_secret(&s, id).await?)
    } else {
        None
    };
    Ok(Json(json!({ "id": id, "secret": secret })))
}

#[utoipa::path(put, path = "/api/v1/admin/notification-rules/{id}", request_body = RuleBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::NotificationsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RuleBody>,
) -> ApiResult<Json<Value>> {
    at::update_rule(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "notification_rule.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/notification-rules/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::NotificationsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    at::delete_rule(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "notification_rule.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PreviewBody {
    pub provider: String,
    pub event: String,
}

/// Payload that would be POSTed for an event (no network call).
#[utoipa::path(post, path = "/api/v1/admin/notification-rules/preview", request_body = PreviewBody, responses((status = 200)))]
pub async fn preview(
    RequirePermission(_u, _): RequirePermission<perms::NotificationsManage>,
    Json(b): Json<PreviewBody>,
) -> ApiResult<Json<Value>> {
    Ok(Json(at::payload(
        &b.provider,
        &b.event,
        "Example notification from PeltyCMS",
        None,
    )))
}

/// Client summary page (all tabs) for one customer.
#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/summary", responses((status = 200), (status = 403), (status = 404)))]
pub async fn customer_summary(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        crate::routes::customer_workspace::legacy_summary(&s, &u, id).await?,
    ))
}

async fn set_secret(s: &AppState, id: Uuid) -> ApiResult<String> {
    let sb = s
        .secret_box
        .as_ref()
        .ok_or(pelty_core::AppError::Validation(
            "error.encryption_key_missing",
        ))?;
    let secret = crate::plugin_hooks::new_secret();
    at::set_rule_secret(&s.pool, id, Some(&sb.seal(secret.as_bytes()))).await?;
    Ok(secret)
}

/// Rotate the HMAC signing secret of a generic webhook; returned once.
#[utoipa::path(post, path = "/api/v1/admin/notification-rules/{id}/secret", responses((status = 200), (status = 404), (status = 422)))]
pub async fn rotate_secret(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::NotificationsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let r = at::rule(&s.pool, id)
        .await?
        .ok_or(pelty_core::AppError::NotFound)?;
    if r.provider != "webhook" {
        return Err(pelty_core::AppError::Validation("error.validation").into());
    }
    let secret = set_secret(&s, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "notification_rule.secret_rotated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "secret": secret })))
}

/// Stop signing a webhook.
#[utoipa::path(delete, path = "/api/v1/admin/notification-rules/{id}/secret", responses((status = 200), (status = 404)))]
pub async fn clear_secret(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::NotificationsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    at::set_rule_secret(&s.pool, id, None).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "notification_rule.secret_cleared",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Delivery log for one rule (latest 100 attempts' outcome).
#[utoipa::path(get, path = "/api/v1/admin/notification-rules/{id}/deliveries", responses((status = 200), (status = 403)))]
pub async fn rule_deliveries(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::NotificationsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": at::deliveries(&s.pool, id, 100).await? }),
    ))
}

/// Queue a test delivery through the real channel.
#[utoipa::path(post, path = "/api/v1/admin/notification-rules/{id}/test", responses((status = 200), (status = 404)))]
pub async fn test_rule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::NotificationsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let r = at::rule(&s.pool, id)
        .await?
        .ok_or(pelty_core::AppError::NotFound)?;
    let did = crate::notify::enqueue(
        &s,
        &r,
        "test",
        "PeltyCMS test notification",
        &json!({ "test": true }),
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "notification_rule.tested",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "delivery_id": did })))
}

#[utoipa::path(get, path = "/api/v1/admin/noticeboard", responses((status = 200), (status = 403)))]
pub async fn list_notices(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TodosManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": pelty_database::noticeboard::list(&s.pool, 100).await? }),
    ))
}

#[derive(Deserialize, serde::Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NoticeBody {
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/noticeboard", request_body = NoticeBody, responses((status = 200), (status = 422)))]
pub async fn create_notice(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TodosManage>,
    Json(b): Json<NoticeBody>,
) -> ApiResult<Json<Value>> {
    let id = pelty_database::noticeboard::create(&s.pool, u.user_id, &b.body, b.pinned).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "noticeboard.posted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "pinned": b.pinned }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

/// Authors delete their own notices; staff with settings access can delete any.
#[utoipa::path(delete, path = "/api/v1/admin/noticeboard/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_notice(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TodosManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    pelty_database::noticeboard::delete(&s.pool, id, u.user_id, u.has("settings.manage")).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "noticeboard.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
