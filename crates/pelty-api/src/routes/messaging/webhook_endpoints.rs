//! Admin: outgoing webhook endpoints, their delivery log and redelivery
//! (`webhooks.manage`). See [`crate::webhooks_out`] for the delivery rules.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::webhooks_out;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, webhook_endpoints as db};
use pelty_extensions::events::{EventKind, REGISTRY};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const MAX_ENDPOINTS: usize = 50;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/webhook-endpoints", get(list).post(create))
        .route("/admin/webhook-endpoints/{id}", put(update).delete(remove))
        .route("/admin/webhook-endpoints/{id}/secret", post(rotate_secret))
        .route("/admin/webhook-endpoints/{id}/test", post(test))
        .route("/admin/webhook-deliveries", get(deliveries))
        .route("/admin/webhook-deliveries/{id}", get(delivery))
        .route("/admin/webhook-deliveries/{id}/redeliver", post(redeliver))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct EndpointBody {
    pub name: String,
    /// HTTPS URL (private / loopback addresses are refused at send time).
    pub url: String,
    /// Event names, `<category>.*` or `*`.
    pub events: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Domain events endpoints can subscribe to (the event registry).
fn catalog() -> Vec<Value> {
    REGISTRY
        .iter()
        .filter(|e| e.kind == EventKind::Domain && !e.name.starts_with("webhook_endpoint."))
        .map(|e| json!({ "name": e.name, "category": e.category(), "description": e.description }))
        .collect()
}

/// Validate and normalise a body.
pub fn validate(b: &EndpointBody) -> Result<(String, String, Vec<String>), AppError> {
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::Validation("error.invalid_name"));
    }
    let url = b.url.trim();
    let host_ok = url.strip_prefix("https://").and_then(|r| r.split(['/', '?', '#']).next()).is_some_and(|h| !h.is_empty() && !h.contains('@'));
    if !host_ok || url.len() > 500 || url.contains(char::is_whitespace) {
        return Err(AppError::Validation("error.invalid_webhook_url"));
    }
    let mut events: Vec<String> = b.events.iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
    events.sort();
    events.dedup();
    let known = |e: &str| {
        e == "*"
            || e.strip_suffix(".*").is_some_and(|c| REGISTRY.iter().any(|x| x.kind == EventKind::Domain && x.category() == c))
            || REGISTRY.iter().any(|x| x.kind == EventKind::Domain && x.name == e && !e.starts_with("webhook_endpoint."))
    };
    if events.is_empty() || events.len() > 200 || events.iter().any(|e| !known(e)) {
        return Err(AppError::Validation("error.invalid_events"));
    }
    Ok((name.to_string(), url.to_string(), events))
}

fn seal(s: &AppState, secret: &str) -> Result<Vec<u8>, AppError> {
    let sb = s.secret_box.as_ref().ok_or(AppError::Validation("error.encryption_key_missing"))?;
    Ok(sb.seal(secret.as_bytes()))
}

#[utoipa::path(get, path = "/api/v1/admin/webhook-endpoints", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: RequirePermission<perms::WebhooksManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": db::list(&s.pool).await?, "events": catalog(), "signing_available": s.secret_box.is_some() })))
}

/// Create an endpoint. The signing secret is returned once.
#[utoipa::path(post, path = "/api/v1/admin/webhook-endpoints", request_body = EndpointBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::WebhooksManage>,
    Json(b): Json<EndpointBody>,
) -> ApiResult<Json<Value>> {
    let (name, url, events) = validate(&b)?;
    if db::list(&s.pool).await?.len() >= MAX_ENDPOINTS {
        return Err(AppError::Conflict("error.too_many_webhooks").into());
    }
    let secret = webhooks_out::new_secret();
    let ep = db::create(&s.pool, &name, &url, &events, &seal(&s, &secret)?, b.enabled, u.user_id).await?;
    audit::record(&s.pool, Some(u.user_id), "webhook_endpoint.created", Some(&ep.id.to_string()), u.ip.as_deref(), json!({ "url": url, "events": events })).await?;
    Ok(Json(json!({ "endpoint": ep, "secret": secret })))
}

#[utoipa::path(put, path = "/api/v1/admin/webhook-endpoints/{id}", request_body = EndpointBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::WebhooksManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<EndpointBody>,
) -> ApiResult<Json<Value>> {
    let (name, url, events) = validate(&b)?;
    let ep = db::update(&s.pool, id, &name, &url, &events, b.enabled).await?;
    audit::record(&s.pool, Some(u.user_id), "webhook_endpoint.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "url": url, "events": events, "enabled": b.enabled })).await?;
    Ok(Json(json!({ "endpoint": ep })))
}

#[utoipa::path(delete, path = "/api/v1/admin/webhook-endpoints/{id}", responses((status = 204), (status = 404)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::WebhooksManage>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    if !db::delete(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "webhook_endpoint.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Rotate the signing secret; the new one is returned once.
#[utoipa::path(post, path = "/api/v1/admin/webhook-endpoints/{id}/secret", responses((status = 200), (status = 404), (status = 422)))]
pub async fn rotate_secret(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::WebhooksManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let secret = webhooks_out::new_secret();
    db::set_secret(&s.pool, id, &seal(&s, &secret)?).await?;
    audit::record(&s.pool, Some(u.user_id), "webhook_endpoint.secret_rotated", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "secret": secret })))
}

/// Queue a `webhook.ping` delivery (also to a disabled endpoint).
#[utoipa::path(post, path = "/api/v1/admin/webhook-endpoints/{id}/test", responses((status = 200), (status = 404)))]
pub async fn test(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::WebhooksManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let did = webhooks_out::enqueue(&s, id, webhooks_out::PING_EVENT, &json!({ "endpoint_id": id, "by": u.user_id }), None).await?;
    Ok(Json(json!({ "delivery_id": did })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DeliveryQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub endpoint_id: Option<Uuid>,
    /// `pending`, `delivered` or `failed`.
    pub status: Option<String>,
    pub event: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/webhook-deliveries", params(DeliveryQuery), responses((status = 200), (status = 403)))]
pub async fn deliveries(State(s): State<AppState>, _p: RequirePermission<perms::WebhooksManage>, Query(q): Query<DeliveryQuery>) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = crate::routes::Page { page: q.page, per_page: q.per_page }.bounds();
    let status = q.status.as_deref().filter(|x| ["pending", "delivered", "failed"].contains(x));
    let event = q.event.as_deref().map(str::trim).filter(|x| !x.is_empty());
    let (items, total) = db::deliveries(&s.pool, q.endpoint_id, status, event, limit, offset).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/webhook-deliveries/{id}", responses((status = 200), (status = 404)))]
pub async fn delivery(State(s): State<AppState>, _p: RequirePermission<perms::WebhooksManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "delivery": db::delivery(&s.pool, id).await?.ok_or(AppError::NotFound)? })))
}

/// Send the same event and payload again as a new delivery.
#[utoipa::path(post, path = "/api/v1/admin/webhook-deliveries/{id}/redeliver", responses((status = 200), (status = 404)))]
pub async fn redeliver(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::WebhooksManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let new_id = webhooks_out::redeliver(&s, id).await?;
    audit::record(&s.pool, Some(u.user_id), "webhook_delivery.redelivered", Some(&id.to_string()), u.ip.as_deref(), json!({ "new_delivery": new_id })).await?;
    Ok(Json(json!({ "delivery_id": new_id })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(url: &str, events: &[&str]) -> EndpointBody {
        EndpointBody { name: "x".into(), url: url.into(), events: events.iter().map(|e| e.to_string()).collect(), enabled: true }
    }

    #[test]
    fn validation() {
        assert!(validate(&body("https://hooks.example.com/x", &["invoice.paid", "ticket.*", "*"])).is_ok());
        assert!(validate(&body("http://hooks.example.com/x", &["*"])).is_err());
        assert!(validate(&body("https://user@evil/x", &["*"])).is_err());
        assert!(validate(&body("https://", &["*"])).is_err());
        assert!(validate(&body("https://a.example", &[])).is_err());
        assert!(validate(&body("https://a.example", &["nope.event"])).is_err());
        assert!(validate(&body("https://a.example", &["nope.*"])).is_err());
        assert!(validate(&body("https://a.example", &["webhook_endpoint.disabled"])).is_err());
        let (_, _, ev) = validate(&body(" https://a.example ", &["order.created", "order.created", "invoice.*"])).unwrap();
        assert_eq!(ev, vec!["invoice.*", "order.created"]);
    }
}
