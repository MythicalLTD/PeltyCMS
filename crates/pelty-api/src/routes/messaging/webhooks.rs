//! Inbound payment webhooks: verify → dedupe → apply → mark processed.
use crate::error::ApiResult;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::gateway_admin::NewDelivery;
use pelty_payments::{GatewayConfig, GatewayError, Headers, WebhookEvent, WebhookKind};
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new().route("/webhooks/{gateway}", post(receive))
}

/// Resolve env-referenced secrets for a gateway row.
pub fn resolve_config(row: &pelty_database::extensions::GatewayRow) -> GatewayConfig {
    let mut secrets = HashMap::new();
    if let Some(m) = row.secret_refs.as_object() {
        for (k, v) in m {
            if let Some(val) = v.as_str().and_then(|env| std::env::var(env).ok()) {
                secrets.insert(k.clone(), val);
            }
        }
    }
    GatewayConfig {
        id: row.id.clone(),
        config: row.config.clone(),
        secrets,
        ..Default::default()
    }
}

pub fn lower_headers(headers: &HeaderMap) -> Headers {
    headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_ascii_lowercase(), v.to_string()))
        })
        .collect()
}

pub fn kind_str(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::PaymentSucceeded => "payment_succeeded",
        WebhookKind::PaymentFailed => "payment_failed",
        WebhookKind::Refunded => "refunded",
        WebhookKind::Disputed => "disputed",
        WebhookKind::Ignored => "ignored",
    }
}

#[utoipa::path(post, path = "/api/v1/webhooks/{gateway}", request_body(content = String, content_type = "application/json"), responses((status = 200), (status = 400), (status = 404)))]
pub async fn receive(
    State(s): State<AppState>,
    Path(gw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let row = pelty_database::extensions::gateway(&s.pool, &gw)
        .await?
        .filter(|r| r.enabled)
        .ok_or(AppError::NotFound)?;
    let gateway = s.gateways.build(&gw, &resolve_config(&row)).map_err(|e| {
        tracing::error!(gateway = %gw, error = %e, "gateway misconfigured");
        AppError::NotFound
    })?;
    let hdrs = lower_headers(&headers);
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let mut log = NewDelivery {
        gateway: &gw,
        body_bytes: body.len().min(i32::MAX as usize) as i32,
        ..Default::default()
    };
    let ev = match gateway.handle_webhook(&hdrs, &body, now).await {
        Ok(ev) => ev,
        Err(e) => {
            let msg = e.to_string();
            let (code, err) = match e {
                GatewayError::InvalidSignature => {
                    (400, AppError::BadRequest("error.webhook_signature"))
                }
                GatewayError::Unsupported => (404, AppError::NotFound),
                _ => (400, AppError::BadRequest("error.bad_request")),
            };
            log.status_code = code;
            log.error = Some(&msg);
            pelty_database::gateway_admin::log_delivery(&s.pool, &log).await?;
            return Err(err.into());
        }
    };
    log.signature_ok = true;
    log.event_id = Some(&ev.event_id);
    log.event_type = Some(&ev.event_type);
    log.kind = Some(kind_str(&ev.kind));
    log.provider_ref = ev.provider_ref.as_deref();
    log.status_code = 200;
    if !pelty_database::extensions::record_webhook(&s.pool, &gw, &ev.event_id, &ev.event_type)
        .await?
    {
        tracing::info!(gateway = %gw, event = %ev.event_id, "duplicate webhook ignored");
        log.duplicate = true;
        pelty_database::gateway_admin::log_delivery(&s.pool, &log).await?;
        return Ok(ack(&gw, json!({ "received": true, "duplicate": true })));
    }
    let updated = apply_event(&s, &gw, &ev).await?;
    pelty_database::extensions::mark_webhook_processed(&s.pool, &gw, &ev.event_id).await?;
    pelty_database::gateway_admin::log_delivery(&s.pool, &log).await?;
    Ok(ack(
        &gw,
        json!({ "received": true, "duplicate": false, "payments_updated": updated }),
    ))
}

/// Adyen-style notifications expect the literal body `[accepted]`.
fn ack(gw: &str, v: Value) -> Response {
    if gw == "adyen" {
        "[accepted]".into_response()
    } else {
        Json(v).into_response()
    }
}

/// Apply a verified event to payments/invoices. Returns rows updated.
pub async fn apply_event(s: &AppState, gw: &str, ev: &WebhookEvent) -> ApiResult<u64> {
    let status = match ev.kind {
        WebhookKind::PaymentSucceeded => Some("succeeded"),
        WebhookKind::PaymentFailed => Some("failed"),
        WebhookKind::Refunded => Some("refunded"),
        WebhookKind::Disputed => {
            use pelty_extensions::events::DomainEvent;
            s.emit(DomainEvent::new(
                "payment.disputed",
                json!({ "gateway": gw, "provider_ref": ev.provider_ref, "payment_id": ev.payment_id, "event_type": ev.event_type }),
            ));
            pelty_database::audit::record(
                &s.pool,
                None,
                "payment.disputed",
                ev.provider_ref.as_deref(),
                None,
                json!({ "gateway": gw, "event_id": ev.event_id, "payment_id": ev.payment_id }),
            )
            .await?;
            None
        }
        WebhookKind::Ignored => None,
    };
    // Prefer the provider reference; fall back to our echoed payment id.
    let mut pref = ev.provider_ref.clone();
    if let Some(pid) = ev.payment_id.as_deref() {
        let known = match pref.as_deref() {
            Some(r) => pelty_database::gateway_admin::payment_exists_by_ref(&s.pool, gw, r).await?,
            None => false,
        };
        if !known {
            if let Some(r) =
                pelty_database::gateway_admin::provider_ref_for(&s.pool, gw, pid).await?
            {
                pref = Some(r);
            }
        }
    }
    let mut updated = 0;
    if let (Some(st), Some(r)) = (status, pref.as_deref()) {
        use pelty_extensions::events::DomainEvent;
        if st == "succeeded" {
            // Credits the linked invoice exactly once (applied_at guard).
            match s.invoices().apply_by_ref(gw, r).await? {
                Some(a) => {
                    updated = 1;
                    if !a.duplicate {
                        s.emit(DomainEvent::new(
                            "payment.succeeded",
                            json!({ "gateway": gw, "provider_ref": r, "invoice_id": a.invoice_id }),
                        ));
                    }
                    if a.invoice_paid {
                        s.emit(DomainEvent::new(
                            "invoice.paid",
                            json!({ "invoice_id": a.invoice_id }),
                        ));
                        crate::changes::after_payment(s, a.invoice_id).await;
                    }
                }
                None => {
                    updated =
                        pelty_database::extensions::set_payment_status_by_ref(&s.pool, gw, r, st)
                            .await?;
                    if updated > 0 {
                        s.emit(DomainEvent::new(
                            "payment.succeeded",
                            json!({ "gateway": gw, "provider_ref": r }),
                        ));
                    }
                }
            }
        } else {
            updated =
                pelty_database::extensions::set_payment_status_by_ref(&s.pool, gw, r, st).await?;
            if st == "failed" && updated > 0 {
                s.emit(DomainEvent::new(
                    "payment.failed",
                    json!({ "gateway": gw, "provider_ref": r }),
                ));
            }
        }
    }
    Ok(updated)
}
