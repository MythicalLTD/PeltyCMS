//! Glue for the daily `billing.overdue` job: runs the DB automation, then
//! mails reminders / suspension notices, queues panel actions and emits events.
use crate::AppState;
use pelty_database::overdue;
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use time::Date;
use uuid::Uuid;

pub const JOB_KIND: &str = "billing.overdue";

pub async fn schedule(s: &AppState) -> pelty_core::AppResult<Option<i64>> {
    let today = crate::routes::automation::local_today();
    pelty_database::jobs::enqueue(&s.pool, JOB_KIND, json!({ "date": today.to_string() }), None, Some(&format!("overdue:{today}")), 5).await
}

async fn mail(s: &AppState, customer: Uuid, template: &str, extra: &[(&str, &str)]) -> pelty_core::AppResult<()> {
    let Some((to, name, locale)) = overdue::recipient(&s.pool, customer).await? else {
        return Ok(());
    };
    let locale = locale.filter(|l| s.i18n.has(l)).unwrap_or_else(|| s.cfg.default_locale.clone());
    let mut v = crate::mail::vars(extra);
    v.insert("name".into(), name);
    crate::mail::enqueue(s, template, &locale, &to, v).await
}

/// Run the automation for `today` and perform the side effects.
pub async fn run(s: &AppState, today: Date) -> pelty_core::AppResult<Value> {
    // `billing.before_overdue_run`: an addon may postpone the whole run
    // (holidays, payment-provider outages); the next daily run catches up.
    match crate::addon_ext::pre_event(s, "billing.before_overdue_run", json!({ "date": today.to_string() })).await {
        Ok(_) => {}
        Err(pelty_core::AppError::Conflict("error.addon_hook_cancelled")) => {
            tracing::info!(%today, "overdue run postponed by an addon");
            return Ok(json!({ "skipped": "addon_hook_cancelled" }));
        }
        Err(e) => return Err(e),
    }
    let r = overdue::run_overdue(&s.pool, today).await?;
    for m in &r.reminders {
        let link = format!("{}/invoices/{}", s.cfg.public_url, m.invoice_id);
        let total = format!("{} {}", m.balance.normalize(), m.currency);
        let (due, days) = (m.due_date.to_string(), m.days_overdue.to_string());
        mail(s, m.customer_id, "invoice_reminder", &[("invoice_number", &m.number), ("total", &total), ("due_date", &due), ("days_overdue", &days), ("link", &link)]).await?;
        s.emit(DomainEvent::new("invoice.overdue", json!({ "invoice_id": m.invoice_id, "customer_id": m.customer_id, "days_overdue": m.days_overdue })));
    }
    for f in &r.late_fees {
        s.emit(DomainEvent::new("invoice.created", json!({ "invoice_id": f.fee_invoice_id, "customer_id": f.customer_id, "late_fee_for": f.invoice_id })));
    }
    for x in &r.suspended {
        crate::provision::on_transition(s, x.service_id, "active", "suspended").await;
        s.emit(DomainEvent::new("service.suspended", json!({ "service_id": x.service_id, "from": "active", "reason": overdue::REASON })));
        let svc = pelty_database::services::get(&s.pool, x.service_id).await?.map(|v| v.product_name).unwrap_or_default();
        let link = format!("{}/invoices/{}", s.cfg.public_url, x.invoice_id);
        let reason = s.i18n.t(&s.cfg.default_locale, "overdue.reason").to_string();
        mail(s, x.customer_id, "service_suspended", &[("service", &svc), ("reason", &reason), ("link", &link)]).await?;
    }
    for x in &r.terminated {
        crate::provision::on_transition(s, x.service_id, "suspended", "terminated").await;
        s.emit(DomainEvent::new("service.terminated", json!({ "service_id": x.service_id, "from": "suspended", "reason": overdue::REASON })));
    }
    for id in &r.voided {
        s.emit(DomainEvent::new("invoice.cancelled", json!({ "invoice_id": id, "reason": "overdue_auto_cancel" })));
    }
    Ok(json!({ "reminders": r.reminders.len(), "late_fees": r.late_fees.len(), "suspended": r.suspended.len(), "terminated": r.terminated.len(), "voided": r.voided.len() }))
}

/// Unsuspend services suspended for an invoice that is now paid. Never fails
/// the payment; errors are logged.
pub async fn after_payment(s: &AppState, invoice_id: Uuid) {
    match overdue::after_payment(&s.pool, invoice_id).await {
        Ok(ids) => {
            for id in ids {
                crate::provision::on_transition(s, id, "suspended", "active").await;
                s.emit(DomainEvent::new("service.unsuspended", json!({ "service_id": id, "from": "suspended", "reason": "paid" })));
            }
        }
        Err(e) => tracing::error!(error = %e, %invoice_id, "overdue unsuspend failed"),
    }
}
