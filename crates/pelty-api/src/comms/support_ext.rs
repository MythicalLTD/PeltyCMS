//! Support automation: department auto-replies and mailbox piping runs.
use crate::error::ApiResult;
use crate::mailbox::{MailboxConfig, MailboxSource};
use crate::{mail, pipe, AppState};
use base64::Engine;
use pelty_database::{support_ext as db, tickets};
use serde_json::{json, Value};
use uuid::Uuid;

pub const PIPE_JOB: &str = "support.pipe";

/// Send the department auto-reply for a newly opened ticket (if enabled).
pub async fn auto_reply(s: &AppState, ticket: Uuid) -> ApiResult<bool> {
    let Some(t) = tickets::get(&s.pool, ticket, None).await? else {
        return Ok(false);
    };
    let Some(text) = db::auto_reply_for(&s.pool, t.department_id).await? else {
        return Ok(false);
    };
    if t.customer_email.is_empty() {
        return Ok(false);
    }
    let name = if t.customer_name.trim().is_empty() {
        t.customer_email.clone()
    } else {
        t.customer_name.clone()
    };
    let link = format!("{}/tickets/{}", s.cfg.public_url, t.id);
    let num = t.number.to_string();
    mail::enqueue(
        s,
        "ticket_autoreply",
        &s.cfg.default_locale,
        &t.customer_email,
        mail::vars(&[
            ("name", &name),
            ("ticket_number", &num),
            ("subject", &t.subject),
            ("department", &t.department_name),
            ("message", &text),
            ("link", &link),
        ]),
    )
    .await?;
    Ok(true)
}

/// Fan out committed inbound notifications through the existing retrying queue.
/// Delivery is at-least-once: a worker crash after fanout but before completing
/// this job can enqueue a duplicate notification, but cannot lose the intent.
pub async fn notify_inbound(s: &AppState, payload: &Value) -> Result<Value, String> {
    let ticket = payload["ticket_id"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
        .ok_or("missing inbound ticket_id")?;
    let message = payload["ticket_message_id"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
        .ok_or("missing inbound ticket_message_id")?;
    let outcome = payload["outcome"]
        .as_str()
        .filter(|v| matches!(*v, "opened" | "replied"))
        .ok_or("invalid inbound outcome")?;
    let Some(t) = tickets::get(&s.pool, ticket, None)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(json!({"deleted": true}));
    };
    let body: Option<String> = sqlx::query_scalar(
        "SELECT body FROM ticket_messages WHERE id=$1 AND ticket_id=$2 AND NOT is_staff AND NOT is_internal"
    ).bind(message).bind(ticket).fetch_optional(&s.pool).await.map_err(|e| e.to_string())?;
    let Some(body) = body else {
        return Ok(json!({"deleted": true}));
    };
    crate::routes::tickets::notify_staff(s, &t, &body)
        .await
        .map_err(|e| e.0.to_string())?;
    if outcome == "opened" {
        auto_reply(s, ticket).await.map_err(|e| e.0.to_string())?;
        s.emit(pelty_extensions::events::DomainEvent::new(
            "ticket.opened",
            json!({
                "ticket_id": ticket, "number": t.number, "customer_id": t.customer_id,
                "department_id": t.department_id, "priority": t.priority, "by_staff": false,
            }),
        ));
    } else {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "ticket.replied",
            json!({"ticket_id": ticket, "by_staff": false}),
        ));
    }
    Ok(json!({"ticket_id": ticket, "notified": true}))
}

pub fn seal_password(s: &AppState, plain: &str) -> Option<String> {
    s.secret_box
        .as_ref()
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b.seal(plain.as_bytes())))
}

pub(crate) fn open_password(s: &AppState, sealed: &str) -> Option<String> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(sealed)
        .ok()?;
    String::from_utf8(s.secret_box.as_ref()?.open(&raw)?).ok()
}

/// Poll every department mailbox once. Errors are stored per department.
pub async fn run_pipe(s: &AppState, only: Option<Uuid>) -> ApiResult<Value> {
    let mut out = Vec::new();
    let active_departments = pelty_database::tickets::departments(&s.pool, true).await?;
    for d in db::departments(&s.pool).await? {
        let active = active_departments.iter().any(|dep| dep.id == d.id);
        if !active {
            continue;
        }
        if d.pipe_protocol == "none" || d.pipe_host.is_empty() || only.is_some_and(|o| o != d.id) {
            continue;
        }
        let Some(password) = open_password(s, &d.pipe_password) else {
            db::mark_pipe_run(&s.pool, d.id, "password not set").await?;
            out.push(json!({ "department": d.id, "error": "password not set" }));
            continue;
        };
        let src = MailboxSource {
            connector: s.mailbox.clone(),
            cfg: MailboxConfig {
                protocol: d.pipe_protocol.clone(),
                host: d.pipe_host.clone(),
                port: u16::try_from(d.pipe_port).unwrap_or(993),
                tls: d.pipe_tls,
                username: d.pipe_username.clone(),
                password,
                address: d.email.clone(),
            },
        };
        match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            pipe::process_in_department(s, &src, d.id),
        )
        .await
        .unwrap_or_else(|_| Err("mailbox poll timed out".into()))
        {
            Ok(st) => {
                db::mark_pipe_run(&s.pool, d.id, "").await?;
                out.push(json!({ "department": d.id, "opened": st.opened, "replied": st.replied, "rejected": st.rejected, "duplicate": st.duplicate, "spam": st.spam }));
            }
            Err(e) => {
                db::mark_pipe_run(&s.pool, d.id, &e).await?;
                out.push(json!({ "department": d.id, "error": e }));
            }
        }
    }
    Ok(json!({ "departments": out }))
}

/// Enqueue a mailbox poll per 5-minute window (deduplicated).
pub async fn schedule(s: &AppState) -> pelty_core::AppResult<Option<i64>> {
    let key = format!(
        "{PIPE_JOB}:{}",
        time::OffsetDateTime::now_utc().unix_timestamp() / 300
    );
    pelty_database::jobs::enqueue(&s.pool, PIPE_JOB, json!({}), None, Some(&key), 3).await
}
