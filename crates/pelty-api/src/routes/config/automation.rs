//! Automation (cron) dashboard: the built-in scheduled tasks, their last
//! outcome and next run, plus a manual "run now" (`jobs.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, job_stats, jobs};
use serde_json::{json, Value};
use time::OffsetDateTime;

/// (kind, interval in seconds) for every task the worker schedules itself.
pub const TASKS: &[(&str, i64)] = &[
    (crate::worker::RECURRING, 86_400),
    (crate::overdue::JOB_KIND, 86_400),
    (crate::autopay::JOB_KIND, 86_400),
    (crate::domains::DAILY_JOB, 86_400),
    (crate::domains::PROCESS_JOB, 300),
    (crate::routes::support::ESCALATE_JOB, 300),
    (crate::routes::usage::BILL_JOB, 3_600),
    (crate::worker::ORPHAN_UPLOADS, 3_600),
    (crate::support_ext::PIPE_JOB, 300),
];

/// Per-task settings loaded once per scheduler tick.
#[derive(Default)]
pub struct Gate(
    pub std::collections::HashMap<String, (bool, Option<i16>)>,
    pub std::collections::HashMap<String, (String, i64)>,
);

impl Gate {
    /// Disabled tasks are never auto-scheduled; tasks with a run hour wait
    /// until that hour of the company's day (`now` in company time; the daily
    /// keys use the company date too, see [`local_today`]).
    /// Tasks with a cron expression are queued by [`cron_tick`] instead.
    pub fn allows(&self, kind: &str, now: OffsetDateTime) -> bool {
        if self.1.contains_key(kind) {
            return false;
        }
        match self.0.get(kind) {
            None => true,
            Some((false, _)) => false,
            Some((true, None)) => true,
            Some((true, Some(h))) => i16::from(now.hour()) >= *h,
        }
    }
}

/// UTC offset (seconds) of the company time zone (Admin > General >
/// Localisation), refreshed every scheduler tick. Postgres resolves the zone
/// name, so no time zone database is bundled.
static OFFSET: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Now in the company time zone.
pub fn local_now() -> OffsetDateTime {
    let off =
        time::UtcOffset::from_whole_seconds(OFFSET.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(time::UtcOffset::UTC);
    OffsetDateTime::now_utc().to_offset(off)
}

/// Today in the company time zone: the dedupe date of the daily tasks.
pub fn local_today() -> time::Date {
    local_now().date()
}

/// Refresh [`OFFSET`] from the company time zone; unknown zones count as UTC.
pub async fn refresh_offset(s: &AppState) -> String {
    let tz = pelty_database::general_settings::load(&s.pool)
        .await
        .map(|g| g.localisation.timezone)
        .unwrap_or_else(|_| "UTC".into());
    let off: Option<i32> = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (now() AT TIME ZONE $1) - (now() AT TIME ZONE 'UTC'))::int",
    )
    .bind(&tz)
    .fetch_one(&s.pool)
    .await
    .ok();
    OFFSET.store(off.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    if off.is_none() {
        "UTC".into()
    } else {
        tz
    }
}

/// Load the gate; on DB errors fall back to "everything enabled".
pub async fn gate(s: &AppState) -> Gate {
    refresh_offset(s).await;
    crate::addon_ext::refresh_runtime_cron(&s.pool).await;
    match pelty_database::automation_settings::all(&s.pool).await {
        Ok(v) => {
            let mut g = Gate(
                v.iter()
                    .map(|r| (r.kind.clone(), (r.enabled, r.run_hour)))
                    .collect(),
                v.into_iter()
                    .filter_map(|r| Some((r.kind, (r.schedule?, r.updated_at.unix_timestamp()))))
                    .collect(),
            );
            // Addon cron jobs run on their manifest schedule unless an admin
            // changed it; no catch-up of runs from before this process.
            static STARTED: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
            let started = *STARTED.get_or_init(|| OffsetDateTime::now_utc().unix_timestamp());
            for (name, addon, job, _, cron) in addon_tasks(s) {
                if let Some(c) = cron {
                    // Runtime jobs: no run before their registration.
                    let since = crate::addon_ext::runtime_cron(&addon)
                        .iter()
                        .find(|j| j.0 == job)
                        .map_or(started, |j| j.3.max(started));
                    g.1.entry(name).or_insert((c, since));
                }
            }
            g
        }
        Err(e) => {
            tracing::warn!(error = %e, "automation settings unavailable");
            Gate::default()
        }
    }
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct SettingBody {
    pub enabled: bool,
    pub run_hour: Option<i16>,
}

#[utoipa::path(put, path = "/api/v1/admin/automation/{kind}/settings", responses((status = 200), (status = 404)))]
pub async fn put_setting(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
    Path(kind): Path<String>,
    Json(b): Json<SettingBody>,
) -> ApiResult<Json<Value>> {
    let Some((kind, interval)) = TASKS
        .iter()
        .find(|t| t.0 == kind)
        .map(|t| (t.0.to_string(), t.1))
        .or_else(|| addon_task(&s, &kind).map(|_| (kind.clone(), 0)))
    else {
        return Err(AppError::NotFound.into());
    };
    let kind = kind.as_str();
    // A start hour only makes sense for daily tasks.
    if b.run_hour
        .is_some_and(|h| !(0..=23).contains(&h) || interval < 86_400)
    {
        return Err(AppError::Validation("error.invalid_run_hour").into());
    }
    pelty_database::automation_settings::upsert(&s.pool, kind, b.enabled, b.run_hour).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "automation.settings",
        None,
        u.ip.as_deref(),
        json!({ "kind": kind, "enabled": b.enabled, "run_hour": b.run_hour }),
    )
    .await?;
    Ok(Json(
        json!({ "kind": kind, "enabled": b.enabled, "run_hour": b.run_hour }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/automation/tasks", responses((status = 200)))]
pub async fn list_tasks(
    State(s): State<AppState>,
    _p: RequirePermission<perms::JobsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "tasks": tasks(&s).await?, "timezone": refresh_offset(&s).await }),
    ))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct ScheduleBody {
    /// Five-field cron expression in the company time zone; null or empty
    /// = back to the built-in interval.
    pub schedule: Option<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/automation/{kind}/schedule", responses((status = 200), (status = 404), (status = 422)))]
pub async fn put_schedule(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
    Path(kind): Path<String>,
    Json(b): Json<ScheduleBody>,
) -> ApiResult<Json<Value>> {
    let next = set_task_schedule(&s, &kind, b.schedule.as_deref()).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "automation.schedule",
        None,
        u.ip.as_deref(),
        json!({ "kind": kind, "schedule": b.schedule }),
    )
    .await?;
    Ok(Json(
        json!({ "kind": kind, "schedule": b.schedule.filter(|e| !e.trim().is_empty()), "next_runs": next }),
    ))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct PreviewBody {
    pub schedule: String,
}

/// Validate an expression and show its next five runs (company time zone).
#[utoipa::path(post, path = "/api/v1/admin/automation/cron-preview", responses((status = 200), (status = 422)))]
pub async fn cron_preview(
    State(s): State<AppState>,
    _p: RequirePermission<perms::JobsManage>,
    Json(b): Json<PreviewBody>,
) -> ApiResult<Json<Value>> {
    let tz = refresh_offset(&s).await;
    let c =
        crate::cron::parse(&b.schedule).map_err(|_| AppError::Validation("error.invalid_cron"))?;
    Ok(Json(
        json!({ "description": crate::cron::describe(&c), "next_runs": crate::cron::next_runs(&c, local_now(), 5), "timezone": tz }),
    ))
}

/// Clear a tripped circuit breaker and enable the task again.
#[utoipa::path(post, path = "/api/v1/admin/automation/{kind}/reset", responses((status = 200), (status = 404)))]
pub async fn reset_task(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
    Path(kind): Path<String>,
) -> ApiResult<Json<Value>> {
    reset_breaker(&s, &kind).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "automation.task_reset",
        None,
        u.ip.as_deref(),
        json!({ "kind": kind }),
    )
    .await?;
    Ok(Json(
        json!({ "kind": kind, "enabled": true, "failures": 0 }),
    ))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct MissedBody {
    /// `skip` (default) or `run_once`: run once on scheduler start or
    /// leader change when the last scheduled run was missed.
    pub missed: String,
}

#[utoipa::path(put, path = "/api/v1/admin/automation/{kind}/missed", responses((status = 200), (status = 404), (status = 422)))]
pub async fn put_missed(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
    Path(kind): Path<String>,
    Json(b): Json<MissedBody>,
) -> ApiResult<Json<Value>> {
    set_task_missed(&s, &kind, &b.missed).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "automation.missed_policy",
        None,
        u.ip.as_deref(),
        json!({ "kind": kind, "missed": b.missed }),
    )
    .await?;
    Ok(Json(json!({ "kind": kind, "missed": b.missed })))
}

#[derive(serde::Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/automation/{kind}/history", responses((status = 200), (status = 404)))]
pub async fn task_history(
    State(s): State<AppState>,
    _p: RequirePermission<perms::JobsManage>,
    Path(kind): Path<String>,
    axum::extract::Query(q): axum::extract::Query<HistoryQuery>,
) -> ApiResult<Json<Value>> {
    if !TASKS.iter().any(|t| t.0 == kind) && addon_task(&s, &kind).is_none() {
        return Err(AppError::NotFound.into());
    }
    Ok(Json(
        json!({ "runs": history(&s, &kind, q.limit.unwrap_or(20).clamp(1, 100)).await? }),
    ))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/automation", get(overview))
        .route("/admin/automation/{kind}/run", post(run_now))
        .route(
            "/admin/automation/{kind}/settings",
            axum::routing::put(put_setting),
        )
        .route("/admin/automation/tasks", get(list_tasks))
        .route("/admin/automation/cron-preview", post(cron_preview))
        .route(
            "/admin/automation/{kind}/schedule",
            axum::routing::put(put_schedule),
        )
        .route("/admin/automation/{kind}/history", get(task_history))
        .route("/admin/automation/{kind}/reset", post(reset_task))
        .route(
            "/admin/automation/{kind}/missed",
            axum::routing::put(put_missed),
        )
}

/// Health of a task given its last finish and interval.
pub fn health(
    last_status: Option<&str>,
    last: Option<OffsetDateTime>,
    interval: i64,
    now: OffsetDateTime,
) -> &'static str {
    match (last_status, last) {
        (None, _) | (_, None) => "never",
        (Some("dead" | "failed"), _) => "failing",
        (Some(_), Some(t)) if (now - t).whole_seconds() > interval * 2 + 120 => "late",
        _ => "ok",
    }
}

#[utoipa::path(get, path = "/api/v1/admin/automation", responses((status = 200), (status = 403)))]
pub async fn overview(
    State(s): State<AppState>,
    _p: RequirePermission<perms::JobsManage>,
) -> ApiResult<Json<Value>> {
    let kinds: Vec<&str> = TASKS.iter().map(|t| t.0).collect();
    let now = OffsetDateTime::now_utc();
    let stats = job_stats::for_kinds(&s.pool, &kinds).await?;
    let g = gate(&s).await;
    let tasks: Vec<Value> = stats
        .into_iter()
        .map(|st| {
            let interval = TASKS.iter().find(|t| t.0 == st.kind).map_or(0, |t| t.1);
            let h = health(st.last_status.as_deref(), st.last_finished_at, interval, now);
            let (enabled, run_hour) = g.0.get(&st.kind).copied().unwrap_or((true, None));
            json!({ "kind": st.kind, "interval_secs": interval, "health": h, "enabled": enabled, "run_hour": run_hour, "stats": st })
        })
        .collect();
    let totals: serde_json::Map<String, Value> = job_stats::totals(&s.pool)
        .await?
        .into_iter()
        .map(|(k, n)| (k, json!(n)))
        .collect();
    // Scheduled addon jobs (`<addon-id>:<op>`), same shape as `tasks()`.
    let addon_tasks: Vec<Value> = self::tasks(&s)
        .await?
        .into_iter()
        .filter(|t| t["source"] != "built-in")
        .collect();
    Ok(Json(
        json!({ "tasks": tasks, "addon_tasks": addon_tasks, "totals": totals }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/automation/{kind}/run", responses((status = 200), (status = 404)))]
pub async fn run_now(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
    Path(kind): Path<String>,
) -> ApiResult<Json<Value>> {
    let id = run_task(&s, &kind).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "job.enqueued",
        None,
        u.ip.as_deref(),
        json!({ "kind": kind, "id": id }),
    )
    .await?;
    Ok(Json(json!({ "id": id, "duplicate": id.is_none() })))
}

// ----- task registry (console `cron ...`, `pelty-server cron run <task>`) -----

/// A scheduled addon job: (`<addon-id>:<op>`, addon, op, interval, cron).
/// Interval jobs come from `capabilities.jobs` (`every_secs`), cron jobs
/// (ABI 0.5, `on-cron`) from `capabilities.cron` with their default
/// expression. Only enabled, loaded addons count, so disabling or
/// uninstalling an addon removes its tasks.
pub fn addon_tasks(s: &AppState) -> Vec<(String, String, String, u64, Option<String>)> {
    crate::addon_ext::loaded_all(s)
        .iter()
        .flat_map(|a| {
            let jobs = a.caps.jobs.iter().filter(|j| j.every_secs >= 60).map(|j| {
                (
                    format!("{}:{}", a.id, j.op),
                    a.id.clone(),
                    j.op.clone(),
                    j.every_secs,
                    None,
                )
            });
            let cron = a.caps.cron.iter().map(|j| {
                (
                    format!("{}:{}", a.id, j.name),
                    a.id.clone(),
                    j.name.clone(),
                    0,
                    Some(j.schedule.clone()),
                )
            });
            // Runtime jobs (`power.cron-register`), only while the addon still holds the cron approval.
            let runtime = if a.caps.cron.is_empty() && !a.caps.cron_runtime {
                vec![]
            } else {
                crate::addon_ext::runtime_cron(&a.id)
            };
            let runtime = runtime
                .into_iter()
                .map(|j| (format!("{}:{}", a.id, j.0), a.id.clone(), j.0, 0, Some(j.1)));
            jobs.chain(cron).chain(runtime).collect::<Vec<_>>()
        })
        .collect()
}

/// Earliest time the scheduler queues a task again: one interval after its
/// last run (daily tasks may wait further for their run hour), or now.
fn next_estimate(last: Option<OffsetDateTime>, interval: i64, now: OffsetDateTime) -> i64 {
    last.map_or(now.unix_timestamp(), |t| {
        (t.unix_timestamp() + interval).max(now.unix_timestamp())
    })
}

/// Every scheduled task, built-in first, then addon jobs (`<addon>:<op>`):
/// name, interval or cron schedule, enabled, run hour, health, the last job
/// (status, finish, duration, error) and the next runs.
pub async fn tasks(s: &AppState) -> pelty_core::AppResult<Vec<Value>> {
    let now = OffsetDateTime::now_utc();
    let g = gate(s).await;
    let mut list: Vec<(String, String, i64, Option<i16>)> = TASKS
        .iter()
        .map(|t| {
            (
                t.0.to_string(),
                "built-in".to_string(),
                t.1,
                g.0.get(t.0).and_then(|e| e.1),
            )
        })
        .collect();
    let defaults: std::collections::HashMap<String, String> = addon_tasks(s)
        .into_iter()
        .filter_map(|t| Some((t.0, t.4?)))
        .collect();
    list.extend(
        addon_tasks(s)
            .into_iter()
            .map(|(name, addon, _, every, _)| (name, addon, every as i64, None)),
    );
    let rows: std::collections::HashMap<
        String,
        pelty_database::automation_settings::AutomationSetting,
    > = pelty_database::automation_settings::all(&s.pool)
        .await?
        .into_iter()
        .map(|r| (r.kind.clone(), r))
        .collect();
    let mut out = Vec::with_capacity(list.len());
    for (name, source, interval, run_hour) in list {
        let row = rows.get(&name);
        let enabled = g.0.get(&name).is_none_or(|e| e.0);
        let last = history(s, &name, 1).await?.into_iter().next();
        let fin = last
            .as_ref()
            .and_then(|l| l["finished_at"].as_i64())
            .and_then(|t| OffsetDateTime::from_unix_timestamp(t).ok());
        let st = last
            .as_ref()
            .and_then(|l| l["status"].as_str().map(str::to_string));
        let schedule = g.1.get(&name).map(|e| e.0.clone());
        let cron = schedule.as_deref().and_then(|e| crate::cron::parse(e).ok());
        let next: Vec<i64> = match &cron {
            Some(c) if enabled => crate::cron::next_runs(c, local_now(), 3),
            None if enabled => vec![next_estimate(fin, interval, now)],
            _ => vec![],
        };
        // A cron task is late after twice its gap between runs.
        let gap = match &cron {
            Some(c) => {
                let n = crate::cron::next_runs(c, local_now(), 2);
                n.get(1).zip(n.first()).map_or(interval, |(b, a)| b - a)
            }
            None => interval,
        };
        out.push(json!({
            "name": name, "source": source, "interval_secs": interval, "enabled": enabled, "run_hour": run_hour,
            "schedule": schedule, "default_schedule": defaults.get(&name).cloned().or_else(|| crate::cron::from_interval(interval, run_hour)),
            "description": cron.as_ref().map(crate::cron::describe),
            "health": health(st.as_deref(), fin, gap, now), "last_status": st, "last_finished_at": fin.map(|t| t.unix_timestamp()),
            "last_duration_ms": last.as_ref().map(|l| l["duration_ms"].clone()), "last_error": last.as_ref().map(|l| l["error"].clone()),
            "last_result": last.as_ref().map(|l| l["result"].clone()), "next_run_at": next.first(), "next_runs": next,
            "failures": row.map_or(0, |r| r.failures), "tripped_at": row.and_then(|r| r.tripped_at).map(|t| t.unix_timestamp()),
            "tripped_error": row.and_then(|r| r.tripped_error.clone()), "missed": row.map_or("skip", |r| r.missed.as_str()),
        }));
    }
    Ok(out)
}

fn addon_task(s: &AppState, name: &str) -> Option<(String, String)> {
    addon_tasks(s)
        .into_iter()
        .find(|t| t.0 == name)
        .map(|t| (t.1, t.2))
}

/// `Some(job)` when `name` is an addon cron job (`on-cron`).
fn addon_cron_job(s: &AppState, name: &str) -> Option<(String, String)> {
    addon_tasks(s)
        .into_iter()
        .find(|t| t.0 == name && t.4.is_some())
        .map(|t| (t.1, t.2))
}

/// Queue `name` now (built-in kind or `<addon>:<op>`). `Ok(None)`: already
/// queued this minute.
pub async fn run_task(s: &AppState, name: &str) -> pelty_core::AppResult<Option<i64>> {
    let minute = OffsetDateTime::now_utc().unix_timestamp() / 60;
    enqueue_task(
        s,
        name,
        ("manual", json!(true)),
        &format!("manual:{name}:{minute}"),
    )
    .await
}

/// Queue one run of a task (built-in kind or `<addon>:<op>`) with a marker
/// field in the payload and a dedupe key.
async fn enqueue_task(
    s: &AppState,
    name: &str,
    mark: (&str, Value),
    key: &str,
) -> pelty_core::AppResult<Option<i64>> {
    if let Some((kind, _)) = TASKS.iter().find(|t| t.0 == name) {
        return jobs::enqueue(&s.pool, kind, json!({ mark.0: mark.1 }), None, Some(key), 3).await;
    }
    if let Some((addon, job)) = addon_cron_job(s, name) {
        return jobs::enqueue(
            &s.pool,
            crate::addon_power::JOB_KIND,
            json!({ "addon": addon, "op": job, "cron_job": job, mark.0: mark.1 }),
            None,
            Some(key),
            3,
        )
        .await;
    }
    let (addon, op) = addon_task(s, name).ok_or(AppError::NotFound)?;
    jobs::enqueue(
        &s.pool,
        crate::addon_power::JOB_KIND,
        json!({ "addon": addon, "op": op, "input": {}, mark.0: mark.1 }),
        None,
        Some(key),
        3,
    )
    .await
}

/// Queue every enabled task whose cron expression matched since the last
/// pass (once per matching minute; old runs are not caught up).
pub async fn cron_tick(s: &AppState, g: &Gate) {
    let now = local_now();
    for (name, (expr, saved)) in &g.1 {
        if g.0.get(name).is_some_and(|e| !e.0)
            || (!TASKS.iter().any(|t| t.0 == name) && addon_task(s, name).is_none())
        {
            continue;
        }
        let Ok(c) = crate::cron::parse(expr) else {
            continue;
        };
        if let Some(t) = crate::cron::due(&c, now, *saved) {
            if let Err(e) =
                enqueue_task(s, name, ("cron", json!(t)), &format!("cron:{name}:{t}")).await
            {
                tracing::error!(task = %name, error = %e, "cron enqueue failed");
            }
        }
    }
}

/// Validate and store a cron expression for a task (`None` = back to the
/// built-in interval). Returns the next runs.
pub async fn set_task_schedule(
    s: &AppState,
    name: &str,
    expr: Option<&str>,
) -> pelty_core::AppResult<Vec<i64>> {
    if !TASKS.iter().any(|t| t.0 == name) && addon_task(s, name).is_none() {
        return Err(AppError::NotFound);
    }
    let expr = expr.map(str::trim).filter(|e| !e.is_empty());
    let next = match expr {
        Some(e) => crate::cron::next_runs(
            &crate::cron::parse(e).map_err(|_| AppError::Validation("error.invalid_cron"))?,
            local_now(),
            5,
        ),
        None => vec![],
    };
    pelty_database::automation_settings::set_schedule(&s.pool, name, expr).await?;
    Ok(next)
}

/// Turn automatic scheduling of `name` on or off (the run hour is kept).
pub async fn set_task_enabled(s: &AppState, name: &str, on: bool) -> pelty_core::AppResult<()> {
    if !TASKS.iter().any(|t| t.0 == name) && addon_task(s, name).is_none() {
        return Err(AppError::NotFound);
    }
    let hour = gate(s).await.0.get(name).and_then(|e| e.1);
    pelty_database::automation_settings::upsert(&s.pool, name, on, hour).await
}

fn is_task(s: &AppState, name: &str) -> bool {
    TASKS.iter().any(|t| t.0 == name) || addon_task(s, name).is_some()
}

/// The scheduled task a job belongs to (built-in kind or `<addon>:<op>`).
pub fn task_of(s: &AppState, kind: &str, payload: &Value) -> Option<String> {
    if TASKS.iter().any(|t| t.0 == kind) {
        return Some(kind.to_string());
    }
    if kind != crate::addon_power::JOB_KIND {
        return None;
    }
    let name = format!("{}:{}", payload["addon"].as_str()?, payload["op"].as_str()?);
    addon_task(s, &name).map(|_| name)
}

/// Failed runs in a row (retries of one run count once) before a scheduled task is disabled ("tripped") until reset (default 5).
const ENV_MAX_FAILURES: &str = "PELTY_CRON_MAX_FAILURES";

/// Consecutive failed runs before a task is disabled (default 5).
pub fn max_failures() -> i32 {
    std::env::var(ENV_MAX_FAILURES)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &i32| *n >= 1)
        .unwrap_or(5)
}

/// Feed a finished run (success, or a failure with no retries left) to the
/// task's circuit breaker. On a trip the task is disabled, audited,
/// `automation.task_tripped` is emitted and staff with `jobs.manage` are
/// mailed.
pub async fn record_outcome(s: &AppState, name: &str, err: Option<&str>) {
    let n = match pelty_database::automation_settings::record_outcome(
        &s.pool,
        name,
        err,
        max_failures(),
    )
    .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return,
        Err(e) => return tracing::warn!(task = name, error = %e, "automation breaker"),
    };
    let error: String = err.unwrap_or_default().chars().take(500).collect();
    tracing::warn!(
        task = name,
        failures = n,
        "scheduled task tripped its circuit breaker and was disabled"
    );
    let _ = audit::record(
        &s.pool,
        None,
        "automation.task_tripped",
        Some(name),
        None,
        json!({ "kind": name, "failures": n, "error": error }),
    )
    .await;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "automation.task_tripped",
        json!({ "kind": name, "failures": n, "error": error }),
    ));
    let link = format!("{}/admin/automation", s.cfg.public_url);
    let failures = n.to_string();
    match pelty_database::affiliates::staff_recipients(&s.pool, "jobs.manage").await {
        Ok(to) => {
            for (to, locale) in to {
                let locale = locale
                    .filter(|l| s.i18n.has(l))
                    .unwrap_or_else(|| s.cfg.default_locale.clone());
                let _ = crate::mail::enqueue(
                    s,
                    "automation_task_tripped",
                    &locale,
                    &to,
                    crate::mail::vars(&[
                        ("task", name),
                        ("failures", &failures),
                        ("error", &error),
                        ("link", &link),
                    ]),
                )
                .await;
            }
        }
        Err(e) => tracing::warn!(error = %e, "automation breaker mail"),
    }
}

/// Clear a task's breaker (and, for an addon task, the addon's call breaker).
pub async fn reset_breaker(s: &AppState, name: &str) -> pelty_core::AppResult<()> {
    if !is_task(s, name) {
        return Err(AppError::NotFound);
    }
    if !pelty_database::automation_settings::reset(&s.pool, name).await? {
        pelty_database::automation_settings::upsert(&s.pool, name, true, None).await?;
    }
    if let Some((addon, _)) = addon_task(s, name) {
        s.addon_health.reset(&addon);
    }
    Ok(())
}

/// Set the missed-run policy (`skip` or `run_once`).
pub async fn set_task_missed(s: &AppState, name: &str, missed: &str) -> pelty_core::AppResult<()> {
    if !is_task(s, name) {
        return Err(AppError::NotFound);
    }
    if !pelty_database::automation_settings::MISSED.contains(&missed) {
        return Err(AppError::Validation("error.invalid_missed_policy"));
    }
    pelty_database::automation_settings::set_missed(&s.pool, name, missed).await
}

/// Missed runs (`missed = run_once`), on scheduler start or leader change:
/// a cron task whose latest scheduled time has no run since then is queued
/// once, with the same dedupe key as the regular cron run. Returns the
/// tasks queued.
pub async fn catch_up(s: &AppState) -> Vec<String> {
    let g = gate(s).await;
    let rows = pelty_database::automation_settings::all(&s.pool)
        .await
        .unwrap_or_default();
    let mut queued = vec![];
    for r in rows
        .iter()
        .filter(|r| r.missed == "run_once" && r.enabled && is_task(s, &r.kind))
    {
        // Cron schedule, or the built-in interval as an expression.
        let expr = g.1.get(&r.kind).map(|e| e.0.clone()).or_else(|| {
            TASKS
                .iter()
                .find(|t| t.0 == r.kind)
                .and_then(|t| crate::cron::from_interval(t.1, r.run_hour))
        });
        let Some(Ok(c)) = expr.as_deref().map(crate::cron::parse) else {
            continue;
        };
        let Some(t) = crate::cron::prev_run(&c, local_now()) else {
            continue;
        };
        let since = OffsetDateTime::from_unix_timestamp(t).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let ran: Result<bool, _> = if TASKS.iter().any(|x| x.0 == r.kind) {
            sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM jobs WHERE kind = $1 AND created_at >= $2)",
            )
            .bind(&r.kind)
            .bind(since)
            .fetch_one(&s.pool)
            .await
        } else {
            let (addon, op) = r.kind.split_once(':').unwrap_or_default();
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE kind = $1 AND payload->>'addon' = $2 AND payload->>'op' = $3 AND created_at >= $4)").bind(crate::addon_power::JOB_KIND).bind(addon).bind(op).bind(since).fetch_one(&s.pool).await
        };
        if ran.unwrap_or(true) {
            continue;
        }
        match enqueue_task(
            s,
            &r.kind,
            ("catch_up", json!(t)),
            &format!("cron:{}:{t}", r.kind),
        )
        .await
        {
            Ok(Some(_)) => queued.push(r.kind.clone()),
            Ok(None) => {}
            Err(e) => tracing::warn!(task = %r.kind, error = %e, "catch-up enqueue failed"),
        }
    }
    if !queued.is_empty() {
        tracing::info!(tasks = ?queued, "queued missed scheduled runs");
    }
    queued
}

/// The last `n` runs of `name`, newest first.
pub async fn history(s: &AppState, name: &str, n: i64) -> pelty_core::AppResult<Vec<Value>> {
    type Row = (
        i64,
        String,
        i32,
        OffsetDateTime,
        Option<OffsetDateTime>,
        Option<String>,
        Option<Value>,
    );
    let q =
        "SELECT id, status, attempts, created_at, finished_at, last_error, result FROM jobs WHERE ";
    let rows: Vec<Row> = if TASKS.iter().any(|t| t.0 == name) {
        sqlx::query_as(&format!("{q} kind = $1 ORDER BY id DESC LIMIT $2")).bind(name).bind(n).fetch_all(&s.pool).await
    } else {
        let (addon, op) = name.split_once(':').ok_or(AppError::NotFound)?;
        sqlx::query_as(&format!("{q} kind = $1 AND payload->>'addon' = $2 AND payload->>'op' = $3 ORDER BY id DESC LIMIT $4")).bind(crate::addon_power::JOB_KIND).bind(addon).bind(op).bind(n).fetch_all(&s.pool).await
    }
    .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|(id, status, attempts, created, fin, err, result)| json!({ "id": id, "status": status, "attempts": attempts, "created_at": created.unix_timestamp(), "finished_at": fin.map(|f| f.unix_timestamp()), "duration_ms": fin.map(|f| (f - created).whole_milliseconds() as i64), "error": err, "result": result }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gate_rules() {
        let mut g = Gate::default();
        let at = |h: u8| OffsetDateTime::now_utc().replace_hour(h).unwrap();
        assert!(g.allows("x", at(0)));
        g.0.insert("off".into(), (false, None));
        g.0.insert("late".into(), (true, Some(6)));
        assert!(!g.allows("off", at(12)));
        assert!(!g.allows("late", at(5)));
        assert!(g.allows("late", at(6)));
    }
    #[test]
    fn health_rules() {
        let now = OffsetDateTime::now_utc();
        assert_eq!(health(None, None, 300, now), "never");
        assert_eq!(health(Some("dead"), Some(now), 300, now), "failing");
        assert_eq!(
            health(
                Some("succeeded"),
                Some(now - time::Duration::hours(1)),
                300,
                now
            ),
            "late"
        );
        assert_eq!(
            health(
                Some("succeeded"),
                Some(now - time::Duration::minutes(4)),
                300,
                now
            ),
            "ok"
        );
    }
}
