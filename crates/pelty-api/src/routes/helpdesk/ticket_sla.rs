//! Ticket SLA policies (department × priority), live SLA state, the
//! at-risk list and the warning / breach scan.
use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, ticket_sla};
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{error::ApiResult, extract::{perms, RequirePermission}, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/ticket-sla/policies", get(list).post(create))
        .route("/admin/ticket-sla/policies/{id}", put(update).delete(remove))
        .route("/admin/ticket-sla/at-risk", get(at_risk))
        .route("/admin/tickets/{id}/sla", get(ticket_sla))
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-sla/policies", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: RequirePermission<perms::TicketsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": ticket_sla::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/ticket-sla/policies", request_body = ticket_sla::PolicyInput, responses((status = 200, body = ticket_sla::Policy), (status = 409), (status = 422)))]
pub async fn create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::TicketsManage>, Json(b): Json<ticket_sla::PolicyInput>) -> ApiResult<Json<ticket_sla::Policy>> {
    b.validate().map_err(AppError::Validation)?;
    let p = ticket_sla::create(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "ticket.sla_policy_created", Some(&p.id.to_string()), u.ip.as_deref(), json!(p)).await?;
    Ok(Json(p))
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-sla/policies/{id}", request_body = ticket_sla::PolicyInput, responses((status = 200, body = ticket_sla::Policy), (status = 404), (status = 409), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ticket_sla::PolicyInput>,
) -> ApiResult<Json<ticket_sla::Policy>> {
    b.validate().map_err(AppError::Validation)?;
    let p = ticket_sla::update(&s.pool, id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "ticket.sla_policy_updated", Some(&id.to_string()), u.ip.as_deref(), json!(p)).await?;
    Ok(Json(p))
}

#[utoipa::path(delete, path = "/api/v1/admin/ticket-sla/policies/{id}", responses((status = 200), (status = 404)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::TicketsManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    ticket_sla::delete(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "ticket.sla_policy_deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AtRiskQuery {
    pub department_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-sla/at-risk", params(AtRiskQuery), responses((status = 200), (status = 403)))]
pub async fn at_risk(State(s): State<AppState>, _p: RequirePermission<perms::TicketsManage>, Query(q): Query<AtRiskQuery>) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    Ok(Json(json!({ "items": ticket_sla::at_risk(&s.pool, q.department_id, limit).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/tickets/{id}/sla", responses((status = 200, body = ticket_sla::TicketSla), (status = 404)))]
pub async fn ticket_sla(State(s): State<AppState>, _p: RequirePermission<perms::TicketsManage>, Path(id): Path<Uuid>) -> ApiResult<Json<ticket_sla::TicketSla>> {
    Ok(Json(ticket_sla::for_ticket(&s.pool, id).await?))
}

/// Scan for SLA warnings and breaches; runs inside the ticket escalation job.
/// Each fires `ticket.sla_warning` / `ticket.sla_breached` (route them to
/// Slack, e-mail or webhooks in the notification settings).
pub async fn run_scan(s: &AppState) -> pelty_core::AppResult<usize> {
    let fired = ticket_sla::scan(&s.pool).await?;
    for f in &fired {
        let data = json!({
            "ticket_id": f.ticket_id, "number": f.number, "subject": f.subject, "target": f.target,
            "due_at": f.due_at.unix_timestamp(), "assigned_to": f.assigned_to, "escalated": f.escalated,
        });
        if f.stage == "breach" {
            s.emit(DomainEvent::new("ticket.sla_breached", data));
        } else {
            s.emit(DomainEvent::new("ticket.sla_warning", data));
        }
        if f.stage == "breach" {
            audit::record(&s.pool, None, "ticket.sla_breached", Some(&f.ticket_id.to_string()), None, json!({ "target": f.target, "escalated": f.escalated })).await?;
        }
    }
    Ok(fired.len())
}
