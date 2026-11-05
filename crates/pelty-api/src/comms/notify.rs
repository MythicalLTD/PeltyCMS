//! Staff notifications (Slack / Discord / Teams / Telegram / SMS gateway /
//! email-to-SMS / HMAC-signed generic webhook) fanned out from domain events
//! through the job queue, with a per-delivery log. The HTTP transport is a
//! trait so tests can record deliveries instead of reaching the network.
//!
//! Signed webhooks carry `X-Pelty-Event`, `X-Pelty-Delivery`,
//! `X-Pelty-Timestamp` and `X-Pelty-Signature: sha256=<hex hmac(secret,
//! "<timestamp>.<body>")>` (same scheme as plugin webhooks).
use crate::worker::JobError;
use crate::AppState;
use pelty_database::{
    admin_tools,
    jobs::{self, Job},
};
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type PostFut<'a> = Pin<Box<dyn Future<Output = Result<u16, String>> + Send + 'a>>;

pub const JOB_KIND: &str = "notify.deliver";

pub trait NotifyTransport: Send + Sync {
    fn post<'a>(
        &'a self,
        url: &'a str,
        headers: Vec<(String, String)>,
        body: String,
    ) -> PostFut<'a>;
}

/// Real transport (SSRF-guarded client shared with provisioning).
pub struct HttpNotify;

impl NotifyTransport for HttpNotify {
    fn post<'a>(
        &'a self,
        url: &'a str,
        headers: Vec<(String, String)>,
        body: String,
    ) -> PostFut<'a> {
        Box::pin(async move {
            let mut h: Vec<(&str, &str)> = headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            h.push(("user-agent", "PeltyCMS-Notify/1"));
            pelty_provisioning::http::post_to(url, &h, body).await
        })
    }
}

/// Test transport: records `(url, body)` and answers with `status`.
pub struct MemoryNotify {
    pub sent: Mutex<Vec<(String, Value)>>,
    /// Headers of each send, same order as `sent`.
    pub headers: Mutex<Vec<Vec<(String, String)>>>,
    /// Raw bodies, same order as `sent` (to verify signatures).
    pub raw: Mutex<Vec<String>>,
    pub status: u16,
}

impl Default for MemoryNotify {
    fn default() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
            headers: Mutex::new(Vec::new()),
            raw: Mutex::new(Vec::new()),
            status: 200,
        }
    }
}

impl NotifyTransport for MemoryNotify {
    fn post<'a>(
        &'a self,
        url: &'a str,
        headers: Vec<(String, String)>,
        body: String,
    ) -> PostFut<'a> {
        let v = serde_json::from_str(&body).unwrap_or(Value::Null);
        if let (Ok(mut g), Ok(mut h), Ok(mut r)) =
            (self.sent.lock(), self.headers.lock(), self.raw.lock())
        {
            g.push((url.to_string(), v));
            h.push(headers);
            r.push(body);
        }
        let st = self.status;
        Box::pin(async move { Ok(st) })
    }
}

pub fn default_transport() -> Arc<dyn NotifyTransport> {
    Arc::new(HttpNotify)
}

/// Short human summary of an event's data for the message line.
pub fn summary(ev: &DomainEvent) -> String {
    let d = &ev.payload;
    let pick = |k: &str| d.get(k).and_then(Value::as_str).map(str::to_string);
    let mut parts = Vec::new();
    for k in [
        "invoice_id",
        "order_id",
        "service_id",
        "customer_id",
        "ticket_id",
        "domain",
        "chat_id",
        "email",
    ] {
        if let Some(v) = pick(k) {
            parts.push(format!("{k}={v}"));
        }
    }
    if parts.is_empty() {
        "event received".into()
    } else {
        parts.join(" ")
    }
}

/// Enqueue one delivery job per enabled rule subscribed to this event.
pub async fn on_event(s: &AppState, ev: &DomainEvent) {
    let rules = match admin_tools::rules_for(&s.pool, ev.name).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "notification rules lookup failed");
            return;
        }
    };
    let text = summary(ev);
    for r in rules {
        let _ = enqueue(s, &r, ev.name, &text, &ev.payload).await;
    }
}

/// Queue one delivery for a rule (also used by the admin "send test").
pub async fn enqueue(
    s: &AppState,
    r: &admin_tools::ActiveRule,
    event: &str,
    text: &str,
    data: &Value,
) -> pelty_core::AppResult<uuid::Uuid> {
    let mut body = admin_tools::payload(&r.provider, event, text, None);
    let did = admin_tools::create_delivery(&s.pool, r.id, event).await?;
    match r.provider.as_str() {
        "telegram" => body["chat_id"] = json!(r.target),
        "sms_webhook" => body["to"] = json!(r.target),
        "webhook" => {
            body = json!({ "id": did, "event": event, "created_at": time::OffsetDateTime::now_utc().unix_timestamp(), "text": text, "data": data });
        }
        _ => {}
    }
    jobs::enqueue(&s.pool, JOB_KIND, json!({ "url": r.url, "body": body, "event": event, "delivery_id": did, "rule_id": r.id, "provider": r.provider, "target": r.target }), None, None, MAX_ATTEMPTS).await?;
    Ok(did)
}

pub const MAX_ATTEMPTS: i32 = 5;

/// HMAC secret for a signed webhook rule, if one is set.
fn rule_secret(s: &AppState, sealed: Option<&[u8]>) -> Result<Option<String>, String> {
    let Some(sealed) = sealed else {
        return Ok(None);
    };
    let sb = s
        .secret_box
        .as_ref()
        .ok_or("PELTY_ENCRYPTION_KEY is not set; cannot sign webhooks")?;
    sb.open(sealed)
        .and_then(|v| String::from_utf8(v).ok())
        .map(Some)
        .ok_or_else(|| "stored webhook secret cannot be decrypted".into())
}

pub async fn run_job(s: &AppState, job: &Job) -> Result<Value, JobError> {
    let url = job
        .payload
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| JobError::Permanent("bad payload".into()))?
        .to_string();
    let body = job.payload.get("body").cloned().unwrap_or(Value::Null);
    let did = job
        .payload
        .get("delivery_id")
        .and_then(Value::as_str)
        .and_then(|x| x.parse::<uuid::Uuid>().ok());
    let rule_id = job
        .payload
        .get("rule_id")
        .and_then(Value::as_str)
        .and_then(|x| x.parse::<uuid::Uuid>().ok());
    let provider = job
        .payload
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // `attempts` already counts this run (the claim increments it).
    let last = job.attempts >= job.max_attempts;
    let log = |status: Option<u16>, err: Option<String>, ok: bool, permanent: bool| async move {
        if let Some(d) = did {
            let _ = admin_tools::record_delivery(
                &s.pool,
                d,
                status,
                err.as_deref(),
                ok,
                permanent || (last && !ok),
            )
            .await;
        }
    };
    // Email-to-SMS goes through the configured mailer, not HTTP.
    if provider == "sms_email" {
        let to = job
            .payload
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let text = body
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let email = pelty_notifications::Email {
            to,
            subject: job
                .payload
                .get("event")
                .and_then(Value::as_str)
                .unwrap_or("notification")
                .to_string(),
            html: pelty_notifications::template::text_to_html(&text),
            text,
            list_unsubscribe: None,
        };
        return match s.active_mailer().send(&email).await {
            Ok(()) => {
                log(None, None, true, false).await;
                Ok(json!({ "mailed": true }))
            }
            Err(e) => {
                log(None, Some(e.clone()), false, false).await;
                Err(JobError::Retry(e))
            }
        };
    }
    let raw = body.to_string();
    let mut headers = Vec::new();
    if provider == "webhook" {
        let sealed = match rule_id {
            Some(r) => admin_tools::rule(&s.pool, r)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?
                .and_then(|r| r.secret_sealed),
            None => None,
        };
        match rule_secret(s, sealed.as_deref()) {
            Ok(Some(secret)) => {
                let ts = time::OffsetDateTime::now_utc().unix_timestamp();
                headers.push(("x-pelty-timestamp".to_string(), ts.to_string()));
                headers.push((
                    "x-pelty-signature".to_string(),
                    crate::plugin_hooks::signature(&secret, ts, &raw),
                ));
            }
            Ok(None) => {}
            Err(e) => {
                log(None, Some(e.clone()), false, true).await;
                return Err(JobError::Permanent(e));
            }
        }
        headers.push((
            "x-pelty-event".to_string(),
            job.payload
                .get("event")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ));
        if let Some(d) = did {
            headers.push(("x-pelty-delivery".to_string(), d.to_string()));
        }
    }
    match s.notify_transport.post(&url, headers, raw).await {
        Ok(st) if (200..300).contains(&st) => {
            log(Some(st), None, true, false).await;
            Ok(json!({ "status": st }))
        }
        Ok(st) if (400..500).contains(&st) && st != 429 => {
            log(Some(st), Some(format!("HTTP {st}")), false, true).await;
            Err(JobError::Permanent(format!("HTTP {st}")))
        }
        Ok(st) => {
            log(Some(st), Some(format!("HTTP {st}")), false, false).await;
            Err(JobError::Retry(format!("HTTP {st}")))
        }
        Err(e) => {
            log(None, Some(e.clone()), false, false).await;
            Err(JobError::Retry(e))
        }
    }
}
