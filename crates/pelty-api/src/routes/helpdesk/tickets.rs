//! Support tickets: client area (own tickets only, no internal notes) and
//! staff desk (`tickets.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_for;
use crate::routes::{paged, ListQuery};
use crate::{mail, AppState};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::tickets::{self, NewTicket, PRIORITIES};
use pelty_database::{audit, users};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/ticket-departments", get(client_departments))
        .route("/client/tickets", get(client_list).post(client_open))
        .route("/client/tickets/{id}", get(client_get))
        .route("/client/tickets/{id}/reply", post(client_reply))
        .route("/client/tickets/{id}/close", post(client_close))
        .route(
            "/admin/ticket-departments",
            get(admin_departments).post(admin_create_department),
        )
        .route(
            "/admin/ticket-departments/{id}",
            put(admin_update_department),
        )
        .route("/admin/tickets", get(admin_list).post(admin_open))
        .route("/admin/tickets/{id}", get(admin_get).patch(admin_update))
        .route("/admin/tickets/{id}/reply", post(admin_reply))
        .route(
            "/admin/tickets/{id}/messages/{mid}",
            put(admin_edit_message).delete(admin_delete_message),
        )
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EditMessage {
    pub body: String,
}

#[utoipa::path(put, path = "/api/v1/admin/tickets/{id}/messages/{mid}", request_body = EditMessage, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_edit_message(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path((id, mid)): Path<(Uuid, Uuid)>,
    Json(b): Json<EditMessage>,
) -> ApiResult<Json<Value>> {
    let old = s.tickets().edit_message(id, mid, &b.body).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.message_edited",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "message_id": mid, "previous_length": old.chars().count() }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/tickets/{id}/messages/{mid}", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_delete_message(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path((id, mid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    s.tickets().delete_message(id, mid).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.message_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "message_id": mid }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OpenBody {
    pub department_id: Uuid,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub service_id: Option<Uuid>,
    /// Staff only: open on behalf of this customer.
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    /// Ids returned by the attachment upload endpoint.
    #[serde(default)]
    pub attachment_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReplyBody {
    pub body: String,
    /// Staff only: note invisible to the customer.
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub attachment_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateBody {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub department_id: Option<Uuid>,
    /// `null` unassigns; omit to keep.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>)]
    pub assigned_to: Option<Option<Uuid>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DepartmentBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "yes")]
    pub is_active: bool,
    #[serde(default)]
    pub sort_order: i32,
}
fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<Uuid>>, D::Error> {
    Option::<Uuid>::deserialize(d).map(Some)
}

fn yes() -> bool {
    true
}

fn text(v: &str, max: usize, code: &'static str) -> ApiResult<String> {
    let t = v.trim();
    if t.is_empty() || t.chars().count() > max {
        return Err(AppError::Validation(code).into());
    }
    Ok(t.to_string())
}

fn priority(p: Option<&str>) -> ApiResult<&'static str> {
    match PRIORITIES.iter().find(|x| **x == p.unwrap_or("medium")) {
        Some(p) => Ok(p),
        None => Err(AppError::Validation("error.invalid_priority").into()),
    }
}

/// `ticket.before_open`: addons may cancel, or rewrite subject, body,
/// priority and department. Returns `(subject, body, priority, department)`.
async fn pre_open(
    s: &AppState,
    cid: Uuid,
    b: &OpenBody,
    subject: String,
    body: String,
    by_staff: bool,
) -> ApiResult<(String, String, &'static str, Uuid)> {
    let pr = priority(b.priority.as_deref())?;
    let pre = crate::addon_ext::pre_event(s, "ticket.before_open", json!({ "customer_id": cid, "subject": subject, "body": body, "priority": pr, "department_id": b.department_id, "service_id": b.service_id, "by_staff": by_staff })).await?;
    let pr = pre
        .get("priority")
        .and_then(Value::as_str)
        .and_then(|p| PRIORITIES.iter().find(|x| **x == p))
        .copied()
        .unwrap_or(pr);
    Ok((
        crate::addon_ext::pre_str(&pre, "subject", 200, &subject),
        crate::addon_ext::pre_str(&pre, "body", 20000, &body),
        pr,
        crate::addon_ext::pre_uuid(&pre, "department_id", Some(b.department_id))
            .unwrap_or(b.department_id),
    ))
}

/// `ticket.before_reply`: addons may cancel or rewrite the message.
async fn pre_reply(
    s: &AppState,
    id: Uuid,
    body: String,
    by_staff: bool,
    internal: bool,
) -> ApiResult<String> {
    let pre = crate::addon_ext::pre_event(
        s,
        "ticket.before_reply",
        json!({ "ticket_id": id, "body": body, "by_staff": by_staff, "internal": internal }),
    )
    .await?;
    Ok(crate::addon_ext::pre_str(&pre, "body", 20000, &body))
}

async fn detail(s: &AppState, t: tickets::Ticket, staff: bool) -> ApiResult<Json<Value>> {
    let messages = s.tickets().messages(t.id, staff).await?;
    let files = pelty_database::ticket_attachments::for_ticket(&s.pool, t.id, staff).await?;
    let messages: Vec<Value> = messages
        .into_iter()
        .map(|m| {
            let mine: Vec<_> = files
                .iter()
                .filter(|a| a.message_id == Some(m.id))
                .collect();
            let mut v = json!(m);
            v["attachments"] = json!(mine);
            v
        })
        .collect();
    let tags = if staff {
        pelty_database::support::tags(&s.pool, t.id).await?
    } else {
        Vec::new()
    };
    Ok(Json(
        json!({ "ticket": t, "messages": messages, "tags": tags }),
    ))
}

/// Most recipients for one staff notification (unassigned tickets go to every
/// active `tickets.manage` user).
const STAFF_NOTIFY_CAP: usize = 25;

/// Mail staff about a new ticket or a customer reply: the assignee if any,
/// otherwise everyone who can manage tickets.
pub(crate) async fn notify_staff(
    s: &AppState,
    t: &tickets::Ticket,
    message: &str,
) -> ApiResult<()> {
    let staff = pelty_database::support::staff(&s.pool).await?;
    let watching = pelty_database::support_ext::watchers(&s.pool, t.id).await?;
    let to: Vec<_> = match t.assigned_to {
        Some(a) => staff
            .iter()
            .filter(|x| x.id == a || watching.contains(&x.id))
            .collect(),
        None => staff
            .iter()
            .take(STAFF_NOTIFY_CAP)
            .chain(staff.iter().filter(|x| watching.contains(&x.id)))
            .collect(),
    };
    let mut seen = std::collections::HashSet::new();
    let to: Vec<_> = to.into_iter().filter(|x| seen.insert(x.id)).collect();
    let link = format!("{}/admin/tickets/{}", s.cfg.public_url, t.id);
    let num = t.number.to_string();
    let excerpt: String = message.chars().take(500).collect();
    let customer = if t.customer_name.trim().is_empty() {
        t.customer_email.clone()
    } else {
        t.customer_name.clone()
    };
    for x in to {
        let name = if x.display_name.is_empty() {
            x.email.clone()
        } else {
            x.display_name.clone()
        };
        mail::enqueue(
            s,
            "ticket_staff_notify",
            &s.cfg.default_locale,
            &x.email,
            mail::vars(&[
                ("name", &name),
                ("ticket_number", &num),
                ("subject", &t.subject),
                ("customer", &customer),
                ("department", &t.department_name),
                ("message", &excerpt),
                ("link", &link),
            ]),
        )
        .await?;
    }
    Ok(())
}

/// Mail the customer's login (or billing e-mail) that staff replied.
async fn notify_customer(s: &AppState, t: &tickets::Ticket) -> ApiResult<()> {
    notify_customer_with(s, t, "ticket_reply").await
}

/// Mail the ticket's customer with a ticket template (`name`, `ticket_number`, `subject`, `link`).
pub async fn notify_customer_with(
    s: &AppState,
    t: &tickets::Ticket,
    template: &str,
) -> ApiResult<()> {
    let Some(customer) = t.customer_id else {
        let num = t.number.to_string();
        return mail::enqueue(
            s,
            template,
            &s.cfg.default_locale,
            &t.customer_email,
            mail::vars(&[
                ("name", &t.customer_name),
                ("ticket_number", &num),
                ("subject", &t.subject),
                ("link", &s.cfg.public_url),
            ]),
        )
        .await
        .map(|_| ())
        .map_err(Into::into);
    };
    let c = s.clients().get(customer).await?.ok_or(AppError::NotFound)?;
    let (to, locale, name) = match c.owner_user_id {
        Some(uid) => match users::find(&s.pool, uid).await? {
            Some(u) => (
                u.email.clone(),
                mail::user_locale(s, &u),
                if u.display_name.is_empty() {
                    u.email.clone()
                } else {
                    u.display_name.clone()
                },
            ),
            None => (
                c.email.clone(),
                s.cfg.default_locale.clone(),
                c.first_name.clone(),
            ),
        },
        None => (
            c.email.clone(),
            s.cfg.default_locale.clone(),
            c.first_name.clone(),
        ),
    };
    let link = format!("{}/tickets/{}", s.cfg.public_url, t.id);
    let num = t.number.to_string();
    mail::enqueue(
        s,
        template,
        &locale,
        &to,
        mail::vars(&[
            ("name", &name),
            ("ticket_number", &num),
            ("subject", &t.subject),
            ("link", &link),
        ]),
    )
    .await?;
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/client/ticket-departments", responses((status = 200)))]
pub async fn client_departments(
    State(s): State<AppState>,
    _u: CurrentUser,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": s.tickets().departments(true).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/tickets", params(ListQuery), responses((status = 200)))]
pub async fn client_list(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "tickets").await?;
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = s
        .tickets()
        .list(Some(cid), q.status(), None, limit, offset)
        .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/client/tickets", request_body = OpenBody, responses((status = 200), (status = 422)))]
pub async fn client_open(
    State(s): State<AppState>,
    u: CurrentUser,
    headers: axum::http::HeaderMap,
    Json(b): Json<OpenBody>,
) -> ApiResult<Json<Value>> {
    crate::captcha::require(&s, &headers, "ticket", u.ip.as_deref()).await?;
    let cid = customer_for(&s, &u, "tickets").await?;
    let input = TicketInput {
        department_id: b.department_id,
        subject: b.subject.clone(),
        body: b.body.clone(),
        priority: b.priority.clone(),
    };
    let (su, uu) = (&s, &u);
    let v = crate::addon_functions::run(
        &s,
        "ticket.open",
        json!({ "customer_id": cid }),
        input,
        |i: TicketInput| async move {
            let b = OpenBody {
                department_id: i.department_id,
                subject: i.subject,
                body: i.body,
                priority: i.priority,
                ..b
            };
            client_open_core(su, uu, cid, b)
                .await
                .map(|Json(v)| v)
                .map_err(|e| e.0)
        },
    )
    .await?;
    Ok(Json(v))
}

/// The rewritable part of a new client ticket (`fn ticket.open`).
#[derive(Debug, serde::Serialize, Deserialize)]
struct TicketInput {
    department_id: Uuid,
    subject: String,
    body: String,
    priority: Option<String>,
}

async fn client_open_core(
    s: &AppState,
    u: &CurrentUser,
    cid: Uuid,
    b: OpenBody,
) -> ApiResult<Json<Value>> {
    let subject = text(&b.subject, 200, "error.invalid_subject")?;
    let body = text(&b.body, 20000, "error.invalid_message")?;
    let (subject, body, pr, department_id) = pre_open(s, cid, &b, subject, body, false).await?;
    let t = tickets::open(
        &s.pool,
        NewTicket {
            customer_id: cid,
            opened_by: u.user_id,
            department_id,
            service_id: b.service_id,
            subject: &subject,
            priority: pr,
            body: &body,
            by_staff: false,
            attachments: att_ok(s, &b.attachment_ids).await?,
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.opened",
        Some(&t.id.to_string()),
        u.ip.as_deref(),
        json!({ "number": t.number }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "ticket.opened",
        json!({ "ticket_id": t.id, "number": t.number, "customer_id": cid, "department_id": t.department_id, "priority": t.priority, "by_staff": false }),
    ));
    notify_staff(s, &t, &body).await?;
    crate::support_ext::auto_reply(s, t.id).await?;
    detail(s, t, false).await
}

#[utoipa::path(get, path = "/api/v1/client/tickets/{id}", responses((status = 200), (status = 404)))]
pub async fn client_get(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "tickets").await?;
    let t = s
        .tickets()
        .get(id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    detail(&s, t, false).await
}

#[utoipa::path(post, path = "/api/v1/client/tickets/{id}/reply", request_body = ReplyBody, responses((status = 200), (status = 404)))]
pub async fn client_reply(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<ReplyBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "tickets").await?;
    s.tickets()
        .get(id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    let body = text(&b.body, 20000, "error.invalid_message")?;
    let body = pre_reply(&s, id, body, false, false).await?;
    tickets::reply(
        &s.pool,
        id,
        u.user_id,
        false,
        false,
        &body,
        att_ok(&s, &b.attachment_ids).await?,
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "ticket.replied",
        json!({ "ticket_id": id, "by_staff": false }),
    ));
    let t = s
        .tickets()
        .get(id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    notify_staff(&s, &t, &body).await?;
    detail(&s, t, false).await
}

#[utoipa::path(post, path = "/api/v1/client/tickets/{id}/close", responses((status = 200), (status = 404)))]
pub async fn client_close(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "tickets").await?;
    s.tickets()
        .get(id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    s.tickets()
        .update(id, Some("closed"), None, None, None)
        .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "ticket.closed",
        json!({ "ticket_id": id, "reason": "client", "by_staff": false }),
    ));
    let t = s
        .tickets()
        .get(id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    detail(&s, t, false).await
}

#[utoipa::path(get, path = "/api/v1/admin/ticket-departments", responses((status = 200), (status = 403)))]
pub async fn admin_departments(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": s.tickets().departments(false).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/ticket-departments", request_body = DepartmentBody, responses((status = 200), (status = 403)))]
pub async fn admin_create_department(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<DepartmentBody>,
) -> ApiResult<Json<Value>> {
    let name = text(&b.name, 100, "error.invalid_name")?;
    let d = tickets::upsert_department(
        &s.pool,
        None,
        &name,
        b.description.trim(),
        b.is_active,
        b.sort_order,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.department_saved",
        Some(&d.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(d)))
}

#[utoipa::path(put, path = "/api/v1/admin/ticket-departments/{id}", request_body = DepartmentBody, responses((status = 200), (status = 404)))]
pub async fn admin_update_department(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<DepartmentBody>,
) -> ApiResult<Json<Value>> {
    let name = text(&b.name, 100, "error.invalid_name")?;
    let d = tickets::upsert_department(
        &s.pool,
        Some(id),
        &name,
        b.description.trim(),
        b.is_active,
        b.sort_order,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.department_saved",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(d)))
}

#[utoipa::path(get, path = "/api/v1/admin/tickets", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    if let Some(st) = q.status() {
        if st != "active" && !pelty_database::ticket_statuses::exists(&s.pool, st).await? {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    let (items, total) = s
        .tickets()
        .list(None, q.status(), q.q(), limit, offset)
        .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/admin/tickets", request_body = OpenBody, responses((status = 200), (status = 422)))]
pub async fn admin_open(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<OpenBody>,
) -> ApiResult<Json<Value>> {
    let cid = b
        .customer_id
        .ok_or(AppError::Validation("error.invalid_customer"))?;
    s.clients()
        .get(cid)
        .await?
        .ok_or(AppError::Validation("error.invalid_customer"))?;
    let subject = text(&b.subject, 200, "error.invalid_subject")?;
    let body = text(&b.body, 20000, "error.invalid_message")?;
    let (subject, body, pr, department_id) = pre_open(&s, cid, &b, subject, body, true).await?;
    let t = tickets::open(
        &s.pool,
        NewTicket {
            customer_id: cid,
            opened_by: u.user_id,
            department_id,
            service_id: b.service_id,
            subject: &subject,
            priority: pr,
            body: &body,
            by_staff: true,
            attachments: att_ok(&s, &b.attachment_ids).await?,
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.opened",
        Some(&t.id.to_string()),
        u.ip.as_deref(),
        json!({ "number": t.number, "by_staff": true }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "ticket.opened",
        json!({ "ticket_id": t.id, "number": t.number, "customer_id": cid, "department_id": t.department_id, "priority": t.priority, "by_staff": true }),
    ));
    notify_customer(&s, &t).await?;
    detail(&s, t, true).await
}

#[utoipa::path(get, path = "/api/v1/admin/tickets/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let t = s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    detail(&s, t, true).await
}

#[utoipa::path(patch, path = "/api/v1/admin/tickets/{id}", request_body = UpdateBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateBody>,
) -> ApiResult<Json<Value>> {
    if let Some(st) = b.status.as_deref() {
        if !pelty_database::ticket_statuses::exists(&s.pool, st).await? {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    if let Some(p) = b.priority.as_deref() {
        priority(Some(p))?;
    }
    if let Some(Some(a)) = b.assigned_to {
        let staff = users::find(&s.pool, a).await?.is_some_and(|x| x.is_active)
            && users::permissions(&s.pool, a)
                .await?
                .iter()
                .any(|p| p == "tickets.manage");
        if !staff {
            return Err(AppError::Validation("error.invalid_assignee").into());
        }
    }
    tickets::update(
        &s.pool,
        id,
        b.status.as_deref(),
        b.priority.as_deref(),
        b.department_id,
        b.assigned_to,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status, "priority": b.priority }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "ticket.updated",
        json!({ "ticket_id": id, "status": b.status, "priority": b.priority, "department_id": b.department_id, "assigned_to": b.assigned_to.flatten(), "by": u.user_id }),
    ));
    if let Some(Some(a)) = b.assigned_to {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "ticket.assigned",
            json!({ "ticket_id": id, "assigned_to": a, "by": u.user_id }),
        ));
    }
    if b.status.as_deref() == Some("closed") {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "ticket.closed",
            json!({ "ticket_id": id, "reason": "staff", "by_staff": true }),
        ));
    }
    let t = s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    detail(&s, t, true).await
}

#[utoipa::path(post, path = "/api/v1/admin/tickets/{id}/reply", request_body = ReplyBody, responses((status = 200), (status = 404)))]
pub async fn admin_reply(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ReplyBody>,
) -> ApiResult<Json<Value>> {
    s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    let body = text(&b.body, 20000, "error.invalid_message")?;
    let body = pre_reply(&s, id, body, true, b.internal).await?;
    tickets::reply(
        &s.pool,
        id,
        u.user_id,
        true,
        b.internal,
        &body,
        att_ok(&s, &b.attachment_ids).await?,
    )
    .await?;
    if !b.internal {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "ticket.replied",
            json!({ "ticket_id": id, "by_staff": true }),
        ));
    } else {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "ticket.note_added",
            json!({ "ticket_id": id, "by": u.user_id }),
        ));
    }
    let t = s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    if !b.internal {
        notify_customer(&s, &t).await?;
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        if b.internal {
            "ticket.note_added"
        } else {
            "ticket.replied"
        },
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    detail(&s, t, true).await
}

/// Enforces the configurable per-message attachment count.
async fn att_ok<'a>(s: &AppState, ids: &'a [Uuid]) -> Result<&'a [Uuid], AppError> {
    if !ids.is_empty() {
        let max = pelty_database::general_settings::load(&s.pool)
            .await?
            .support
            .attachments_per_message
            .max(1) as usize;
        if ids.len() > max {
            return Err(AppError::Validation("error.attachment_count"));
        }
    }
    Ok(ids)
}
