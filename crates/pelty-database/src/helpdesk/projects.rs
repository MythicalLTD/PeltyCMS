//! Project management: projects for a client with tasks, time tracking and
//! links to tickets and invoices; billable time is invoiced as lines.
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

pub const STATUSES: [&str; 5] = ["planned", "active", "on_hold", "completed", "cancelled"];

fn v() -> AppError {
    AppError::Validation("error.validation")
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct ProjectInput {
    pub customer_id: Option<Uuid>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "active")]
    pub status: String,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub due_date: Option<Date>,
    #[serde(default)]
    pub client_visible: bool,
    #[serde(default)]
    #[schema(value_type = String)]
    pub hourly_rate: Decimal,
}
fn active() -> String {
    "active".into()
}

pub fn check(p: &ProjectInput) -> AppResult<()> {
    let n = p.title.trim().chars().count();
    if n == 0
        || n > 200
        || p.description.chars().count() > 10_000
        || !STATUSES.contains(&p.status.as_str())
    {
        return Err(v());
    }
    if p.hourly_rate < Decimal::ZERO
        || p.hourly_rate > Decimal::from(1_000_000)
        || p.hourly_rate.scale() > 2
    {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    Ok(())
}

/// Price of an entry: rate × minutes / 60, rounded half-up to cents.
pub fn entry_amount(rate: Decimal, minutes: i32) -> Decimal {
    (rate * Decimal::from(minutes) / Decimal::from(60))
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
}

const SUMMARY: &str = "SELECT p.id, p.customer_id, c.email::text AS customer_email, p.title, p.status, p.due_date, p.client_visible,
    p.hourly_rate::text AS hourly_rate, p.created_at, p.updated_at,
    (SELECT count(*) FROM project_tasks t WHERE t.project_id = p.id) AS tasks_total,
    (SELECT count(*) FROM project_tasks t WHERE t.project_id = p.id AND t.done) AS tasks_done,
    (SELECT COALESCE(sum(minutes),0) FROM project_time_entries e WHERE e.project_id = p.id) AS minutes_total
  FROM projects p LEFT JOIN customers c ON c.id = p.customer_id";

/// Staff list (all, or one client's); `client` restricts to visible ones.
pub async fn list(pool: &PgPool, customer: Option<Uuid>, client: bool) -> AppResult<Value> {
    sqlx::query_scalar(&format!(
        "SELECT COALESCE(json_agg(row_to_json(x) ORDER BY x.created_at DESC), '[]'::json)::jsonb FROM ({SUMMARY}
         WHERE ($1::uuid IS NULL OR p.customer_id = $1) AND (NOT $2 OR p.client_visible) ORDER BY p.created_at DESC LIMIT 500) x"
    ))
    .bind(customer)
    .bind(client)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Full project. With `customer` set, only that client's visible project.
pub async fn detail(pool: &PgPool, id: Uuid, customer: Option<Uuid>) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar(&format!(
        "SELECT jsonb_build_object(
           'project', (SELECT row_to_json(x)::jsonb FROM ({SUMMARY} WHERE p.id = $1) x) || jsonb_build_object('description', p.description),
           'tasks', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', t.id, 'title', t.title, 'done', t.done, 'due_date', t.due_date,
                     'assignee_id', t.assignee_id, 'assignee', u.display_name) ORDER BY t.created_at)
                     FROM project_tasks t LEFT JOIN users u ON u.id = t.assignee_id WHERE t.project_id = p.id), '[]'::jsonb),
           'time', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', e.id, 'task_id', e.task_id, 'minutes', e.minutes, 'note', e.note,
                     'billable', e.billable, 'spent_on', e.spent_on, 'invoice_id', e.invoice_id, 'user', u.display_name) ORDER BY e.spent_on DESC, e.created_at DESC)
                     FROM project_time_entries e LEFT JOIN users u ON u.id = e.user_id WHERE e.project_id = p.id), '[]'::jsonb),
           'links', COALESCE((SELECT jsonb_agg(jsonb_build_object('kind', l.kind, 'target_id', l.target_id,
                     'label', CASE l.kind WHEN 'ticket' THEN (SELECT '#' || t.number || ' ' || t.subject FROM tickets t WHERE t.id = l.target_id)
                                          ELSE (SELECT COALESCE(i.number, 'PF-' || upper(left(i.id::text, 8))) FROM invoices i WHERE i.id = l.target_id) END)
                     ORDER BY l.kind, l.target_id) FROM project_links l WHERE l.project_id = p.id), '[]'::jsonb))
         FROM projects p WHERE p.id = $1 AND ($2::uuid IS NULL OR (p.customer_id = $2 AND p.client_visible))"
    ))
    .bind(id)
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    v.ok_or(AppError::NotFound)
}

async fn customer_exists(pool: &PgPool, c: Option<Uuid>) -> AppResult<()> {
    if let Some(c) = c {
        let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM customers WHERE id = $1)")
            .bind(c)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
        if !ok {
            return Err(AppError::NotFound);
        }
    }
    Ok(())
}

pub async fn create(pool: &PgPool, by: Uuid, p: &ProjectInput) -> AppResult<Uuid> {
    check(p)?;
    customer_exists(pool, p.customer_id).await?;
    sqlx::query_scalar(
        "INSERT INTO projects (customer_id, title, description, status, due_date, client_visible, hourly_rate, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
    )
    .bind(p.customer_id)
    .bind(p.title.trim())
    .bind(&p.description)
    .bind(&p.status)
    .bind(p.due_date)
    .bind(p.client_visible)
    .bind(p.hourly_rate)
    .bind(by)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, p: &ProjectInput) -> AppResult<()> {
    check(p)?;
    customer_exists(pool, p.customer_id).await?;
    let r = sqlx::query(
        "UPDATE projects SET customer_id = $2, title = $3, description = $4, status = $5, due_date = $6, client_visible = $7,
            hourly_rate = $8, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(p.customer_id)
    .bind(p.title.trim())
    .bind(&p.description)
    .bind(&p.status)
    .bind(p.due_date)
    .bind(p.client_visible)
    .bind(p.hourly_rate)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub assignee_id: Option<Uuid>,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub due_date: Option<Date>,
}

pub async fn add_task(pool: &PgPool, project: Uuid, t: &TaskInput) -> AppResult<Uuid> {
    let n = t.title.trim().chars().count();
    if n == 0 || n > 300 {
        return Err(v());
    }
    sqlx::query_scalar(
        "INSERT INTO project_tasks (project_id, title, assignee_id, due_date)
         SELECT id, $2, $3, $4 FROM projects WHERE id = $1 RETURNING id",
    )
    .bind(project)
    .bind(t.title.trim())
    .bind(t.assignee_id)
    .bind(t.due_date)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn set_task_done(pool: &PgPool, project: Uuid, task: Uuid, done: bool) -> AppResult<()> {
    let r = sqlx::query("UPDATE project_tasks SET done = $3 WHERE id = $2 AND project_id = $1")
        .bind(project)
        .bind(task)
        .bind(done)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_task(pool: &PgPool, project: Uuid, task: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM project_tasks WHERE id = $2 AND project_id = $1")
        .bind(project)
        .bind(task)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct TimeInput {
    pub minutes: i32,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub note: String,
    #[serde(default = "yes")]
    pub billable: bool,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub spent_on: Option<Date>,
}
fn yes() -> bool {
    true
}

pub async fn add_time(pool: &PgPool, project: Uuid, user: Uuid, t: &TimeInput) -> AppResult<Uuid> {
    if !(1..=1440).contains(&t.minutes) || t.note.chars().count() > 500 {
        return Err(v());
    }
    if let Some(task) = t.task_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM project_tasks WHERE id = $1 AND project_id = $2)",
        )
        .bind(task)
        .bind(project)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
        if !ok {
            return Err(v());
        }
    }
    sqlx::query_scalar(
        "INSERT INTO project_time_entries (project_id, task_id, user_id, minutes, note, billable, spent_on)
         SELECT id, $2, $3, $4, $5, $6, COALESCE($7, CURRENT_DATE) FROM projects WHERE id = $1 RETURNING id",
    )
    .bind(project)
    .bind(t.task_id)
    .bind(user)
    .bind(t.minutes)
    .bind(t.note.trim())
    .bind(t.billable)
    .bind(t.spent_on)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Invoiced entries are kept (they back an invoice line).
pub async fn delete_time(pool: &PgPool, project: Uuid, entry: Uuid) -> AppResult<()> {
    let r = sqlx::query(
        "DELETE FROM project_time_entries WHERE id = $2 AND project_id = $1 AND invoice_id IS NULL",
    )
    .bind(project)
    .bind(entry)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.time_entry_locked"));
    }
    Ok(())
}

/// Link a ticket or invoice of the project's client.
pub async fn link(pool: &PgPool, project: Uuid, kind: &str, target: Uuid) -> AppResult<()> {
    let table = match kind {
        "ticket" => "tickets",
        "invoice" => "invoices",
        _ => return Err(v()),
    };
    let ok: Option<bool> = sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM {table} x WHERE x.id = $2 AND x.customer_id IS NOT DISTINCT FROM p.customer_id) FROM projects p WHERE p.id = $1"
    ))
    .bind(project)
    .bind(target)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match ok {
        None => Err(AppError::NotFound),
        Some(false) => Err(AppError::Validation("error.project_link_invalid")),
        Some(true) => {
            sqlx::query("INSERT INTO project_links (project_id, kind, target_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(project).bind(kind).bind(target).execute(pool).await.map_err(db_err)?;
            Ok(())
        }
    }
}

pub async fn unlink(pool: &PgPool, project: Uuid, kind: &str, target: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM project_links WHERE project_id = $1 AND kind = $2 AND target_id = $3")
        .bind(project)
        .bind(kind)
        .bind(target)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Invoice all unbilled billable time (one line per entry) and link it.
pub async fn invoice_time(pool: &PgPool, project: Uuid, due_days: i64) -> AppResult<Uuid> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(Option<Uuid>, Decimal, String)> = sqlx::query_as(
        "SELECT customer_id, hourly_rate, title FROM projects WHERE id = $1 FOR UPDATE",
    )
    .bind(project)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (cust, rate, title) = row.ok_or(AppError::NotFound)?;
    let cust = cust.ok_or(AppError::Conflict("error.project_no_client"))?;
    let entries: Vec<(Uuid, i32, String, Date)> = sqlx::query_as(
        "SELECT id, minutes, note, spent_on FROM project_time_entries WHERE project_id = $1 AND billable AND invoice_id IS NULL ORDER BY spent_on, created_at FOR UPDATE",
    )
    .bind(project).fetch_all(&mut *tx).await.map_err(db_err)?;
    if entries.is_empty() || rate.is_zero() {
        return Err(AppError::Conflict("error.project_nothing_to_bill"));
    }
    let cur: String = sqlx::query_scalar("SELECT currency::text FROM customers WHERE id = $1")
        .bind(cust)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let lines: Vec<NewItem> = entries
        .iter()
        .map(|(_, m, note, d)| NewItem {
            description: format!(
                "{title}: {d} {}h{:02}m{}",
                m / 60,
                m % 60,
                if note.is_empty() {
                    String::new()
                } else {
                    format!(", {note}")
                }
            )
            .chars()
            .take(500)
            .collect(),
            quantity: Decimal::ONE,
            unit_price: entry_amount(rate, *m),
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        })
        .collect();
    let inv = invoices::create_draft(&mut tx, cust, &cur, None).await?;
    invoices::add_items(&mut tx, inv, &cur, &lines).await?;
    invoices::finalize(&mut tx, inv, due_days).await?;
    let ids: Vec<Uuid> = entries.iter().map(|e| e.0).collect();
    sqlx::query("UPDATE project_time_entries SET invoice_id = $2 WHERE id = ANY($1)")
        .bind(&ids)
        .bind(inv)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO project_links (project_id, kind, target_id) VALUES ($1,'invoice',$2) ON CONFLICT DO NOTHING").bind(project).bind(inv).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn amounts_are_exact() {
        let r: Decimal = "90.00".parse().unwrap();
        assert_eq!(entry_amount(r, 60), "90.00".parse::<Decimal>().unwrap());
        assert_eq!(entry_amount(r, 10), "15.00".parse::<Decimal>().unwrap());
        assert_eq!(
            entry_amount("50.00".parse().unwrap(), 7),
            "5.83".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            entry_amount("0.03".parse().unwrap(), 30),
            "0.02".parse::<Decimal>().unwrap()
        );
    }
    #[test]
    fn validation() {
        let mut p = ProjectInput {
            customer_id: None,
            title: "X".into(),
            description: String::new(),
            status: "active".into(),
            due_date: None,
            client_visible: false,
            hourly_rate: Decimal::ZERO,
        };
        assert!(check(&p).is_ok());
        p.status = "bogus".into();
        assert!(check(&p).is_err());
        p.status = "active".into();
        p.hourly_rate = "1.234".parse().unwrap();
        assert!(check(&p).is_err());
    }
}
