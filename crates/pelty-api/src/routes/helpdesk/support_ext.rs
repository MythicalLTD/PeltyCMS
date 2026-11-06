//! Support extensions: department piping/auto-reply settings, spam rules,
//! ticket time tracking, KB tags, announcements RSS and the contact form.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::support_ext::{self as db, DeptUpdate, PROTOCOLS, SPAM_KINDS};
use pelty_database::{audit, content, support, tickets};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/support/departments", get(departments))
        .route("/admin/support/departments/{id}", put(update_department))
        .route("/admin/support/departments/{id}/pipe", post(pipe_now))
        .route(
            "/admin/support/departments/{id}/inbound-token",
            post(inbound_token),
        )
        .route(
            "/public/support/inbound/{id}",
            post(inbound_email).layer(axum::extract::DefaultBodyLimit::max(
                crate::mailbox::MAX_MESSAGE,
            )),
        )
        .route("/admin/support/spam-rules", get(spam_rules).post(add_spam))
        .route("/admin/support/spam-rules/{id}", delete(del_spam))
        .route("/admin/tickets/{id}/time", get(time_list).post(time_add))
        .route("/admin/tickets/{id}/time/{entry}", delete(time_del))
        .route(
            "/admin/tickets/{id}/watchers",
            get(get_watchers).put(put_watchers),
        )
        .route("/admin/kb/articles/{id}/tags", get(get_tags).put(put_tags))
        .route("/public/kb/tags", get(tag_cloud))
        .route("/public/kb/tags/{tag}", get(by_tag))
        .route("/public/announcements/rss.xml", get(rss))
        .route("/public/contact", post(contact_submit))
        .route("/admin/contact-messages", get(contact_list))
        .route(
            "/admin/contact-messages/{id}",
            put(contact_status).delete(contact_del),
        )
        .route(
            "/admin/contact-messages/{id}/ticket",
            post(contact_to_ticket),
        )
}

// ---- Departments ----

#[utoipa::path(get, path = "/api/v1/admin/support/departments", responses((status = 200)))]
pub async fn departments(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    let rows = db::departments(&s.pool).await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|d| {
            let has = !d.pipe_password.is_empty();
            let mut v = json!(d);
            v["has_password"] = json!(has);
            v
        })
        .collect();
    Ok(Json(
        json!({ "departments": out, "encryption": s.secret_box.is_some() }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct DeptBody {
    pub auto_reply: bool,
    #[serde(default)]
    pub auto_reply_text: String,
    pub pipe_protocol: String,
    #[serde(default)]
    pub pipe_host: String,
    #[serde(default)]
    pub pipe_port: i32,
    #[serde(default)]
    pub pipe_tls: bool,
    #[serde(default)]
    pub pipe_username: String,
    /// Write-only; omitted/empty keeps the stored password.
    #[serde(default)]
    pub pipe_password: Option<String>,
    /// Unknown sender handling: `reject` (default) or email-only `guest`.
    #[serde(default)]
    pub pipe_unknown_sender: Option<String>,
}

pub fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

#[utoipa::path(put, path = "/api/v1/admin/support/departments/{id}", responses((status = 200), (status = 422)))]
pub async fn update_department(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<DeptBody>,
) -> ApiResult<Json<Value>> {
    db::department(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !PROTOCOLS.contains(&b.pipe_protocol.as_str())
        || b.auto_reply_text.len() > 5000
        || b.pipe_unknown_sender
            .as_deref()
            .is_some_and(|v| !["guest", "reject"].contains(&v))
        || b.pipe_username.contains(['\r', '\n'])
        || b.pipe_password
            .as_deref()
            .is_some_and(|p| p.contains(['\r', '\n']))
    {
        return Err(AppError::Validation("error.invalid_pipe_settings").into());
    }
    if b.pipe_protocol != "none"
        && (!valid_host(&b.pipe_host)
            || !(1..=65535).contains(&b.pipe_port)
            || b.pipe_username.trim().is_empty())
    {
        return Err(AppError::Validation("error.invalid_pipe_settings").into());
    }
    let sealed = match b.pipe_password.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => Some(
            crate::support_ext::seal_password(&s, p)
                .ok_or(AppError::Validation("error.encryption_not_configured"))?,
        ),
        None => None,
    };
    let d = db::update_department(
        &s.pool,
        id,
        &DeptUpdate {
            auto_reply: b.auto_reply,
            auto_reply_text: b.auto_reply_text.trim().to_string(),
            pipe_protocol: b.pipe_protocol,
            pipe_host: b.pipe_host.trim().to_string(),
            pipe_port: b.pipe_port,
            pipe_tls: b.pipe_tls,
            pipe_username: b.pipe_username.trim().to_string(),
            pipe_password_sealed: sealed,
            pipe_unknown_sender: b.pipe_unknown_sender,
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "department.settings",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "protocol": d.pipe_protocol, "auto_reply": d.auto_reply }),
    )
    .await?;
    Ok(Json(json!(d)))
}

#[utoipa::path(post, path = "/api/v1/admin/support/departments/{id}/pipe", responses((status = 200)))]
pub async fn pipe_now(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(crate::support_ext::run_pipe(&s, Some(id)).await?))
}

// ---- Spam rules ----

#[utoipa::path(get, path = "/api/v1/admin/support/spam-rules", responses((status = 200)))]
pub async fn spam_rules(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "rules": db::spam_rules(&s.pool).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SpamBody {
    pub kind: String,
    pub value: String,
}

#[utoipa::path(post, path = "/api/v1/admin/support/spam-rules", responses((status = 200), (status = 422)))]
pub async fn add_spam(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<SpamBody>,
) -> ApiResult<Json<Value>> {
    let v = b.value.trim().to_lowercase();
    if !SPAM_KINDS.contains(&b.kind.as_str()) || v.is_empty() || v.len() > 255 {
        return Err(AppError::Validation("error.invalid_spam_rule").into());
    }
    let rule = json!(db::add_spam_rule(&s.pool, &b.kind, &v).await?);
    audit::record(
        &s.pool,
        Some(u.user_id),
        "support.spam_rule_added",
        rule["id"].as_str(),
        u.ip.as_deref(),
        json!({ "kind": b.kind, "value": v }),
    )
    .await?;
    Ok(Json(rule))
}

#[utoipa::path(delete, path = "/api/v1/admin/support/spam-rules/{id}", responses((status = 200)))]
pub async fn del_spam(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::delete_spam_rule(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "support.spam_rule_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---- Time tracking ----

#[utoipa::path(get, path = "/api/v1/admin/tickets/{id}/time", responses((status = 200)))]
pub async fn time_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let rows = db::time_entries(&s.pool, id).await?;
    let total: i64 = rows.iter().map(|r| i64::from(r.minutes)).sum();
    let billable: i64 = rows
        .iter()
        .filter(|r| r.billable)
        .map(|r| i64::from(r.minutes))
        .sum();
    Ok(Json(
        json!({ "entries": rows, "total_minutes": total, "billable_minutes": billable }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TimeBody {
    pub minutes: i32,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub billable: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/tickets/{id}/time", responses((status = 200), (status = 422)))]
pub async fn time_add(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TimeBody>,
) -> ApiResult<Json<Value>> {
    if !(1..=1440).contains(&b.minutes) || b.note.len() > 500 {
        return Err(AppError::Validation("error.invalid_time_entry").into());
    }
    s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    let eid = db::add_time(&s.pool, id, u.user_id, b.minutes, b.note.trim(), b.billable).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.time_added",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "entry": eid, "minutes": b.minutes, "billable": b.billable }),
    )
    .await?;
    Ok(Json(json!({ "id": eid })))
}

#[utoipa::path(delete, path = "/api/v1/admin/tickets/{id}/time/{entry}", responses((status = 200)))]
pub async fn time_del(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path((id, entry)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    db::delete_time(&s.pool, id, entry).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.time_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "entry": entry }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---- Watchers ----

#[utoipa::path(get, path = "/api/v1/admin/tickets/{id}/watchers", responses((status = 200)))]
pub async fn get_watchers(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "watchers": db::watchers(&s.pool, id).await? }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WatchersBody {
    pub user_ids: Vec<Uuid>,
}

#[utoipa::path(put, path = "/api/v1/admin/tickets/{id}/watchers", responses((status = 200)))]
pub async fn put_watchers(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<WatchersBody>,
) -> ApiResult<Json<Value>> {
    s.tickets().get(id, None).await?.ok_or(AppError::NotFound)?;
    let ids: Vec<Uuid> = b.user_ids.into_iter().take(20).collect();
    let w = db::set_watchers(&s.pool, id, &ids).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "ticket.watchers",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "watchers": w }),
    )
    .await?;
    Ok(Json(json!({ "watchers": w })))
}

// ---- KB tags ----

pub fn normalize_tags(raw: &[String]) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t = t.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.len() > 40
            || !t
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == ' ')
        {
            return None;
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    (out.len() <= 20).then_some(out)
}

#[utoipa::path(get, path = "/api/v1/admin/kb/articles/{id}/tags", responses((status = 200)))]
pub async fn get_tags(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ContentManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "tags": db::article_tags(&s.pool, id).await? }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TagsBody {
    pub tags: Vec<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/kb/articles/{id}/tags", responses((status = 200), (status = 422)))]
pub async fn put_tags(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ContentManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TagsBody>,
) -> ApiResult<Json<Value>> {
    let tags = normalize_tags(&b.tags).ok_or(AppError::Validation("error.invalid_tag"))?;
    content::article(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    db::set_article_tags(&s.pool, id, &tags).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "kb.article_tags_set",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "tags": tags }),
    )
    .await?;
    Ok(Json(json!({ "tags": tags })))
}

#[utoipa::path(get, path = "/api/v1/public/kb/tags", responses((status = 200)))]
pub async fn tag_cloud(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let tags: Vec<Value> = db::tag_cloud(&s.pool)
        .await?
        .into_iter()
        .map(|(t, n)| json!({ "tag": t, "count": n }))
        .collect();
    Ok(Json(json!({ "tags": tags })))
}

#[utoipa::path(get, path = "/api/v1/public/kb/tags/{tag}", responses((status = 200)))]
pub async fn by_tag(State(s): State<AppState>, Path(tag): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "articles": db::articles_by_tag(&s.pool, &tag.trim().to_lowercase()).await? }),
    ))
}

// ---- Announcements RSS ----

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Deserialize)]
pub struct RssQuery {
    pub locale: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/public/announcements/rss.xml", responses((status = 200)))]
pub async fn rss(
    State(s): State<AppState>,
    Query(q): Query<RssQuery>,
) -> ApiResult<impl IntoResponse> {
    let (items, _) = content::announcements(&s.pool, true, q.locale.as_deref(), 30, 0).await?;
    let base = s.cfg.public_url.trim_end_matches('/');
    let mut x =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel>");
    let loc = q
        .locale
        .clone()
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let title = xml_escape(s.i18n.t(&loc, "rss.announcements.title"));
    x.push_str(&format!(
        "<title>{title}</title><link>{base}/announcements</link><description>{title}</description>"
    ));
    for a in items {
        let date = a
            .published_at
            .format(&time::format_description::well_known::Rfc2822)
            .unwrap_or_default();
        x.push_str(&format!(
            "<item><title>{}</title><link>{base}/announcements/{id}</link><guid isPermaLink=\"false\">{id}</guid><pubDate>{date}</pubDate><description>{}</description></item>",
            xml_escape(&a.title),
            xml_escape(&a.body.chars().take(1000).collect::<String>()),
            id = a.id,
        ));
    }
    x.push_str("</channel></rss>");
    Ok((
        [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
        x,
    ))
}

// ---- Contact form ----

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ContactBody {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub subject: String,
    pub message: String,
    /// Honeypot: real browsers leave it empty (field hidden from humans).
    #[serde(default)]
    pub website: String,
}

fn ip_of(ext: &axum::http::Extensions) -> String {
    ext.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

pub fn valid_email(e: &str) -> bool {
    let e = e.trim();
    e.len() <= 254
        && e.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !e.contains(' ')
        })
}

#[utoipa::path(post, path = "/api/v1/public/contact", responses((status = 200), (status = 422), (status = 429)))]
pub async fn contact_submit(
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    ext: axum::http::Extensions,
    Json(b): Json<ContactBody>,
) -> ApiResult<Json<Value>> {
    let ip = ip_of(&ext);
    if !s.auth_limiter.check(&format!("contact:{ip}")) {
        return Err(AppError::RateLimited.into());
    }
    crate::captcha::require(&s, &headers, "contact", Some(&ip)).await?;
    let (name, email, msg) = (
        b.name.trim(),
        b.email.trim().to_lowercase(),
        b.message.trim(),
    );
    if name.is_empty()
        || name.chars().count() > 100
        || !valid_email(&email)
        || msg.is_empty()
        || msg.len() > 10_000
        || b.subject.len() > 200
    {
        return Err(AppError::Validation("error.invalid_contact").into());
    }
    let rules = db::spam_rules(&s.pool).await?;
    // Honeypot hits and spam-rule matches are stored as spam, answered as success.
    let spam = !b.website.is_empty() || db::spam_match(&rules, &email, &b.subject, msg).is_some();
    let subject = if b.subject.trim().is_empty() {
        "(no subject)"
    } else {
        b.subject.trim()
    };
    db::add_contact(&s.pool, name, &email, subject, msg, Some(&ip), spam).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ContactQuery {
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/contact-messages", responses((status = 200)))]
pub async fn contact_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Query(q): Query<ContactQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "messages": db::contacts(&s.pool, q.status.as_deref()).await? }),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct StatusBody {
    pub status: String,
}

#[utoipa::path(put, path = "/api/v1/admin/contact-messages/{id}", responses((status = 200), (status = 422)))]
pub async fn contact_status(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    if !["new", "read", "spam"].contains(&b.status.as_str()) {
        return Err(AppError::Validation("error.invalid_status").into());
    }
    let r = json!(db::set_contact_status(&s.pool, id, &b.status, None).await?);
    audit::record(
        &s.pool,
        Some(u.user_id),
        "support.contact_status",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(r))
}

#[utoipa::path(delete, path = "/api/v1/admin/contact-messages/{id}", responses((status = 200)))]
pub async fn contact_del(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::delete_contact(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "support.contact_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ConvertBody {
    pub department_id: Uuid,
}

/// Turn a contact message into a ticket for the client with that email.
#[utoipa::path(post, path = "/api/v1/admin/contact-messages/{id}/ticket", responses((status = 200), (status = 422)))]
pub async fn contact_to_ticket(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ConvertBody>,
) -> ApiResult<Json<Value>> {
    let m = db::contact(&s.pool, id).await?;
    if m.ticket_id.is_some() {
        return Err(AppError::Conflict("error.already_converted").into());
    }
    let (customer, _) = support::customer_by_email(&s.pool, &m.email)
        .await?
        .ok_or(AppError::Validation("error.contact_no_customer"))?;
    let subject = if m.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        m.subject.clone()
    };
    let t = s
        .tickets()
        .open(tickets::NewTicket {
            customer_id: customer,
            opened_by: u.user_id,
            department_id: b.department_id,
            service_id: None,
            subject: &subject,
            priority: "medium",
            body: &m.body,
            by_staff: true,
            attachments: &[],
        })
        .await?;
    db::set_contact_status(&s.pool, id, "converted", Some(t.id)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "contact.converted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "ticket": t.id }),
    )
    .await?;
    Ok(Json(json!({ "ticket_id": t.id, "number": t.number })))
}

/// Generate/rotate the department webhook signing token. Returned only once.
#[utoipa::path(post, path = "/api/v1/admin/support/departments/{id}/inbound-token", responses((status = 200), (status = 403), (status = 422)))]
pub async fn inbound_token(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::department(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let sealed = crate::support_ext::seal_password(&s, &token)
        .ok_or(AppError::Validation("error.encryption_not_configured"))?;
    sqlx::query("UPDATE ticket_departments SET pipe_webhook_secret=$2 WHERE id=$1")
        .bind(id)
        .bind(sealed)
        .execute(&s.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "department.inbound_token_rotated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({"token": token})))
}

/// Verify a five-minute, body-bound provider signature using constant-time HMAC.
pub fn verify_inbound(token: &str, timestamp: &str, signature: &str, raw: &[u8], now: i64) -> bool {
    use base64::Engine;
    use hmac::{digest::KeyInit, Hmac, Mac};
    let Ok(ts) = timestamp.parse::<i64>() else {
        return false;
    };
    if now.abs_diff(ts) > 300 {
        return false;
    }
    let Ok(sig) = base64::engine::general_purpose::STANDARD.decode(signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<sha2::Sha256>::new_from_slice(token.as_bytes()) else {
        return false;
    };
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(raw);
    mac.verify_slice(&sig).is_ok()
}

/// Provider adapter: POST raw RFC 5322 email, not provider-specific JSON.
#[utoipa::path(post, path = "/api/v1/public/support/inbound/{id}",
    request_body(content = String, content_type = "message/rfc822"),
    params(("X-Pelty-Timestamp" = String, Header, description = "Unix seconds (five-minute tolerance)"),
           ("X-Pelty-Signature" = String, Header, description = "Base64 HMAC-SHA256(token, timestamp + dot + exact request bytes)")),
    responses((status = 200), (status = 401), (status = 413), (status = 422)))]
pub async fn inbound_email(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
    raw: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let denied = || AppError::Unauthenticated;
    let d = db::department(&s.pool, id).await?.ok_or_else(denied)?;
    let token = crate::support_ext::open_password(&s, &d.pipe_webhook_secret).ok_or_else(denied)?;
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !verify_inbound(
        &token,
        h("x-pelty-timestamp"),
        h("x-pelty-signature"),
        &raw,
        time::OffsetDateTime::now_utc().unix_timestamp(),
    ) {
        return Err(denied().into());
    }
    let active = s
        .tickets()
        .departments(true)
        .await?
        .iter()
        .any(|dep| dep.id == id);
    if !active || d.email.is_empty() {
        return Err(AppError::Validation("error.invalid_pipe_settings").into());
    }
    let text = std::str::from_utf8(&raw)
        .map_err(|_| AppError::Validation("error.invalid_pipe_settings"))?;
    let stats = crate::pipe::process_messages_in_department(
        &s,
        vec![crate::mailbox::parse_message(text, &d.email)],
        id,
    )
    .await
    .map_err(AppError::Internal)?;
    Ok(Json(
        json!({"opened": stats.opened, "replied": stats.replied, "rejected": stats.rejected, "duplicate": stats.duplicate, "spam": stats.spam}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use hmac::{Hmac, KeyInit, Mac};
    #[test]
    fn helpers() {
        assert_eq!(
            normalize_tags(&[" Billing ".into(), "billing".into(), "".into()]),
            Some(vec!["billing".to_string()])
        );
        assert_eq!(normalize_tags(&["<x>".into()]), None);
        assert_eq!(xml_escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
        assert!(valid_email("a@b.co"));
        assert!(!valid_email("a@b"));
        assert!(!valid_email("a b@c.d"));
        assert!(valid_host("imap.example.com"));
        assert!(!valid_host("x/y"));
    }

    #[test]
    fn body_bound_timestamp_signature() {
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"1700000000.message");
        let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        assert!(verify_inbound(
            "secret",
            "1700000000",
            &sig,
            b"message",
            1700000000
        ));
        assert!(!verify_inbound(
            "secret",
            "1700000000",
            &sig,
            b"tampered",
            1700000000
        ));
        assert!(!verify_inbound(
            "wrong",
            "1700000000",
            &sig,
            b"message",
            1700000000
        ));
        assert!(!verify_inbound(
            "secret",
            "1700000000",
            &sig,
            b"message",
            1700000301
        ));
        assert!(!verify_inbound(
            "secret",
            "1700000000",
            &sig,
            b"message",
            1699999699
        ));
        assert!(!verify_inbound(
            "secret", "bad", &sig, b"message", 1700000000
        ));
        assert!(!verify_inbound(
            "secret",
            "1700000000",
            "not base64",
            b"message",
            1700000000
        ));
    }
}
