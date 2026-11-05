//! Durable webhook delivery of domain events to external plugins.
//!
//! `emit` → one `plugin_deliveries` row + one `plugin.webhook` job per
//! subscribed, enabled external plugin. The worker POSTs
//! `{"id","event","created_at","data"}` with
//! `X-Pelty-Signature: sha256=<hex hmac(secret, "<timestamp>.<body>")>`,
//! `X-Pelty-Timestamp`, `X-Pelty-Event` and `X-Pelty-Delivery`. Any 2xx is
//! delivered; anything else retries with the job queue's backoff until
//! `MAX_ATTEMPTS`, then the delivery is marked failed.
use crate::worker::JobError;
use crate::AppState;
use pelty_database::jobs::{self, Job};
use pelty_database::plugin_webhooks as db;
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub const JOB_KIND: &str = "plugin.webhook";
pub const MAX_ATTEMPTS: i32 = 6;

/// Queue deliveries for every subscribed external plugin. Never fails the caller.
pub async fn on_event(s: &AppState, ev: &DomainEvent) {
    for (plugin, _) in s.plugins.webhook_targets(ev.name) {
        if let Err(e) = enqueue(s, &plugin, ev.name, &ev.payload).await {
            tracing::warn!(%plugin, event = ev.name, error = %e, "plugin delivery not queued");
        }
    }
}

async fn enqueue(
    s: &AppState,
    plugin: &str,
    event: &str,
    payload: &Value,
) -> pelty_core::AppResult<Uuid> {
    let id = db::create(&s.pool, plugin, event, payload).await?;
    queue_job(s, id, 0).await?;
    Ok(id)
}

async fn queue_job(s: &AppState, id: Uuid, round: i64) -> pelty_core::AppResult<()> {
    jobs::enqueue(
        &s.pool,
        JOB_KIND,
        json!({ "delivery_id": id }),
        None,
        Some(&format!("plugin-delivery:{id}:{round}")),
        MAX_ATTEMPTS,
    )
    .await?;
    Ok(())
}

/// Re-send a delivery (admin "redeliver"): a fresh job, same delivery id.
pub async fn redeliver(s: &AppState, id: Uuid) -> pelty_core::AppResult<()> {
    let d = db::get(&s.pool, id)
        .await?
        .ok_or(pelty_core::AppError::NotFound)?;
    db::reset(&s.pool, id).await?;
    queue_job(
        s,
        id,
        i64::from(d.attempts) + OffsetDateTime::now_utc().unix_timestamp(),
    )
    .await
}

/// 32 random bytes, hex. Shown to the admin once on rotation.
pub fn new_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// The plugin's signing secret, creating one on first use.
pub async fn secret(s: &AppState, plugin: &str) -> Result<String, String> {
    let sb = s
        .secret_box
        .as_ref()
        .ok_or("PELTY_ENCRYPTION_KEY is not set; cannot sign plugin webhooks")?;
    let (_, sealed) = db::endpoint(&s.pool, plugin)
        .await
        .map_err(|e| e.to_string())?;
    let sealed = match sealed {
        Some(x) => x,
        None => db::ensure_secret(&s.pool, plugin, &sb.seal(new_secret().as_bytes()))
            .await
            .map_err(|e| e.to_string())?,
    };
    sb.open(&sealed)
        .and_then(|v| String::from_utf8(v).ok())
        .ok_or_else(|| "stored plugin secret cannot be decrypted".into())
}

/// Effective URL: admin override, else the manifest `webhook_url`.
pub async fn url_for(s: &AppState, plugin: &str) -> Option<String> {
    let (o, _) = db::endpoint(&s.pool, plugin).await.ok()?;
    o.or_else(|| s.plugins.manifest(plugin).and_then(|m| m.webhook_url))
}

pub fn signature(secret: &str, ts: i64, body: &str) -> String {
    format!(
        "sha256={}",
        pelty_provisioning::template::sign(secret, format!("{ts}.{body}").as_bytes())
    )
}

pub async fn run_job(s: &AppState, job: &Job) -> Result<Value, JobError> {
    let id: Uuid = job
        .payload
        .get("delivery_id")
        .and_then(Value::as_str)
        .and_then(|x| x.parse().ok())
        .ok_or_else(|| JobError::Permanent("bad payload".into()))?;
    let d = db::get(&s.pool, id)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?
        .ok_or_else(|| JobError::Permanent("delivery gone".into()))?;
    if d.status == "delivered" {
        return Ok(json!({ "skipped": "already delivered" }));
    }
    // `attempts` already counts this run (the claim increments it).
    let last = job.attempts >= job.max_attempts;
    let gone = !s.plugins.exists(&d.plugin_id);
    let fail = |url: String, status: Option<u16>, err: String| async move {
        let permanent = last || gone;
        let _ = db::record(&s.pool, id, &url, status, Some(&err), false, permanent).await;
        Err(if permanent {
            JobError::Permanent(err)
        } else {
            JobError::Retry(err)
        })
    };
    let Some(url) = url_for(s, &d.plugin_id).await else {
        return fail(String::new(), None, "plugin has no webhook_url".into()).await;
    };
    let secret = match secret(s, &d.plugin_id).await {
        Ok(x) => x,
        Err(e) => return fail(url, None, e).await,
    };
    let body = json!({ "id": d.id, "event": d.event, "created_at": d.created_at.unix_timestamp(), "data": d.payload }).to_string();
    let ts = OffsetDateTime::now_utc().unix_timestamp();
    let sig = signature(&secret, ts, &body);
    let (ts_s, did) = (ts.to_string(), d.id.to_string());
    let headers = [
        ("x-pelty-event", d.event.as_str()),
        ("x-pelty-delivery", did.as_str()),
        ("x-pelty-timestamp", ts_s.as_str()),
        ("x-pelty-signature", sig.as_str()),
        ("user-agent", "PeltyCMS-Webhooks/1"),
    ];
    match pelty_provisioning::http::post_to(&url, &headers, body).await {
        Ok(st) if (200..300).contains(&st) => {
            db::record(&s.pool, id, &url, Some(st), None, true, false)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?;
            Ok(json!({ "status": st }))
        }
        Ok(st) => fail(url, Some(st), format!("HTTP {st}")).await,
        Err(e) => fail(url, None, e).await,
    }
}
