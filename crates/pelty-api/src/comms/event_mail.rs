//! Customer emails driven by domain events (WHMCS "email on event"):
//! welcome, order confirmation, payment receipt, refund, service lifecycle
//! and ticket opened. Each send respects the operator's disabled list.
use crate::AppState;
use pelty_extensions::events::DomainEvent;
use serde_json::Value;
use uuid::Uuid;

/// Template sent for an event name, if any.
pub fn template_for(event: &str) -> Option<&'static str> {
    Some(match event {
        "user.registered" => "client_signup",
        "order.created" => "order_confirmation",
        "invoice.created" => "invoice_created",
        "invoice.paid" => "invoice_payment_confirmation",
        "invoice.refunded" => "invoice_refund_confirmation",
        "service.provisioned" => "new_service_info",
        "service.unsuspended" => "service_unsuspended",
        "service.terminated" => "service_terminated",
        "service.cancelled" => "cancellation_confirmation",
        "ticket.created" => "ticket_opened",
        _ => return None,
    })
}

fn id(p: &Value, k: &str) -> Option<Uuid> {
    p[k].as_str().and_then(|v| v.parse().ok())
}

pub async fn on_event(s: &AppState, e: &DomainEvent) {
    let Some(tpl) = template_for(e.name) else {
        return;
    };
    if let Err(err) = send(s, tpl, &e.payload).await {
        tracing::warn!(event = e.name, error = %err, "event mail failed");
    }
}

async fn send(s: &AppState, tpl: &str, p: &Value) -> pelty_core::AppResult<()> {
    let base = &s.cfg.public_url;
    let pool = &s.pool;
    use pelty_database::event_mail as q;
    let mut extra: Vec<(String, String)> = Vec::new();
    let customer: Option<Uuid> = match tpl {
        "client_signup" => {
            let Some(uid) = id(p, "user_id") else {
                return Ok(());
            };
            let r = q::user(pool, uid).await?;
            let Some((email, name, locale)) = r else {
                return Ok(());
            };
            let locale = locale
                .filter(|l| s.i18n.has(l))
                .unwrap_or_else(|| s.cfg.default_locale.clone());
            let name = name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| email.clone());
            let v = crate::mail::vars(&[
                ("name", &name),
                ("email", &email),
                ("link", &format!("{base}/client")),
            ]);
            return crate::mail::enqueue(s, tpl, &locale, &email, v).await;
        }
        "order_confirmation" => {
            let Some(oid) = id(p, "order_id") else {
                return Ok(());
            };
            let r = q::order(pool, oid).await?;
            r.map(|(c, n, t)| {
                extra = vec![
                    ("order_number".into(), n.to_string()),
                    ("total".into(), money(&t)),
                    ("link".into(), format!("{base}/client/orders")),
                ];
                c
            })
        }
        "invoice_created" | "invoice_payment_confirmation" | "invoice_refund_confirmation" => {
            let Some(iid) = id(p, "invoice_id") else {
                return Ok(());
            };
            let r = q::invoice(pool, iid).await?;
            match r {
                // Drafts are not sent to customers; the email goes when it is issued.
                Some((_, _, _, st, _)) if tpl == "invoice_created" && st != "unpaid" => None,
                Some((c, n, t, _, due)) => {
                    let total = match (tpl, p["amount"].as_str()) {
                        ("invoice_refund_confirmation", Some(a)) => a.to_string(),
                        _ => money(&t),
                    };
                    extra = vec![
                        (
                            "invoice_number".into(),
                            pelty_database::invoices::display_number(n, iid),
                        ),
                        ("total".into(), total),
                        ("due_date".into(), due.unwrap_or_default()),
                        ("link".into(), format!("{base}/client/invoices/{iid}")),
                    ];
                    Some(c)
                }
                None => None,
            }
        }
        "ticket_opened" => {
            let Some(tid) = id(p, "ticket_id") else {
                return Ok(());
            };
            let r = q::ticket(pool, tid).await?;
            r.map(|(c, n, sub)| {
                extra = vec![
                    ("ticket_number".into(), n.to_string()),
                    ("subject".into(), sub),
                    ("link".into(), format!("{base}/client/tickets/{tid}")),
                ];
                c
            })
        }
        _ => {
            let Some(sid) = id(p, "service_id") else {
                return Ok(());
            };
            let r = q::service(pool, sid).await?;
            r.map(|(c, name, remote, cancel)| {
                extra = vec![
                    ("service".into(), name),
                    ("username".into(), remote.unwrap_or_default()),
                    ("server".into(), String::new()),
                    ("cancel_date".into(), cancel.unwrap_or_default()),
                    ("link".into(), format!("{base}/client/services/{sid}")),
                ];
                c
            })
        }
    };
    let Some(c) = customer else { return Ok(()) };
    let Some((to, name, locale)) = pelty_database::overdue::recipient(pool, c).await? else {
        return Ok(());
    };
    let locale = locale
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let mut v: crate::mail::Vars = extra.into_iter().collect();
    v.insert("name".into(), name);
    crate::mail::enqueue(s, tpl, &locale, &to, v).await
}

/// "49.0000 EUR" -> "49.00 EUR".
fn money(t: &str) -> String {
    let (n, cur) = t.split_once(' ').unwrap_or((t, ""));
    let n = n
        .parse::<rust_decimal::Decimal>()
        .map(|d| d.round_dp(2).to_string())
        .unwrap_or_else(|_| n.to_string());
    format!("{n} {cur}").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_and_money() {
        assert_eq!(
            template_for("invoice.paid"),
            Some("invoice_payment_confirmation")
        );
        assert_eq!(template_for("invoice.overdue"), None);
        assert_eq!(money("49.5000 EUR"), "49.50 EUR");
        for e in [
            "user.registered",
            "order.created",
            "service.cancelled",
            "ticket.created",
        ] {
            assert!(pelty_notifications::template::def(template_for(e).unwrap()).is_some());
        }
    }
}
