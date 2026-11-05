//! Ticket SLA policies (per department and priority), live SLA state, and
//! the warning / breach scan with escalation.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const PRIORITIES: [&str; 4] = ["low", "medium", "high", "urgent"];

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Policy {
    pub id: Uuid,
    pub department_id: Option<Uuid>,
    pub priority: Option<String>,
    pub first_response_minutes: Option<i32>,
    pub resolution_minutes: Option<i32>,
    pub warn_percent: i16,
    pub escalate_priority: Option<String>,
    pub escalate_to: Option<Uuid>,
    pub escalate_department: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyInput {
    pub department_id: Option<Uuid>,
    pub priority: Option<String>,
    pub first_response_minutes: Option<i32>,
    pub resolution_minutes: Option<i32>,
    #[serde(default = "default_warn")]
    pub warn_percent: i16,
    #[serde(default)]
    pub escalate_priority: Option<String>,
    #[serde(default)]
    pub escalate_to: Option<Uuid>,
    #[serde(default)]
    pub escalate_department: Option<Uuid>,
}

fn default_warn() -> i16 {
    80
}

const MAX_MINUTES: i32 = 525_600;

impl PolicyInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        let pri_ok = |p: &Option<String>| p.as_deref().is_none_or(|p| PRIORITIES.contains(&p));
        if !pri_ok(&self.priority) || !pri_ok(&self.escalate_priority) {
            return Err("error.validation");
        }
        if self.first_response_minutes.is_none() && self.resolution_minutes.is_none() {
            return Err("error.invalid_sla");
        }
        let bad = |m: Option<i32>| m.is_some_and(|m| !(1..=MAX_MINUTES).contains(&m));
        if bad(self.first_response_minutes) || bad(self.resolution_minutes) || !(1..=99).contains(&self.warn_percent) {
            return Err("error.invalid_sla");
        }
        if let (Some(f), Some(r)) = (self.first_response_minutes, self.resolution_minutes) {
            if r < f {
                return Err("error.invalid_sla");
            }
        }
        Ok(())
    }
}

const COLS: &str = "id, department_id, priority, first_response_minutes, resolution_minutes, warn_percent, escalate_priority, escalate_to, escalate_department, updated_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Policy>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM ticket_sla_policies ORDER BY department_id NULLS FIRST, array_position(ARRAY['low','medium','high','urgent'], priority) NULLS FIRST"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

fn conflict(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.code().as_deref() == Some("23505") => AppError::Conflict("error.sla_policy_exists"),
        sqlx::Error::Database(d) if d.code().as_deref() == Some("23503") => AppError::Validation("error.validation"),
        _ => db_err(e),
    }
}

pub async fn create(pool: &PgPool, p: &PolicyInput) -> AppResult<Policy> {
    sqlx::query_as(&format!(
        "INSERT INTO ticket_sla_policies (department_id, priority, first_response_minutes, resolution_minutes, warn_percent, escalate_priority, escalate_to, escalate_department)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {COLS}"
    ))
    .bind(p.department_id).bind(&p.priority).bind(p.first_response_minutes).bind(p.resolution_minutes).bind(p.warn_percent)
    .bind(&p.escalate_priority).bind(p.escalate_to).bind(p.escalate_department)
    .fetch_one(pool).await.map_err(conflict)
}

pub async fn update(pool: &PgPool, id: Uuid, p: &PolicyInput) -> AppResult<Policy> {
    sqlx::query_as(&format!(
        "UPDATE ticket_sla_policies SET department_id = $2, priority = $3, first_response_minutes = $4, resolution_minutes = $5, warn_percent = $6,
           escalate_priority = $7, escalate_to = $8, escalate_department = $9, updated_at = now() WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id).bind(p.department_id).bind(&p.priority).bind(p.first_response_minutes).bind(p.resolution_minutes).bind(p.warn_percent)
    .bind(&p.escalate_priority).bind(p.escalate_to).bind(p.escalate_department)
    .fetch_optional(pool).await.map_err(conflict)?.ok_or(AppError::NotFound)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM ticket_sla_policies WHERE id = $1").bind(id).execute(pool).await.map_err(db_err)?.rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Keeps the legacy per-department first-response SLA (0120) in sync with
/// that department's catch-all policy.
pub async fn sync_department_first_response(pool: &PgPool, department: Uuid, minutes: Option<i32>) -> AppResult<()> {
    let key = "COALESCE(department_id, '00000000-0000-0000-0000-000000000000'::uuid) = $1 AND priority IS NULL";
    match minutes {
        Some(m) => {
            let n = sqlx::query(&format!("UPDATE ticket_sla_policies SET first_response_minutes = $2, updated_at = now() WHERE {key}"))
                .bind(department).bind(m).execute(pool).await.map_err(db_err)?.rows_affected();
            if n == 0 {
                sqlx::query("INSERT INTO ticket_sla_policies (department_id, first_response_minutes) VALUES ($1, $2)")
                    .bind(department).bind(m).execute(pool).await.map_err(db_err)?;
            }
        }
        None => {
            sqlx::query(&format!("DELETE FROM ticket_sla_policies WHERE {key} AND resolution_minutes IS NULL")).bind(department).execute(pool).await.map_err(db_err)?;
            sqlx::query(&format!("UPDATE ticket_sla_policies SET first_response_minutes = NULL, updated_at = now() WHERE {key}")).bind(department).execute(pool).await.map_err(db_err)?;
        }
    }
    Ok(())
}

/// One SLA target's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct TargetState {
    #[serde(with = "time::serde::rfc3339")]
    pub due_at: OffsetDateTime,
    /// `ok` | `warning` | `breached` (open), `met` | `missed` (done).
    pub state: &'static str,
}

/// State of a target of `minutes` that started at `start`; `done` is when
/// it was met (first staff reply / close).
pub fn target_state(start: OffsetDateTime, minutes: Option<i32>, done: Option<OffsetDateTime>, warn_percent: i16, now: OffsetDateTime) -> Option<TargetState> {
    let m = i64::from(minutes?);
    let due_at = start + Duration::minutes(m);
    let state = match done {
        Some(d) if d <= due_at => "met",
        Some(_) => "missed",
        None if now >= due_at => "breached",
        None if now >= start + Duration::seconds(m * 60 * i64::from(warn_percent) / 100) => "warning",
        None => "ok",
    };
    Some(TargetState { due_at, state })
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct TicketSla {
    pub policy_id: Option<Uuid>,
    pub first_response: Option<TargetState>,
    pub resolution: Option<TargetState>,
    /// The ticket is on hold: no warnings or breaches are raised.
    pub paused: bool,
}

const MATCH: &str = "LATERAL (SELECT * FROM ticket_sla_policies p WHERE (p.department_id IS NULL OR p.department_id = t.department_id)
      AND (p.priority IS NULL OR p.priority = t.priority)
    ORDER BY (p.department_id IS NOT NULL) DESC, (p.priority IS NOT NULL) DESC LIMIT 1) p";

type Row = (Option<Uuid>, Option<i32>, Option<i32>, Option<i16>, OffsetDateTime, Option<OffsetDateTime>, Option<OffsetDateTime>, String);

pub async fn for_ticket(pool: &PgPool, ticket: Uuid) -> AppResult<TicketSla> {
    let r: Row = sqlx::query_as(&format!(
        "SELECT p.id, p.first_response_minutes, p.resolution_minutes, p.warn_percent, t.created_at, t.first_response_at,
                CASE WHEN t.status = 'closed' THEN COALESCE(t.closed_at, now()) END, t.status
         FROM tickets t LEFT JOIN {MATCH} ON true WHERE t.id = $1"
    ))
    .bind(ticket).fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound)?;
    let (pid, fr, res, warn, created, first, closed, status) = r;
    let now = OffsetDateTime::now_utc();
    let w = warn.unwrap_or(80);
    Ok(TicketSla {
        policy_id: pid,
        first_response: target_state(created, fr, first, w, now),
        resolution: target_state(created, res, closed, w, now),
        paused: status == "on_hold",
    })
}

const OPEN: &str = "WITH c AS (
  SELECT t.id AS ticket_id, t.number, t.subject, t.priority, t.status, t.assigned_to, t.department_id, p.id AS policy_id,
    CASE WHEN p.first_response_minutes IS NOT NULL AND t.first_response_at IS NULL THEN t.created_at + make_interval(mins => p.first_response_minutes) END AS fr_due,
    CASE WHEN p.first_response_minutes IS NOT NULL AND t.first_response_at IS NULL THEN t.created_at + make_interval(secs => (p.first_response_minutes * 60 * p.warn_percent / 100.0)::float8) END AS fr_warn,
    CASE WHEN p.resolution_minutes IS NOT NULL THEN t.created_at + make_interval(mins => p.resolution_minutes) END AS res_due,
    CASE WHEN p.resolution_minutes IS NOT NULL THEN t.created_at + make_interval(secs => (p.resolution_minutes * 60 * p.warn_percent / 100.0)::float8) END AS res_warn
  FROM tickets t JOIN ";

const OPEN_WHERE: &str = " ON true WHERE t.status NOT IN ('closed', 'on_hold') AND t.merged_into IS NULL)";

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct AtRisk {
    pub ticket_id: Uuid,
    pub number: i64,
    pub subject: String,
    pub priority: String,
    pub status: String,
    pub assigned_to: Option<Uuid>,
    pub department_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub fr_due: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub res_due: Option<OffsetDateTime>,
    /// `breached` when any target is past due, else `warning`.
    pub state: String,
}

/// Open tickets past an SLA warning threshold, most urgent first.
pub async fn at_risk(pool: &PgPool, department: Option<Uuid>, limit: i64) -> AppResult<Vec<AtRisk>> {
    sqlx::query_as(&format!(
        "{OPEN}{MATCH}{OPEN_WHERE}
         SELECT ticket_id, number, subject, priority, status, assigned_to, department_id, fr_due, res_due,
                CASE WHEN LEAST(fr_due, res_due) <= now() THEN 'breached' ELSE 'warning' END AS state
         FROM c WHERE LEAST(fr_warn, res_warn) <= now() AND ($1::uuid IS NULL OR department_id = $1)
         ORDER BY LEAST(fr_due, res_due), number LIMIT $2"
    ))
    .bind(department).bind(limit).fetch_all(pool).await.map_err(db_err)
}

/// A warning or breach that fired in a scan.
#[derive(Debug, Clone, Serialize)]
pub struct Fired {
    pub ticket_id: Uuid,
    pub number: i64,
    pub subject: String,
    pub target: &'static str,
    pub stage: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    pub due_at: OffsetDateTime,
    pub assigned_to: Option<Uuid>,
    pub escalated: bool,
}

type ScanRow = (Uuid, i64, String, Option<Uuid>, Uuid, Option<OffsetDateTime>, Option<OffsetDateTime>, Option<OffsetDateTime>, Option<OffsetDateTime>);

async fn claim(pool: &PgPool, ticket: Uuid, target: &str, stage: &str, policy: Uuid, due: OffsetDateTime) -> AppResult<bool> {
    Ok(sqlx::query("INSERT INTO ticket_sla_events (ticket_id, target, stage, policy_id, due_at) VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(ticket).bind(target).bind(stage).bind(policy).bind(due)
        .execute(pool).await.map_err(db_err)?.rows_affected() == 1)
}

/// Escalate per the policy: raise (never lower) the priority, reassign,
/// move department, tag `sla-breached`. Returns whether anything changed.
async fn escalate(pool: &PgPool, ticket: Uuid, policy: Uuid) -> AppResult<bool> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let n = sqlx::query(
        "UPDATE tickets t SET
            priority = CASE WHEN p.escalate_priority IS NOT NULL AND array_position(ARRAY['low','medium','high','urgent'], p.escalate_priority) > array_position(ARRAY['low','medium','high','urgent'], t.priority)
                            THEN p.escalate_priority ELSE t.priority END,
            assigned_to = COALESCE(p.escalate_to, t.assigned_to),
            department_id = COALESCE(p.escalate_department, t.department_id),
            escalated_at = now()
         FROM ticket_sla_policies p WHERE p.id = $2 AND t.id = $1
           AND (p.escalate_priority IS NOT NULL OR p.escalate_to IS NOT NULL OR p.escalate_department IS NOT NULL)",
    )
    .bind(ticket).bind(policy).execute(&mut *tx).await.map_err(db_err)?.rows_affected();
    sqlx::query("INSERT INTO ticket_tags (ticket_id, tag) VALUES ($1, 'sla-breached') ON CONFLICT DO NOTHING").bind(ticket).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(n > 0)
}

/// Raise warnings and breaches that are due (each fires once per ticket and
/// target) and escalate breached tickets. Safe to run concurrently: the
/// event insert is the claim.
pub async fn scan(pool: &PgPool) -> AppResult<Vec<Fired>> {
    let rows: Vec<ScanRow> = sqlx::query_as(&format!(
        "{OPEN}{MATCH}{OPEN_WHERE}
         SELECT ticket_id, number, subject, assigned_to, policy_id, fr_due, fr_warn, res_due, res_warn FROM c
         WHERE LEAST(fr_warn, res_warn) <= now() ORDER BY LEAST(fr_due, res_due) LIMIT 1000"
    ))
    .fetch_all(pool).await.map_err(db_err)?;
    let now = OffsetDateTime::now_utc();
    let mut out = Vec::new();
    for (ticket, number, subject, assigned_to, policy, fr_due, fr_warn, res_due, res_warn) in rows {
        for (target, due, warn) in [("first_response", fr_due, fr_warn), ("resolution", res_due, res_warn)] {
            let (Some(due), Some(warn)) = (due, warn) else { continue };
            let fired = if now >= due {
                // Late scans skip straight to the breach; the warning is moot.
                let _ = claim(pool, ticket, target, "warning", policy, due).await?;
                claim(pool, ticket, target, "breach", policy, due).await?.then_some("breach")
            } else if now >= warn {
                claim(pool, ticket, target, "warning", policy, due).await?.then_some("warning")
            } else {
                None
            };
            if let Some(stage) = fired {
                let escalated = stage == "breach" && escalate(pool, ticket, policy).await?;
                out.push(Fired { ticket_id: ticket, number, subject: subject.clone(), target, stage, due_at: due, assigned_to, escalated });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_states() {
        let t0 = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let at = |m: i64| t0 + Duration::minutes(m);
        assert_eq!(target_state(t0, None, None, 80, at(5)), None);
        let st = |done: Option<OffsetDateTime>, now: i64| target_state(t0, Some(60), done, 80, at(now)).unwrap().state;
        assert_eq!(st(None, 10), "ok");
        assert_eq!(st(None, 48), "warning");
        assert_eq!(st(None, 60), "breached");
        assert_eq!(st(Some(at(59)), 500), "met");
        assert_eq!(st(Some(at(61)), 500), "missed");
        assert_eq!(target_state(t0, Some(60), None, 80, at(0)).unwrap().due_at, at(60));
    }

    #[test]
    fn policy_validation() {
        let p = |fr: Option<i32>, res: Option<i32>| PolicyInput { department_id: None, priority: None, first_response_minutes: fr, resolution_minutes: res, warn_percent: 80, escalate_priority: None, escalate_to: None, escalate_department: None };
        assert!(p(Some(60), Some(480)).validate().is_ok());
        assert!(p(None, None).validate().is_err());
        assert!(p(Some(60), Some(30)).validate().is_err());
        assert!(p(Some(0), None).validate().is_err());
        assert!(PolicyInput { priority: Some("x".into()), ..p(Some(1), None) }.validate().is_err());
        assert!(PolicyInput { warn_percent: 100, ..p(Some(1), None) }.validate().is_err());
    }
}
