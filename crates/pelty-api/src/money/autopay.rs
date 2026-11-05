//! Daily `billing.autopay` job: charge due invoices off-session with the
//! customer's default saved method, retry on the configured schedule and mail
//! a dunning notice after each failed attempt.
use crate::AppState;
use pelty_core::money::{Currency, Money};
use pelty_core::{AppError, AppResult};
use pelty_database::autopay::{self, Claim};
use pelty_database::invoices;
use pelty_extensions::events::DomainEvent;
use pelty_payments::{ChargeOutcome, PaymentGateway, PaymentRequest};
use serde_json::{json, Value};
use time::Date;

pub const JOB_KIND: &str = "billing.autopay";

pub async fn schedule(s: &AppState) -> AppResult<Option<i64>> {
    let today = crate::routes::automation::local_today();
    pelty_database::jobs::enqueue(
        &s.pool,
        JOB_KIND,
        json!({ "date": today.to_string() }),
        None,
        Some(&format!("autopay:{today}")),
        5,
    )
    .await
}

/// Build an enabled gateway that supports saved methods.
pub async fn recurring_gateway(s: &AppState, id: &str) -> AppResult<Box<dyn PaymentGateway>> {
    s.gateways
        .descriptor(id)
        .filter(|d| d.capabilities.recurring)
        .ok_or(AppError::Validation("error.unknown_gateway"))?;
    let row = pelty_database::extensions::gateway(&s.pool, id)
        .await?
        .filter(|r| r.enabled)
        .ok_or(AppError::Validation("error.unknown_gateway"))?;
    if !crate::addons::gateway_enabled(s, id).await? {
        return Err(AppError::Validation("error.unknown_gateway"));
    }
    s.gateways
        .build(id, &crate::routes::webhooks::resolve_config(&row))
        .map_err(|e| {
            tracing::error!(gateway = %id, error = %e, "gateway misconfigured");
            AppError::BadRequest("error.gateway")
        })
}

pub async fn run(s: &AppState, today: Date) -> AppResult<Value> {
    let cfg = autopay::get_settings(&s.pool).await?;
    let claims = autopay::claim_due(&s.pool, &cfg, today).await?;
    let (mut paid, mut pending, mut failed) = (0, 0, 0);
    for c in &claims {
        match charge(s, c).await {
            Ok("succeeded") => paid += 1,
            Ok("pending") => pending += 1,
            Ok(_) => failed += 1,
            Err(e) => {
                failed += 1;
                tracing::error!(error = %e, invoice = %c.invoice_id, "autopay attempt failed");
                let _ =
                    autopay::finish(&s.pool, c.attempt_id, "error", None, Some("internal")).await;
            }
        }
    }
    let notices = card_expiry_notices(s, today).await?;
    Ok(
        json!({ "attempts": claims.len(), "succeeded": paid, "pending": pending, "failed": failed, "card_expiry_notices": notices }),
    )
}

/// Mail each customer whose saved card expires this month or next, once per card.
pub async fn card_expiry_notices(s: &AppState, today: Date) -> AppResult<usize> {
    let cards = pelty_database::card_expiry::claim(&s.pool, today).await?;
    for c in &cards {
        let Some((to, name, locale)) =
            pelty_database::overdue::recipient(&s.pool, c.customer_id).await?
        else {
            continue;
        };
        let locale = locale
            .filter(|l| s.i18n.has(l))
            .unwrap_or_else(|| s.cfg.default_locale.clone());
        let card = format!("{} •••• {}", c.brand, c.last4);
        let expires = format!("{:02}/{}", c.exp_month, c.exp_year);
        let link = format!("{}/billing", s.cfg.public_url);
        crate::mail::enqueue(
            s,
            "credit_card_expiring",
            &locale,
            &to,
            crate::mail::vars(&[
                ("name", &name),
                ("card", &card),
                ("expires", &expires),
                ("link", &link),
            ]),
        )
        .await?;
        s.emit(DomainEvent::new(
            "payment_method.expiring",
            json!({ "payment_method_id": c.payment_method_id, "customer_id": c.customer_id }),
        ));
    }
    Ok(cards.len())
}

async fn charge(s: &AppState, c: &Claim) -> AppResult<&'static str> {
    let Some(inv) = invoices::get(&s.pool, c.invoice_id).await? else {
        return Err(AppError::NotFound);
    };
    let key = format!("autopay:{}:{}", c.invoice_id, c.attempt);
    let rules = pelty_database::gateway_rules::get(&s.pool, &c.method.gateway).await?;
    if let Err(code) = rules.check(&inv.currency, inv.balance()) {
        autopay::finish(
            &s.pool,
            c.attempt_id,
            "error",
            None,
            Some(code.trim_start_matches("error.")),
        )
        .await?;
        return Ok("error");
    }
    let pay = match invoices::create_pending(&s.pool, &inv, &c.method.gateway, None, &key).await {
        Ok(p) => p,
        Err(AppError::Conflict(_)) => {
            autopay::finish(
                &s.pool,
                c.attempt_id,
                "error",
                None,
                Some("invoice_not_payable"),
            )
            .await?;
            return Ok("error");
        }
        Err(e) => return Err(e),
    };
    let exp = pelty_database::branding::exponent(&s.pool, &inv.currency).await?;
    pelty_database::gateway_rules::record_fee(
        &s.pool,
        pay.id,
        rules.fee(&inv.currency, pay.amount, exp),
    )
    .await?;
    let currency =
        Currency::parse(&inv.currency).ok_or(AppError::Validation("error.invalid_currency"))?;
    let number = inv.number.clone().unwrap_or_default();
    let link = format!("{}/invoices/{}", s.cfg.public_url, inv.id);
    let req = PaymentRequest {
        payment_id: pay.id.to_string(),
        reference: number.clone(),
        amount: Money::new(pay.amount, currency),
        description: format!("Invoice {number}"),
        customer_email: None,
        success_url: link.clone(),
        cancel_url: link.clone(),
        webhook_url: format!("{}/api/v1/webhooks/{}", s.cfg.public_url, c.method.gateway),
        idempotency_key: format!("pelty-{key}"),
    };
    let outcome = match recurring_gateway(s, &c.method.gateway).await {
        Ok(gw) => gw
            .charge_saved(&c.method.token, &req)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, invoice = %inv.id, "charge_saved error");
                ChargeOutcome::Declined {
                    reason: "gateway_error".into(),
                }
            }),
        Err(_) => ChargeOutcome::Declined {
            reason: "gateway_unavailable".into(),
        },
    };
    match outcome {
        ChargeOutcome::Succeeded { provider_ref } => {
            invoices::set_provider_ref(&s.pool, pay.id, &provider_ref).await?;
            let a = invoices::apply_by_ref(&s.pool, &c.method.gateway, &provider_ref)
                .await?
                .ok_or(AppError::NotFound)?;
            autopay::finish(&s.pool, c.attempt_id, "succeeded", Some(pay.id), None).await?;
            s.emit(DomainEvent::new("payment.succeeded", json!({ "gateway": c.method.gateway, "provider_ref": provider_ref, "invoice_id": inv.id, "autopay": true })));
            if a.invoice_paid {
                s.emit(DomainEvent::new(
                    "invoice.paid",
                    json!({ "invoice_id": inv.id }),
                ));
                crate::changes::after_payment(s, inv.id).await;
            }
            Ok("succeeded")
        }
        ChargeOutcome::Pending { provider_ref } => {
            invoices::set_provider_ref(&s.pool, pay.id, &provider_ref).await?;
            autopay::finish(&s.pool, c.attempt_id, "pending", Some(pay.id), None).await?;
            Ok("pending")
        }
        ChargeOutcome::Declined { reason } => {
            autopay::fail_payment(&s.pool, pay.id).await?;
            autopay::finish(
                &s.pool,
                c.attempt_id,
                "declined",
                Some(pay.id),
                Some(&reason),
            )
            .await?;
            s.emit(DomainEvent::new("payment.failed", json!({ "invoice_id": inv.id, "payment_id": pay.id, "autopay": true, "attempt": c.attempt })));
            dunning(s, c, &number, &reason, &link).await?;
            Ok("declined")
        }
    }
}

async fn dunning(s: &AppState, c: &Claim, number: &str, reason: &str, link: &str) -> AppResult<()> {
    let Some((to, name, locale)) =
        pelty_database::overdue::recipient(&s.pool, c.customer_id).await?
    else {
        return Ok(());
    };
    let locale = locale
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let next = match c.next_retry {
        Some(d) => d.to_string(),
        None => s.i18n.t(&locale, "autopay.no_more_retries").to_string(),
    };
    let total = format!("{} {}", c.balance.normalize(), c.currency);
    let mut v = crate::mail::vars(&[
        ("invoice_number", number),
        ("total", &total),
        ("reason", reason),
        ("next_retry", &next),
        ("link", link),
    ]);
    v.insert("name".into(), name);
    crate::mail::enqueue(s, "payment_failed", &locale, &to, v).await
}
