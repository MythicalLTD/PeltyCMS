//! Background job worker over the Postgres queue. Multiple server instances
//! can run it concurrently (SKIP LOCKED); the daily recurring-billing job is
//! deduplicated by its idempotency key.
use crate::AppState;
use pelty_database::jobs::{self, Job};
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use std::time::Duration;
use time::OffsetDateTime;

pub const RECURRING: &str = "billing.recurring";
/// Invoice renewals this many days before they are due.
pub const RENEW_AHEAD_DAYS: i64 = 7;
pub const STUCK_AFTER_SECS: i64 = 600;

/// Job failure: retried with backoff, or dead immediately.
pub enum JobError {
    Retry(String),
    Permanent(String),
}

impl From<String> for JobError {
    fn from(e: String) -> Self {
        JobError::Retry(e)
    }
}

async fn handle(s: &AppState, job: &Job) -> Result<Value, JobError> {
    match job.kind.as_str() {
        RECURRING => {
            crate::changes::sweep(s).await.map_err(|e| e.to_string())?;
            let today = OffsetDateTime::now_utc().date();
            let horizon = today + time::Duration::days(RENEW_AHEAD_DAYS);
            let mut total = 0;
            loop {
                let items = pelty_database::billable::bill_recurring(
                    &s.pool,
                    horizon,
                    RENEW_AHEAD_DAYS,
                    200,
                )
                .await
                .map_err(|e| e.to_string())?;
                for (inv, cust) in &items {
                    s.emit(DomainEvent::new(
                        "invoice.created",
                        json!({ "invoice_id": inv, "customer_id": cust, "billable": true }),
                    ));
                }
                total += items.len();
                if items.is_empty() {
                    break;
                }
            }
            loop {
                let batch = pelty_database::services::bill_due_ahead(
                    &s.pool,
                    today,
                    RENEW_AHEAD_DAYS,
                    200,
                    RENEW_AHEAD_DAYS,
                )
                .await
                .map_err(|e| e.to_string())?;
                for r in &batch {
                    s.emit(DomainEvent::new("invoice.created", json!({ "invoice_id": r.invoice_id, "customer_id": r.customer_id, "service_id": r.service_id, "renewal": true })));
                }
                total += batch.len();
                if batch.is_empty() {
                    break;
                }
            }
            Ok(json!({ "invoices_created": total }))
        }
        crate::approval::DUE_JOB => {
            let today = job.payload["date"]
                .as_str()
                .and_then(|d| {
                    time::Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok()
                })
                .unwrap_or_else(|| OffsetDateTime::now_utc().date());
            Ok(crate::approval::run_due(s, today)
                .await
                .map_err(|e| e.to_string())?)
        }
        crate::overdue::JOB_KIND => {
            let today = job.payload["date"]
                .as_str()
                .and_then(|d| {
                    time::Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok()
                })
                .unwrap_or_else(|| OffsetDateTime::now_utc().date());
            Ok(crate::overdue::run(s, today)
                .await
                .map_err(|e| e.to_string())?)
        }
        crate::routes::campaigns::JOB_KIND => {
            Ok(crate::routes::campaigns::run_job(s, &job.payload).await?)
        }
        crate::autopay::JOB_KIND => {
            let today = job.payload["date"]
                .as_str()
                .and_then(|d| {
                    time::Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok()
                })
                .unwrap_or_else(|| OffsetDateTime::now_utc().date());
            Ok(crate::autopay::run(s, today)
                .await
                .map_err(|e| e.to_string())?)
        }
        crate::mail::JOB_KIND => Ok(crate::mail::deliver(s, &job.payload).await?),
        crate::mail::RAW_JOB_KIND => Ok(crate::mail::deliver_raw(s, &job.payload).await?),
        pelty_database::provisioning::JOB_KIND => crate::provision::run_job(s, job).await,
        crate::routes::support::ESCALATE_JOB => {
            let n = pelty_database::support::run_escalations(&s.pool)
                .await
                .map_err(|e| e.to_string())?;
            let closed = crate::routes::support::run_auto_close(s)
                .await
                .map_err(|e| e.to_string())?;
            let sla = crate::routes::ticket_sla::run_scan(s)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "escalated": n, "auto_closed": closed, "sla_events": sla }))
        }
        crate::support_ext::PIPE_JOB => Ok(crate::support_ext::run_pipe(s, None)
            .await
            .map_err(|e| e.0.to_string())?),
        pelty_database::support_ext::INBOUND_NOTIFY_JOB => {
            Ok(crate::support_ext::notify_inbound(s, &job.payload).await?)
        }
        crate::routes::usage::BILL_JOB => {
            let sampled = crate::routes::usage::sample_all(s)
                .await
                .map_err(|e| e.0.to_string())?;
            let n =
                pelty_database::usage::bill_due(&s.pool, time::OffsetDateTime::now_utc().date())
                    .await
                    .map_err(|e| e.to_string())?;
            Ok(json!({ "invoices": n, "sampled": sampled }))
        }
        crate::domains::PROCESS_JOB => {
            let st = crate::domains::process(s)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!(st))
        }
        crate::domains::DAILY_JOB => {
            let st = crate::domains::daily(s, time::OffsetDateTime::now_utc().date())
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!(st))
        }
        ORPHAN_UPLOADS => {
            let n = pelty_database::ticket_attachments::purge_orphans(&s.pool, 24)
                .await
                .map_err(|e| e.to_string())?;
            let h = pelty_database::housekeeping::run(&s.pool)
                .await
                .map_err(|e| e.to_string())?;
            let erased = pelty_database::inactive_clients::run(&s.pool)
                .await
                .map_err(|e| e.to_string())?;
            let fixed = crate::routes::billable::run_fixed_term(s).await;
            let ssl = if pelty_database::housekeeping::get(&s.pool)
                .await
                .is_ok_and(|c| c.ssl_monitoring)
            {
                crate::ssl_probe::run(s, 20).await
            } else {
                0
            };
            Ok(
                json!({ "deleted": n, "housekeeping": h, "fixed_term_terminated": fixed, "ssl_checked": ssl, "inactive_clients_erased": erased }),
            )
        }
        // Used by tests and ops to exercise retry/backoff.
        crate::plugin_hooks::JOB_KIND => crate::plugin_hooks::run_job(s, job).await,
        crate::routes::store::RECOVERY_JOB => {
            let idle = job.payload["idle_minutes"]
                .as_i64()
                .unwrap_or(crate::routes::store::IDLE_MINUTES)
                .max(0);
            let n = crate::routes::store::run_recovery(s, idle)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "reminded": n }))
        }
        crate::updates::JOB_KIND => Ok(crate::updates::check(s)
            .await
            .map_err(|e| JobError::Retry(e.to_string()))?),
        crate::exchange_rates::JOB_KIND => crate::exchange_rates::update_now(s, None)
            .await
            .map_err(|e| JobError::Retry(e.to_string())),
        crate::notify::JOB_KIND => crate::notify::run_job(s, job).await,
        crate::webhooks_out::JOB_KIND => crate::webhooks_out::run_job(s, job).await,
        crate::addon_power::JOB_KIND => crate::addon_ext::run_job(s, &job.payload).await,
        crate::backup::JOB_KIND | "backup.pg_dump" => crate::backup::run_job(s, job).await,
        crate::routes::community::JOB_KIND => crate::routes::community::refresh(s)
            .await
            .map_err(|e| JobError::Retry(e.to_string())),
        crate::routes::game_delivery::JOB_KIND => crate::routes::game_delivery::process_rcon(s)
            .await
            .map_err(|e| JobError::Retry(e.to_string())),
        "test.fail" => Err(JobError::Retry("intentional failure".into())),
        "test.ok" => Ok(json!({ "ok": true })),
        other => Err(JobError::Retry(format!("unknown job kind {other}"))),
    }
}

/// Scheduled/system jobs reported as `automation.*` events. Per-message
/// and per-addon jobs (mail, webhooks, provisioning, addon jobs) are not:
/// they have their own events and would only add noise.
fn is_automation(kind: &str) -> bool {
    ![
        crate::mail::JOB_KIND,
        crate::mail::RAW_JOB_KIND,
        pelty_database::support_ext::INBOUND_NOTIFY_JOB,
        pelty_database::provisioning::JOB_KIND,
        crate::plugin_hooks::JOB_KIND,
        crate::notify::JOB_KIND,
        crate::webhooks_out::JOB_KIND,
        crate::addon_power::JOB_KIND,
        crate::routes::campaigns::JOB_KIND,
    ]
    .contains(&kind)
}

/// Claim and run up to `n` jobs once. Returns how many ran.
pub async fn run_once(s: &AppState, worker: &str, n: i64) -> pelty_core::AppResult<usize> {
    let claimed = jobs::claim(&s.pool, worker, n).await?;
    run_claimed(s, &claimed).await?;
    Ok(claimed.len())
}

/// Run jobs already claimed (status `running`) and record each outcome.
pub async fn run_claimed(s: &AppState, claimed: &[jobs::Job]) -> pelty_core::AppResult<()> {
    for job in claimed {
        match handle(s, job).await {
            Ok(v) => {
                if is_automation(&job.kind) {
                    s.emit(DomainEvent::new(
                        "automation.task_completed",
                        json!({ "kind": job.kind, "job_id": job.id, "result": v }),
                    ));
                }
                if let Some(t) = crate::routes::automation::task_of(s, &job.kind, &job.payload) {
                    crate::routes::automation::record_outcome(s, &t, None).await;
                }
                jobs::complete(&s.pool, job.id, v).await?
            }
            Err(e) => {
                let (st, e) = match e {
                    JobError::Retry(e) => (jobs::fail(&s.pool, job.id, &e).await?, e),
                    JobError::Permanent(e) => (jobs::fail_permanent(&s.pool, job.id, &e).await?, e),
                };
                if job.kind == pelty_database::provisioning::JOB_KIND && st == "dead" {
                    crate::provision::on_dead(s, job, &e).await;
                }
                // Only a run with no retries left counts towards the breaker.
                if st == "dead" {
                    if let Some(t) = crate::routes::automation::task_of(s, &job.kind, &job.payload)
                    {
                        crate::routes::automation::record_outcome(s, &t, Some(&e)).await;
                    }
                }
                if is_automation(&job.kind) {
                    s.emit(DomainEvent::new(
                        "automation.task_failed",
                        json!({ "kind": job.kind, "job_id": job.id, "status": st, "error": e }),
                    ));
                }
                tracing::warn!(job = job.id, kind = %job.kind, status = %st, error = %e, "job failed");
            }
        }
    }
    Ok(())
}

/// Enqueue today's recurring-billing run (no-op if already enqueued).
pub async fn schedule_recurring(s: &AppState) -> pelty_core::AppResult<Option<i64>> {
    let key = format!("recurring:{}", crate::routes::automation::local_today());
    jobs::enqueue(&s.pool, RECURRING, json!({}), None, Some(&key), 5).await
}

pub const ORPHAN_UPLOADS: &str = "uploads.purge_orphans";

/// Enqueue an hourly purge of never-linked ticket uploads (deduplicated).
pub async fn schedule_orphan_purge(s: &AppState) -> pelty_core::AppResult<Option<i64>> {
    let key = format!(
        "{ORPHAN_UPLOADS}:{}",
        OffsetDateTime::now_utc().unix_timestamp() / 3600
    );
    jobs::enqueue(&s.pool, ORPHAN_UPLOADS, json!({}), None, Some(&key), 3).await
}

/// Enqueue a ticket-escalation pass per 5-minute window (deduplicated).
pub async fn schedule_escalations(s: &AppState) -> pelty_core::AppResult<Option<i64>> {
    let now = OffsetDateTime::now_utc();
    let key = format!("tickets.escalate:{}", now.unix_timestamp() / 300);
    jobs::enqueue(
        &s.pool,
        crate::routes::support::ESCALATE_JOB,
        json!({}),
        None,
        Some(&key),
        3,
    )
    .await
}

/// One scheduler pass: recover stuck jobs and enqueue every due task
/// (deduplicated per window, so running it twice is harmless). The worker
/// loop calls it every `PELTY_SCHEDULER_INTERVAL_SECS` when it holds the
/// scheduler role; `pelty-server cron run` calls it from system cron.
pub async fn schedule_tick(s: &AppState) {
    let gate = crate::routes::automation::gate(s).await;
    let ok = |k: &str| gate.allows(k, crate::routes::automation::local_now());
    if let Err(e) = jobs::recover_stuck(&s.pool, STUCK_AFTER_SECS).await {
        tracing::error!(error = %e, "recover_stuck failed");
    }
    if !ok(crate::routes::support::ESCALATE_JOB) {
    } else if let Err(e) = schedule_escalations(s).await {
        tracing::error!(error = %e, "schedule_escalations failed");
    }
    if !ok(ORPHAN_UPLOADS) {
    } else if let Err(e) = schedule_orphan_purge(s).await {
        tracing::error!(error = %e, "schedule_orphan_purge failed");
    }
    if let Err(e) = crate::domains::schedule(s, &ok).await {
        tracing::error!(error = %e, "domains schedule failed");
    }
    if !ok(RECURRING) {
    } else if let Err(e) = schedule_recurring(s).await {
        tracing::error!(error = %e, "schedule_recurring failed");
    }
    crate::routes::automation::cron_tick(s, &gate).await;
    crate::addon_ext::schedule_jobs(s).await;
    if let Err(e) = crate::routes::game_delivery::schedule(s).await {
        tracing::error!(error = %e, "schedule game delivery failed");
    }
    if let Err(e) = crate::routes::community::schedule(s).await {
        tracing::error!(error = %e, "schedule community status failed");
    }
    if let Err(e) = crate::routes::store::schedule_recovery(s).await {
        tracing::error!(error = %e, "schedule cart recovery failed");
    }
    if let Err(e) = crate::approval::schedule_due(s).await {
        tracing::error!(error = %e, "schedule due reminders failed");
    }
    if !ok(crate::overdue::JOB_KIND) {
    } else if let Err(e) = crate::overdue::schedule(s).await {
        tracing::error!(error = %e, "schedule overdue failed");
    }
    if let Err(e) = crate::translations::reload(s).await {
        tracing::error!(error = %e, "translation overrides reload failed");
    }
    if let Err(e) = crate::updates::schedule(s).await {
        tracing::error!(error = %e, "schedule update check failed");
    }
    if let Err(e) = crate::exchange_rates::schedule(s).await {
        tracing::error!(error = %e, "schedule exchange rates failed");
    }
    if !ok(crate::support_ext::PIPE_JOB) {
    } else if let Err(e) = crate::support_ext::schedule(s).await {
        tracing::error!(error = %e, "schedule mail piping failed");
    }
    if !ok(crate::autopay::JOB_KIND) {
    } else if let Err(e) = crate::autopay::schedule(s).await {
        tracing::error!(error = %e, "schedule autopay failed");
    }
}

/// Spawn the worker loop. Stops when the runtime shuts down. Jobs are
/// claimed by every instance; scheduling runs only when `PELTY_SCHEDULER`
/// is on and, with Redis configured, only on the elected leader.
pub fn spawn(s: AppState) -> tokio::task::JoinHandle<()> {
    let worker = s.shared.instance.clone();
    tokio::spawn(async move {
        let mut last: Option<std::time::Instant> = None;
        let mut leader = false;
        let every = Duration::from_secs(s.cfg.scheduler_interval_secs);
        loop {
            if s.cfg.scheduler && last.is_none_or(|t| t.elapsed() >= every) {
                last = Some(std::time::Instant::now());
                let now_leader = crate::shared::is_scheduler_leader(&s, &worker, every).await;
                if now_leader {
                    // Startup or leader change: queue runs missed meanwhile
                    // (tasks with `missed = run_once`).
                    if !leader {
                        crate::routes::automation::catch_up(&s).await;
                    }
                    schedule_tick(&s).await;
                }
                leader = now_leader;
            }
            match run_once(&s, &worker, s.cfg.worker_concurrency).await {
                Ok(0) => tokio::time::sleep(Duration::from_secs(1)).await,
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "job worker error");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}
