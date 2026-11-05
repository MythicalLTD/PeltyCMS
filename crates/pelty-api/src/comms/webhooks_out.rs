//! Outgoing webhooks: every domain event is POSTed to each enabled
//! [`webhook_endpoints`](pelty_database::webhook_endpoints) row whose event
//! filter matches (`*`, `invoice.*` or an exact name).
//!
//! One `webhook_deliveries` row + one `webhook.deliver` job per endpoint.
//! Body: `{"id","event","created_at","data"}`. Headers: `X-Pelty-Event`,
//! `X-Pelty-Delivery`, `X-Pelty-Timestamp` and `X-Pelty-Signature:
//! sha256=<hex hmac(secret, "<timestamp>.<body>")>` (the plugin-webhook
//! scheme). 2xx = delivered; 4xx except 408/429 fails at once; anything else
//! retries with the job queue's exponential backoff up to [`MAX_ATTEMPTS`].
//! After [`DISABLE_AFTER`] deliveries in a row end failed, the endpoint is
//! disabled (`webhook_endpoint.disabled` event, audit entry). Staff can
//! redeliver any delivery: a new row that points at the original.
use crate::worker::JobError;
use crate::AppState;
use pelty_core::{AppError, AppResult};
use pelty_database::jobs::{self, Job};
use pelty_database::webhook_endpoints as db;
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub const JOB_KIND: &str = "webhook.deliver";
pub const MAX_ATTEMPTS: i32 = 8;
pub const DISABLE_AFTER: i32 = 20;
/// Synthetic event sent by the admin "Send test" button.
pub const PING_EVENT: &str = "webhook.ping";

/// Queue deliveries for every matching endpoint. Never fails the caller.
pub async fn on_event(s: &AppState, ev: &DomainEvent) {
    // Our own lifecycle events about endpoints are not echoed to endpoints.
    if ev.name.starts_with("webhook_endpoint.") {
        return;
    }
    let targets = match db::targets(&s.pool, ev.name).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "webhook endpoint lookup failed");
            return;
        }
    };
    for t in targets {
        if let Err(e) = enqueue(s, t.id, ev.name, &ev.payload, None).await {
            tracing::warn!(endpoint = %t.id, event = ev.name, error = %e, "webhook delivery not queued");
        }
    }
}

/// New delivery row + job.
pub async fn enqueue(s: &AppState, endpoint: Uuid, event: &str, payload: &Value, redelivery_of: Option<Uuid>) -> AppResult<Uuid> {
    let id = db::create_delivery(&s.pool, endpoint, event, payload, redelivery_of).await?;
    jobs::enqueue(&s.pool, JOB_KIND, json!({ "delivery_id": id }), None, Some(&format!("webhook-delivery:{id}")), MAX_ATTEMPTS).await?;
    Ok(id)
}

/// Re-send a delivery's event and payload as a new delivery.
pub async fn redeliver(s: &AppState, id: Uuid) -> AppResult<Uuid> {
    let d = db::delivery(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    enqueue(s, d.endpoint_id, &d.event, &d.payload, Some(d.id)).await
}

/// 64 hex chars, shown once.
pub fn new_secret() -> String {
    format!("whsec_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn retryable(status: u16) -> bool {
    !(400..500).contains(&status) || status == 408 || status == 429
}

pub async fn run_job(s: &AppState, job: &Job) -> Result<Value, JobError> {
    let id: Uuid = job.payload.get("delivery_id").and_then(Value::as_str).and_then(|x| x.parse().ok()).ok_or_else(|| JobError::Permanent("bad payload".into()))?;
    let d = db::delivery(&s.pool, id).await.map_err(|e| JobError::Retry(e.to_string()))?.ok_or_else(|| JobError::Permanent("delivery gone".into()))?;
    if d.status != "pending" {
        return Ok(json!({ "skipped": d.status }));
    }
    let ep = db::get(&s.pool, d.endpoint_id).await.map_err(|e| JobError::Retry(e.to_string()))?;
    // `attempts` already counts this run (the claim increments it).
    let last = job.attempts >= job.max_attempts;
    let started = std::time::Instant::now();
    let finish = |status: Option<u16>, err: Option<String>, ok: bool, permanent: bool| async move {
        let final_fail = !ok && (permanent || last);
        let ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
        let streak = db::record(&s.pool, id, status, err.as_deref(), ok, final_fail, ms).await.unwrap_or(0);
        if final_fail && streak >= DISABLE_AFTER && db::disable(&s.pool, d.endpoint_id, "too many failed deliveries").await.unwrap_or(false) {
            tracing::warn!(endpoint = %d.endpoint_id, "webhook endpoint disabled after repeated failures");
            let _ = pelty_database::audit::record(&s.pool, None, "webhook_endpoint.auto_disabled", Some(&d.endpoint_id.to_string()), None, json!({ "failed_in_a_row": streak })).await;
            s.emit(DomainEvent::new("webhook_endpoint.disabled", json!({ "endpoint_id": d.endpoint_id, "reason": "too many failed deliveries" })));
        }
        match (ok, err) {
            (true, _) => Ok(json!({ "status": status })),
            (false, e) if final_fail => Err(JobError::Permanent(e.unwrap_or_default())),
            (false, e) => Err(JobError::Retry(e.unwrap_or_default())),
        }
    };
    let Some(ep) = ep else { return finish(None, Some("endpoint deleted".into()), false, true).await };
    if !ep.enabled && d.event != PING_EVENT {
        return finish(None, Some("endpoint disabled".into()), false, true).await;
    }
    let sealed = db::secret(&s.pool, ep.id).await.map_err(|e| JobError::Retry(e.to_string()))?.unwrap_or_default();
    let Some(secret) = s.secret_box.as_ref().and_then(|sb| sb.open(&sealed)).and_then(|v| String::from_utf8(v).ok()) else {
        return finish(None, Some("signing secret unavailable (PELTY_ENCRYPTION_KEY)".into()), false, true).await;
    };
    let body = json!({ "id": d.id, "event": d.event, "created_at": d.created_at.unix_timestamp(), "data": d.payload }).to_string();
    let ts = OffsetDateTime::now_utc().unix_timestamp();
    let headers = vec![
        ("x-pelty-event".to_string(), d.event.clone()),
        ("x-pelty-delivery".to_string(), d.id.to_string()),
        ("x-pelty-timestamp".to_string(), ts.to_string()),
        ("x-pelty-signature".to_string(), crate::plugin_hooks::signature(&secret, ts, &body)),
    ];
    match s.notify_transport.post(&ep.url, headers, body).await {
        Ok(st) if (200..300).contains(&st) => finish(Some(st), None, true, false).await,
        Ok(st) => finish(Some(st), Some(format!("HTTP {st}")), false, !retryable(st)).await,
        Err(e) => finish(None, Some(e), false, false).await,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn retry_policy() {
        assert!(super::retryable(500));
        assert!(super::retryable(429));
        assert!(super::retryable(408));
        assert!(!super::retryable(404));
        assert!(!super::retryable(410));
    }
}
