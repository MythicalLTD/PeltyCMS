//! Admin ops: dashboard widget/quick-link prefs, login history, staff online,
//! calendar, per-client email log (from the mail job queue), DB backups.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

pub const WIDGETS: [&str; 8] = [
    "stats",
    "revenue",
    "activity",
    "staff_online",
    "quick_links",
    "todos",
    "calendar",
    "health",
];

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct QuickLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Prefs {
    pub widgets: Vec<String>,
    pub quick_links: Vec<QuickLink>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            widgets: [
                "stats",
                "revenue",
                "activity",
                "staff_online",
                "quick_links",
            ]
            .map(String::from)
            .to_vec(),
            quick_links: vec![],
        }
    }
}

/// Widgets must be known and unique; links internal (`/…`, not `//`) or https.
pub fn validate_prefs(p: &Prefs) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    let bad_w = p.widgets.len() > WIDGETS.len()
        || p.widgets
            .iter()
            .any(|w| !WIDGETS.contains(&w.as_str()) || !seen.insert(w));
    let bad_l = p.quick_links.len() > 20
        || p.quick_links.iter().any(|l| {
            let u = l.url.trim();
            l.label.trim().is_empty()
                || l.label.len() > 60
                || u.len() > 300
                || !((u.starts_with('/') && !u.starts_with("//")) || u.starts_with("https://"))
        });
    if bad_w || bad_l {
        Err(AppError::Validation("error.validation"))
    } else {
        Ok(())
    }
}

pub async fn prefs(pool: &PgPool, user: Uuid) -> AppResult<Prefs> {
    let r: Option<(Value, Value)> =
        sqlx::query_as("SELECT widgets, quick_links FROM admin_prefs WHERE user_id = $1")
            .bind(user)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(match r {
        Some((w, l)) => Prefs {
            widgets: serde_json::from_value(w).unwrap_or_default(),
            quick_links: serde_json::from_value(l).unwrap_or_default(),
        },
        None => Prefs::default(),
    })
}

pub async fn save_prefs(pool: &PgPool, user: Uuid, p: &Prefs) -> AppResult<()> {
    validate_prefs(p)?;
    sqlx::query("INSERT INTO admin_prefs (user_id, widgets, quick_links) VALUES ($1,$2,$3) ON CONFLICT (user_id) DO UPDATE SET widgets=$2, quick_links=$3, updated_at=now()")
        .bind(user).bind(json!(p.widgets)).bind(json!(p.quick_links)).execute(pool).await.map_err(db_err)?;
    Ok(())
}

const STAFF: &str = "EXISTS (SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE ur.user_id = u.id AND r.name <> 'customer')";

/// Staff sign-ins (each session is one login), newest first.
pub async fn login_history(pool: &PgPool, user: Option<Uuid>, limit: i64) -> AppResult<Value> {
    let q = format!(
        "SELECT coalesce(json_agg(x), '[]') FROM (SELECT s.id, u.id AS user_id, u.email::text AS email, u.display_name, s.ip, s.user_agent, s.created_at, s.last_seen_at,
           CASE WHEN s.revoked_at IS NOT NULL THEN 'revoked' WHEN s.idle_expires_at < now() OR s.absolute_expires_at < now() THEN 'expired' ELSE 'active' END AS state
         FROM sessions s JOIN users u ON u.id = s.user_id WHERE {STAFF} AND ($1::uuid IS NULL OR u.id = $1)
         ORDER BY s.created_at DESC LIMIT $2) x");
    sqlx::query_scalar(&q)
        .bind(user)
        .bind(limit.clamp(1, 500))
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Staff with a live session seen in the last `minutes`.
pub async fn staff_online(pool: &PgPool, minutes: i64) -> AppResult<Value> {
    let q = format!(
        "SELECT coalesce(json_agg(x ORDER BY x.last_seen_at DESC), '[]') FROM (SELECT u.id AS user_id, u.email::text AS email, u.display_name, max(s.last_seen_at) AS last_seen_at
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE {STAFF} AND s.revoked_at IS NULL AND s.idle_expires_at > now() AND s.last_seen_at > now() - make_interval(mins => $1::int)
         GROUP BY u.id) x");
    sqlx::query_scalar(&q)
        .bind(minutes.clamp(1, 1440) as i32)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Parse `YYYY-MM` into the first day of that month.
pub fn month_start(m: &str) -> AppResult<time::Date> {
    let bad = || AppError::Validation("error.validation");
    let (y, mo) = m.split_once('-').ok_or_else(bad)?;
    let y: i32 = y.parse().map_err(|_| bad())?;
    let mo: u8 = mo.parse().map_err(|_| bad())?;
    let month = time::Month::try_from(mo).map_err(|_| bad())?;
    if !(2000..=2200).contains(&y) || m.len() != 7 {
        return Err(bad());
    }
    time::Date::from_calendar_date(y, month, 1).map_err(|_| bad())
}

/// Calendar items for a month: unpaid invoice due dates, domain expiries,
/// service renewals and staff to-dos.
pub async fn calendar(pool: &PgPool, month: &str) -> AppResult<Value> {
    let from = month_start(month)?;
    sqlx::query_scalar(
        "WITH r AS (SELECT $1::date AS a, ($1::date + interval '1 month')::date AS b)
         SELECT coalesce(json_agg(x ORDER BY x.date, x.kind), '[]') FROM (
           SELECT 'invoice' AS kind, i.id::text AS id, (i.due_at AT TIME ZONE 'UTC')::date AS date, coalesce(i.number, 'draft') AS title, i.customer_id::text AS customer_id
             FROM invoices i, r WHERE i.status = 'unpaid' AND i.due_at >= r.a AND i.due_at < r.b
           UNION ALL SELECT 'domain', d.id::text, d.expires_on, d.name, d.customer_id::text FROM domains d, r WHERE d.status = 'active' AND d.expires_on >= r.a AND d.expires_on < r.b
           UNION ALL SELECT 'service', s.id::text, s.next_due_date, p.name, s.customer_id::text FROM services s JOIN products p ON p.id = s.product_id, r
             WHERE s.status IN ('active','suspended') AND s.next_due_date >= r.a AND s.next_due_date < r.b
           UNION ALL SELECT 'todo', t.id::text, t.due_date, t.title, NULL FROM admin_todos t, r WHERE t.status <> 'done' AND t.due_date >= r.a AND t.due_date < r.b
         ) x",
    ).bind(from).fetch_one(pool).await.map_err(db_err)
}

/// Emails sent (or queued) to a client's address, from the mail job queue.
pub async fn customer_emails(pool: &PgPool, customer: Uuid) -> AppResult<Value> {
    let email: Option<String> =
        sqlx::query_scalar("SELECT email::text FROM customers WHERE id = $1")
            .bind(customer)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let email = email.ok_or(AppError::NotFound)?;
    sqlx::query_scalar(
        "SELECT coalesce(json_agg(x ORDER BY x.created_at DESC), '[]') FROM (SELECT id, payload->>'template' AS template, payload->>'locale' AS locale,
           status, attempts, last_error, created_at, finished_at FROM jobs WHERE kind = 'email.send' AND lower(payload->>'to') = lower($1) ORDER BY created_at DESC LIMIT 200) x",
    ).bind(email).fetch_one(pool).await.map_err(db_err)
}

/// Re-queue a past email job with the same template, locale, recipient and vars.
/// Re-queue a sent mail job. Templates in `deny` (live tokens) are refused.
pub async fn resend_email(pool: &PgPool, job: i64, deny: &[&str]) -> AppResult<(String, i64)> {
    let p: Option<Value> =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1 AND kind = 'email.send'")
            .bind(job)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let p = p.ok_or(AppError::NotFound)?;
    if p["template"].as_str().is_some_and(|t| deny.contains(&t)) {
        return Err(AppError::Conflict("error.email_not_resendable"));
    }
    let to = p["to"].as_str().unwrap_or_default().to_string();
    let id = crate::jobs::enqueue(pool, "email.send", p, None, None, 6)
        .await?
        .ok_or(AppError::Conflict("error.conflict"))?;
    Ok((to, id))
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Backup {
    pub id: Uuid,
    pub file_name: String,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: time::OffsetDateTime,
}

pub async fn backups(pool: &PgPool) -> AppResult<Vec<Backup>> {
    sqlx::query_as("SELECT id, file_name, size_bytes, status, error, created_at FROM db_backups ORDER BY created_at DESC LIMIT 200")
        .fetch_all(pool).await.map_err(db_err)
}

pub async fn backup(pool: &PgPool, id: Uuid) -> AppResult<Backup> {
    sqlx::query_as(
        "SELECT id, file_name, size_bytes, status, error, created_at FROM db_backups WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// `by` is None for backups the server takes itself (e.g. before an update).
pub async fn create_backup(pool: &PgPool, by: Option<Uuid>) -> AppResult<Backup> {
    let id = Uuid::new_v4();
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    let name = format!("pelty-{ts}-{}.dump", &id.simple().to_string()[..8]);
    sqlx::query("INSERT INTO db_backups (id, file_name, created_by) VALUES ($1,$2,$3)")
        .bind(id)
        .bind(&name)
        .bind(by)
        .execute(pool)
        .await
        .map_err(db_err)?;
    backup(pool, id).await
}

pub async fn finish_backup(
    pool: &PgPool,
    id: Uuid,
    size: Option<i64>,
    err: Option<&str>,
) -> AppResult<()> {
    sqlx::query("UPDATE db_backups SET status = CASE WHEN $3::text IS NULL THEN 'done' ELSE 'failed' END, size_bytes=$2, error=$3, finished_at=now() WHERE id=$1")
        .bind(id).bind(size).bind(err).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn delete_backup(pool: &PgPool, id: Uuid) -> AppResult<String> {
    let n: Option<String> =
        sqlx::query_scalar("DELETE FROM db_backups WHERE id = $1 RETURNING file_name")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    n.ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_validation() {
        assert!(validate_prefs(&Prefs::default()).is_ok());
        let link = |u: &str| Prefs {
            widgets: vec![],
            quick_links: vec![QuickLink {
                label: "x".into(),
                url: u.into(),
            }],
        };
        assert!(validate_prefs(&link("/admin/billing")).is_ok());
        assert!(validate_prefs(&link("https://docs.example")).is_ok());
        assert!(validate_prefs(&link("//evil.example")).is_err());
        assert!(validate_prefs(&link("javascript:alert(1)")).is_err());
        assert!(validate_prefs(&Prefs {
            widgets: vec!["stats".into(), "stats".into()],
            quick_links: vec![]
        })
        .is_err());
        assert!(validate_prefs(&Prefs {
            widgets: vec!["nope".into()],
            quick_links: vec![]
        })
        .is_err());
    }

    #[test]
    fn months() {
        assert_eq!(month_start("2026-02").unwrap().to_string(), "2026-02-01");
        for b in ["2026-13", "26-01", "2026-1", "x"] {
            assert!(month_start(b).is_err(), "{b}");
        }
    }
}
