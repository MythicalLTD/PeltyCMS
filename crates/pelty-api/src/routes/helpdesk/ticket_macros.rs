//! Ticket macros: manage them, list the ones offered on a ticket (with the
//! reply already filled in), and apply one.
use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::ticket_macros::{self, Macro, MacroInput};
use pelty_database::{audit, tickets, users};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::routes::tickets::{admin_get, admin_reply, admin_update, ReplyBody, UpdateBody};
use crate::{
    error::ApiResult,
    extract::{perms, CurrentUser, RequirePermission},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/ticket-macros", get(list).post(create))
        .route("/admin/ticket-macros/{id}", put(update).delete(remove))
        .route("/admin/tickets/{id}/macros", get(for_ticket))
        .route("/admin/tickets/{id}/macros/{macro_id}", post(apply))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    /// Only active macros offered in this department.
    pub department_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-macros", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": ticket_macros::list(&s.pool, q.department_id).await? }),
    ))
}

/// Status must exist; a fixed assignee must be active staff.
async fn check(s: &AppState, b: MacroInput) -> ApiResult<MacroInput> {
    let b = b.normalize().map_err(AppError::Validation)?;
    if let Some(st) = b.set_status.as_deref() {
        if !pelty_database::ticket_statuses::exists(&s.pool, st).await? {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    if let Some(a) = b.assign_to {
        let staff = users::find(&s.pool, a).await?.is_some_and(|x| x.is_active)
            && users::permissions(&s.pool, a)
                .await?
                .iter()
                .any(|p| p == "tickets.manage");
        if !staff {
            return Err(AppError::Validation("error.invalid_assignee").into());
        }
    }
    Ok(b)
}

#[utoipa::path(post, path = "/api/v1/admin/ticket-macros", request_body = MacroInput, responses((status = 200, body = Macro), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<MacroInput>,
) -> ApiResult<Json<Macro>> {
    let b = check(&s, b).await?;
    let m = ticket_macros::create(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.macro_created",
        Some(&m.id.to_string()),
        u.ip.as_deref(),
        json!({ "name": m.name }),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-macros/{id}", request_body = MacroInput, responses((status = 200, body = Macro), (status = 404), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<MacroInput>,
) -> ApiResult<Json<Macro>> {
    let b = check(&s, b).await?;
    let m = ticket_macros::update(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.macro_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": m.name }),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(delete, path = "/api/v1/admin/ticket-macros/{id}", responses((status = 200), (status = 404)))]
pub async fn remove(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    ticket_macros::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.macro_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// A macro as offered on one ticket, with its reply filled in.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Offered {
    #[serde(flatten)]
    pub r#macro: Macro,
    pub rendered_body: String,
}

async fn rendered(s: &AppState, t: &tickets::Ticket, staff: Uuid, body: &str) -> ApiResult<String> {
    if body.is_empty() {
        return Ok(String::new());
    }
    let me = users::find(&s.pool, staff).await?;
    let staff_name = me
        .as_ref()
        .map(|u| {
            if u.display_name.trim().is_empty() {
                u.email.as_str()
            } else {
                u.display_name.as_str()
            }
        })
        .unwrap_or("");
    Ok(ticket_macros::render(
        body,
        &ticket_macros::Vars {
            customer_name: &t.customer_name,
            customer_email: &t.customer_email,
            ticket_number: t.number,
            ticket_subject: &t.subject,
            department: &t.department_name,
            staff_name,
        },
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/tickets/{id}/macros", responses((status = 200), (status = 404)))]
pub async fn for_ticket(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let t = s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    let mut items = Vec::new();
    for m in ticket_macros::list(&s.pool, Some(t.department_id)).await? {
        let rendered_body = rendered(&s, &t, u.user_id, &m.reply_body).await?;
        items.push(Offered {
            r#macro: m,
            rendered_body,
        });
    }
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyBody {
    /// The reply as edited by staff; omit to send the macro's own text.
    #[serde(default)]
    pub body: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/tickets/{id}/macros/{macro_id}", request_body = ApplyBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn apply(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path((id, macro_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ApplyBody>>,
) -> ApiResult<Json<Value>> {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let t = s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    let m = ticket_macros::get(&s.pool, macro_id).await?;
    if !m.is_active || m.department_id.is_some_and(|d| d != t.department_id) {
        return Err(AppError::Validation("error.macro_unavailable").into());
    }
    if t.merged_into.is_some() {
        return Err(AppError::Validation("error.ticket_merged").into());
    }
    // Fail before replying if a changed setting no longer applies.
    if let Some(st) = m.set_status.as_deref() {
        if !pelty_database::ticket_statuses::exists(&s.pool, st).await? {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    let perm = |u: &CurrentUser| RequirePermission::<perms::TicketsManage>(u.clone(), PhantomData);
    // Reply first so a "reply and close" macro ends closed.
    let reply = match b.body {
        Some(x) if !m.reply_body.is_empty() => x,
        Some(_) => String::new(),
        None => rendered(&s, &t, u.user_id, &m.reply_body).await?,
    };
    let replied = !reply.trim().is_empty();
    if replied {
        let _ = admin_reply(
            State(s.clone()),
            perm(&u),
            Path(id),
            Json(ReplyBody {
                body: reply,
                internal: m.reply_internal,
                attachment_ids: Vec::new(),
            }),
        )
        .await?;
    }
    let assigned_to = match m.assign_mode.as_str() {
        "self" => Some(Some(u.user_id)),
        "user" => m.assign_to.map(Some),
        "unassign" => Some(None),
        _ => None,
    };
    let patch = UpdateBody {
        status: m.set_status.clone(),
        priority: m.set_priority.clone(),
        department_id: m.set_department,
        assigned_to,
    };
    if patch.status.is_some()
        || patch.priority.is_some()
        || patch.department_id.is_some()
        || patch.assigned_to.is_some()
    {
        let _ = admin_update(State(s.clone()), perm(&u), Path(id), Json(patch)).await?;
    }
    ticket_macros::add_tags(&s.pool, id, &m.add_tags).await?;
    ticket_macros::count_use(&s.pool, m.id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.macro_applied",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "macro_id": m.id, "name": m.name, "replied": replied }),
    )
    .await?;
    admin_get(State(s.clone()), perm(&u), Path(id)).await
}
