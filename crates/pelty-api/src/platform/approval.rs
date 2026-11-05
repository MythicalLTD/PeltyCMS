//! Order auto-approval (rules evaluated on order.created / invoice.paid),
//! shared order activation, and the daily pre-due invoice reminders.
use crate::AppState;
use pelty_core::AppResult;
use pelty_database::{audit, invoice_tools, order_approval, orders};
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

pub const DUE_JOB: &str = "billing.due_reminders";

/// Approve a pending order, then emit events and provision its services.
pub async fn activate(s: &AppState, id: Uuid, actor: Option<Uuid>, ip: Option<&str>, auto: bool) -> AppResult<Vec<Uuid>> {
    let ids = orders::approve(&s.pool, id, actor).await?;
    audit::record(&s.pool, actor, if auto { "order.auto_approved" } else { "order.approved" }, Some(&id.to_string()), ip, json!({ "services": ids })).await?;
    s.emit(DomainEvent::new("order.approved", json!({ "order_id": id, "services": ids, "auto": auto })));
    for sid in &ids {
        s.emit(DomainEvent::new("service.created", json!({ "service_id": sid, "order_id": id })));
        crate::provision::on_transition(s, *sid, "pending", "active").await;
    }
    Ok(ids)
}

/// Approve the order if every rule passes. Returns whether it was approved.
pub async fn try_auto(s: &AppState, order: Uuid) -> AppResult<bool> {
    if !order_approval::auto_ok(&s.pool, order).await? {
        return Ok(false);
    }
    match activate(s, order, None, None, true).await {
        Ok(_) => Ok(true),
        // Raced with another trigger or staff: already handled.
        Err(pelty_core::AppError::Conflict("error.order_not_pending")) => Ok(false),
        Err(e) => Err(e),
    }
}

pub async fn on_event(s: &AppState, e: &DomainEvent) {
    let order = match e.name {
        "order.created" => e.payload["order_id"].as_str().and_then(|v| v.parse().ok()),
        "invoice.paid" => match e.payload["invoice_id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) {
            Some(inv) => order_approval::pending_for_invoice(&s.pool, inv).await.ok().flatten(),
            None => None,
        },
        _ => None,
    };
    if let Some(o) = order {
        if let Err(err) = try_auto(s, o).await {
            tracing::error!(error = %err, order = %o, "order auto-approval failed");
        }
    }
}

pub async fn schedule_due(s: &AppState) -> AppResult<Option<i64>> {
    let today = OffsetDateTime::now_utc().date();
    pelty_database::jobs::enqueue(&s.pool, DUE_JOB, json!({ "date": today.to_string() }), None, Some(&format!("due:{today}")), 5).await
}

/// Mail one "invoice due soon" reminder.
pub async fn mail_due(s: &AppState, m: &invoice_tools::DueSoon) -> AppResult<()> {
    let Some((to, name, locale)) = pelty_database::overdue::recipient(&s.pool, m.customer_id).await? else {
        return Ok(());
    };
    let locale = locale.filter(|l| s.i18n.has(l)).unwrap_or_else(|| s.cfg.default_locale.clone());
    let link = format!("{}/invoices/{}", s.cfg.public_url, m.invoice_id);
    let total = format!("{} {}", m.balance.normalize(), m.currency);
    let (due, days) = (m.due_date.to_string(), m.days_left.to_string());
    let mut v = crate::mail::vars(&[("invoice_number", &m.number), ("total", &total), ("due_date", &due), ("days_left", &days), ("link", &link)]);
    v.insert("name".into(), name);
    crate::mail::enqueue(s, "invoice_due_soon", &locale, &to, v).await
}

pub async fn run_due(s: &AppState, today: Date) -> AppResult<Value> {
    let list = invoice_tools::run_due_reminders(&s.pool, today).await?;
    for m in &list {
        mail_due(s, m).await?;
        s.emit(DomainEvent::new("invoice.due_soon", json!({ "invoice_id": m.invoice_id, "customer_id": m.customer_id, "days_left": m.days_left })));
    }
    Ok(json!({ "sent": list.len() }))
}
