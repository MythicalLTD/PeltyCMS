//! Support desk extras (`tickets.manage`): canned replies, tags, merge/split,
//! escalation rules, SLA metrics, staff list; client ratings.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::support::{self, EscalationRule};
use pelty_database::tickets::PRIORITIES;
use pelty_database::{audit, jobs};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub const ESCALATE_JOB: &str = "tickets.escalate";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/tickets/{id}/rate", post(client_rate))
        .route("/admin/ticket-canned", get(canned_list).post(canned_create))
        .route("/admin/ticket-satisfaction", get(satisfaction))
        .route(
            "/admin/ticket-canned/{id}",
            put(canned_update).delete(canned_delete),
        )
        .route("/admin/ticket-staff", get(staff))
        .route("/admin/ticket-sla", get(sla))
        .route("/admin/ticket-departments-sla", get(department_slas))
        .route(
            "/admin/ticket-departments/{id}/sla",
            put(set_department_sla),
        )
        .route("/admin/ticket-escalations", get(rules).post(rule_create))
        .route("/admin/ticket-escalations/run", post(rules_run))
        .route(
            "/admin/settings/ticket-auto-close",
            get(auto_close_get).put(auto_close_put),
        )
        .route(
            "/admin/ticket-escalations/{id}",
            put(rule_update).delete(rule_delete),
        )
        .route("/admin/tickets/{id}/tags", put(set_tags))
        .route("/admin/tickets/{id}/merge", post(merge))
        .route("/admin/tickets/{id}/split", post(split))
}

fn text(v: &str, max: usize, code: &'static str) -> ApiResult<String> {
    let t = v.trim();
    if t.is_empty() || t.chars().count() > max {
        return Err(AppError::Validation(code).into());
    }
    Ok(t.to_string())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RateBody {
    pub rating: i16,
    #[serde(default)]
    pub comment: String,
}

#[utoipa::path(post, path = "/api/v1/client/tickets/{id}/rate", request_body = RateBody, responses((status = 200), (status = 422)))]
pub async fn client_rate(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<RateBody>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    if !(1..=5).contains(&b.rating) || b.comment.chars().count() > 2000 {
        return Err(AppError::Validation("error.invalid_rating").into());
    }
    support::rate(&s.pool, id, c, b.rating, b.comment.trim()).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DepQuery {
    pub department_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-canned", params(DepQuery), responses((status = 200)))]
pub async fn canned_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Query(q): Query<DepQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": support::canned(&s.pool, q.department_id).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CannedBody {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub department_id: Option<Uuid>,
    /// Free-text grouping shown in the reply picker (WHMCS categories).
    #[serde(default)]
    pub category: String,
}

async fn save_canned(s: &AppState, id: Option<Uuid>, b: CannedBody) -> ApiResult<Json<Value>> {
    let title = text(&b.title, 150, "error.invalid_title")?;
    let body = text(&b.body, 20000, "error.invalid_message")?;
    if b.category.chars().count() > 100 {
        return Err(AppError::Validation("error.invalid_category").into());
    }
    let r =
        support::upsert_canned(&s.pool, id, &title, &body, b.department_id, &b.category).await?;
    Ok(Json(json!(r)))
}

#[utoipa::path(post, path = "/api/v1/admin/ticket-canned", request_body = CannedBody, responses((status = 200), (status = 422)))]
pub async fn canned_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<CannedBody>,
) -> ApiResult<Json<Value>> {
    let out = save_canned(&s, None, b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.canned_created",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(out)
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-canned/{id}", request_body = CannedBody, responses((status = 200), (status = 404)))]
pub async fn canned_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CannedBody>,
) -> ApiResult<Json<Value>> {
    let out = save_canned(&s, Some(id), b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.canned_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(out)
}

#[utoipa::path(delete, path = "/api/v1/admin/ticket-canned/{id}", responses((status = 200), (status = 404)))]
pub async fn canned_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    support::delete_canned(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.canned_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-staff", responses((status = 200)))]
pub async fn staff(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": support::staff(&s.pool).await? })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SlaQuery {
    pub days: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-sla", params(SlaQuery), responses((status = 200)))]
pub async fn sla(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Query(q): Query<SlaQuery>,
) -> ApiResult<Json<Value>> {
    let days = q.days.unwrap_or(30).clamp(1, 3650);
    let since = OffsetDateTime::now_utc() - time::Duration::days(days);
    Ok(Json(
        json!({ "days": days, "items": support::sla(&s.pool, since).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SatisfactionQuery {
    pub days: Option<i64>,
    pub department_id: Option<Uuid>,
    #[serde(default)]
    pub low: bool,
}

/// CSAT report: distribution, per department / staff, recent comments.
#[utoipa::path(get, path = "/api/v1/admin/ticket-satisfaction", params(SatisfactionQuery), responses((status = 200), (status = 403)))]
pub async fn satisfaction(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Query(q): Query<SatisfactionQuery>,
) -> ApiResult<Json<Value>> {
    let days = q.days.unwrap_or(30).clamp(1, 3650);
    let since = OffsetDateTime::now_utc() - time::Duration::days(days);
    let r = pelty_database::satisfaction::report(&s.pool, since, q.department_id, q.low).await?;
    Ok(Json(json!({ "days": days, "report": r })))
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-departments-sla", responses((status = 200)))]
pub async fn department_slas(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": support::department_slas(&s.pool).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DepartmentSlaBody {
    #[serde(default)]
    pub email: String,
    pub sla_first_response_minutes: Option<i32>,
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-departments/{id}/sla", request_body = DepartmentSlaBody, responses((status = 200), (status = 422)))]
pub async fn set_department_sla(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<DepartmentSlaBody>,
) -> ApiResult<Json<Value>> {
    let email = b.email.trim();
    if !email.is_empty() && (email.len() > 254 || !email.contains('@')) {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    if b.sla_first_response_minutes.is_some_and(|m| m <= 0) {
        return Err(AppError::Validation("error.invalid_sla").into());
    }
    support::set_department_sla(&s.pool, id, email, b.sla_first_response_minutes).await?;
    pelty_database::ticket_sla::sync_department_first_response(
        &s.pool,
        id,
        b.sla_first_response_minutes,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.department_sla_set",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RuleBody {
    pub name: String,
    pub department_id: Option<Uuid>,
    #[serde(default)]
    pub statuses: Vec<String>,
    pub priority: Option<String>,
    pub after_minutes: i32,
    pub set_priority: Option<String>,
    pub set_department: Option<Uuid>,
    pub assign_to: Option<Uuid>,
    pub add_tag: Option<String>,
    #[serde(default = "yes")]
    pub is_active: bool,
}

fn yes() -> bool {
    true
}

fn prio_ok(p: &Option<String>) -> ApiResult<()> {
    match p {
        Some(p) if !PRIORITIES.contains(&p.as_str()) => {
            Err(AppError::Validation("error.invalid_priority").into())
        }
        _ => Ok(()),
    }
}

async fn save_rule(s: &AppState, id: Option<Uuid>, b: RuleBody) -> ApiResult<Json<Value>> {
    let name = text(&b.name, 100, "error.invalid_name")?;
    let statuses = if b.statuses.is_empty() {
        vec!["open".to_string(), "customer_reply".to_string()]
    } else {
        b.statuses
    };
    for x in &statuses {
        if x == "closed" || !pelty_database::ticket_statuses::exists(&s.pool, x).await? {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    prio_ok(&b.priority)?;
    prio_ok(&b.set_priority)?;
    if !(1..=525_600).contains(&b.after_minutes) {
        return Err(AppError::Validation("error.invalid_minutes").into());
    }
    let add_tag = match b
        .add_tag
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(t) => {
            Some(support::normalize_tag(t).ok_or(AppError::Validation("error.invalid_tag"))?)
        }
        None => None,
    };
    if let Some(a) = b.assign_to {
        if !support::staff(&s.pool).await?.iter().any(|x| x.id == a) {
            return Err(AppError::Validation("error.invalid_assignee").into());
        }
    }
    let r = EscalationRule {
        id: id.unwrap_or_default(),
        name,
        department_id: b.department_id,
        statuses,
        priority: b.priority,
        after_minutes: b.after_minutes,
        set_priority: b.set_priority,
        set_department: b.set_department,
        assign_to: b.assign_to,
        add_tag,
        is_active: b.is_active,
    };
    Ok(Json(json!(support::upsert_rule(&s.pool, id, &r).await?)))
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-escalations", responses((status = 200)))]
pub async fn rules(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": support::rules(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/ticket-escalations", request_body = RuleBody, responses((status = 200), (status = 422)))]
pub async fn rule_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<RuleBody>,
) -> ApiResult<Json<Value>> {
    let out = save_rule(&s, None, b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.escalation_rule_created",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(out)
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-escalations/{id}", request_body = RuleBody, responses((status = 200), (status = 404)))]
pub async fn rule_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RuleBody>,
) -> ApiResult<Json<Value>> {
    let out = save_rule(&s, Some(id), b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.escalation_rule_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(out)
}

#[utoipa::path(delete, path = "/api/v1/admin/ticket-escalations/{id}", responses((status = 200), (status = 404)))]
pub async fn rule_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    support::delete_rule(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.escalation_rule_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Enqueue an escalation pass now (durable job; the worker also schedules one
/// every few minutes).
#[utoipa::path(post, path = "/api/v1/admin/ticket-escalations/run", responses((status = 200)))]
pub async fn rules_run(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    let id = jobs::enqueue(
        &s.pool,
        ESCALATE_JOB,
        json!({ "manual": true }),
        None,
        None,
        3,
    )
    .await?;
    Ok(Json(json!({ "job_id": id })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TagsBody {
    pub tags: Vec<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/tickets/{id}/tags", request_body = TagsBody, responses((status = 200), (status = 422)))]
pub async fn set_tags(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TagsBody>,
) -> ApiResult<Json<Value>> {
    s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    if b.tags.len() > 20 {
        return Err(AppError::Validation("error.invalid_tag").into());
    }
    let mut tags = b
        .tags
        .iter()
        .map(|t| support::normalize_tag(t).ok_or(AppError::Validation("error.invalid_tag")))
        .collect::<Result<Vec<_>, _>>()?;
    tags.sort();
    tags.dedup();
    support::set_tags(&s.pool, id, &tags).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.tags_set",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "tags": tags }),
    )
    .await?;
    Ok(Json(json!({ "tags": tags })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MergeBody {
    /// Ticket that receives this ticket's messages.
    pub target_id: Uuid,
}

#[utoipa::path(post, path = "/api/v1/admin/tickets/{id}/merge", request_body = MergeBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn merge(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<MergeBody>,
) -> ApiResult<Json<Value>> {
    support::merge(&s.pool, id, b.target_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.merged",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "into": b.target_id }),
    )
    .await?;
    Ok(Json(json!({ "ticket_id": b.target_id })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SplitBody {
    pub message_ids: Vec<Uuid>,
    pub subject: String,
}

#[utoipa::path(post, path = "/api/v1/admin/tickets/{id}/split", request_body = SplitBody, responses((status = 200), (status = 422)))]
pub async fn split(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<SplitBody>,
) -> ApiResult<Json<Value>> {
    let subject = text(&b.subject, 200, "error.invalid_subject")?;
    let mut ids = b.message_ids;
    ids.sort();
    ids.dedup();
    let new = support::split(&s.pool, id, &ids, &subject).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.split",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "new_ticket": new }),
    )
    .await?;
    Ok(Json(json!({ "ticket_id": new })))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/ticket-auto-close", responses((status = 200), (status = 403)))]
pub async fn auto_close_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(
        pelty_database::support::get_auto_close(&s.pool).await?
    )))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/ticket-auto-close", request_body = pelty_database::support::AutoCloseSettings, responses((status = 200), (status = 422)))]
pub async fn auto_close_put(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<pelty_database::support::AutoCloseSettings>,
) -> ApiResult<Json<Value>> {
    b.validate().map_err(AppError::Validation)?;
    pelty_database::support::put_auto_close(&s.pool, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.ticket_auto_close",
        None,
        u.ip.as_deref(),
        json!({ "hours": b.hours }),
    )
    .await?;
    Ok(Json(json!(b)))
}

/// Close idle tickets and mail each customer (`ticket_auto_closed`).
pub async fn run_auto_close(s: &AppState) -> pelty_core::AppResult<usize> {
    let cfg = pelty_database::support::get_auto_close(&s.pool).await?;
    let ids = pelty_database::support::close_inactive(&s.pool, cfg.hours, &cfg.statuses).await?;
    for id in &ids {
        if let Some(t) = s.tickets().get(*id, None).await? {
            if let Err(e) =
                crate::routes::tickets::notify_customer_with(s, &t, "ticket_auto_closed").await
            {
                tracing::warn!(error = ?e.0, ticket = %id, "auto-close mail failed");
            }
            s.emit(pelty_extensions::events::DomainEvent::new(
                "ticket.closed",
                json!({ "ticket_id": id, "reason": "inactive" }),
            ));
        }
    }
    Ok(ids.len())
}
