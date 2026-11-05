//! Support tickets. Client queries always filter by `customer_id` in SQL;
//! internal notes are only returned when `include_internal` is set (staff).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const STATUSES: &[&str] = &["open", "answered", "customer_reply", "on_hold", "closed"];
pub const PRIORITIES: &[&str] = &["low", "medium", "high", "urgent"];

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Department {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub sort_order: i32,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Ticket {
    pub id: Uuid,
    pub number: i64,
    pub customer_id: Option<Uuid>,
    pub customer_name: String,
    pub customer_email: String,
    pub department_id: Uuid,
    pub department_name: String,
    pub service_id: Option<Uuid>,
    pub assigned_to: Option<Uuid>,
    pub subject: String,
    pub status: String,
    pub priority: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_reply_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub closed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub first_response_at: Option<OffsetDateTime>,
    pub rating: Option<i16>,
    pub rating_comment: String,
    pub merged_into: Option<Uuid>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Message {
    pub id: Uuid,
    pub author_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub author_avatar_version: Option<i64>,
    pub is_staff: bool,
    pub is_internal: bool,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[sqlx(default)]
    #[serde(with = "time::serde::rfc3339::option")]
    pub edited_at: Option<OffsetDateTime>,
}

const COLS: &str = "t.id, t.number, t.customer_id, \
    COALESCE(NULLIF(trim(c.first_name || ' ' || c.last_name), ''), c.organization, c.email::text, t.guest_email) AS customer_name, COALESCE(c.email::text, t.guest_email) AS customer_email, \
    t.department_id, d.name AS department_name, t.service_id, t.assigned_to, t.subject, t.status, t.priority, t.created_at, t.last_reply_at, t.closed_at, t.first_response_at, t.rating, t.rating_comment, t.merged_into";
const FROM: &str = "FROM tickets t LEFT JOIN customers c ON c.id = t.customer_id JOIN ticket_departments d ON d.id = t.department_id";

pub async fn departments(pool: &PgPool, only_active: bool) -> AppResult<Vec<Department>> {
    sqlx::query_as("SELECT id, name, description, is_active, sort_order FROM ticket_departments WHERE (is_active OR NOT $1) ORDER BY sort_order, name")
        .bind(only_active).fetch_all(pool).await.map_err(db_err)
}

pub async fn upsert_department(
    pool: &PgPool,
    id: Option<Uuid>,
    name: &str,
    description: &str,
    is_active: bool,
    sort_order: i32,
) -> AppResult<Department> {
    match id {
        Some(id) => sqlx::query_as("UPDATE ticket_departments SET name=$2, description=$3, is_active=$4, sort_order=$5 WHERE id=$1 RETURNING id, name, description, is_active, sort_order")
            .bind(id).bind(name).bind(description).bind(is_active).bind(sort_order).fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound),
        None => sqlx::query_as("INSERT INTO ticket_departments (name, description, is_active, sort_order) VALUES ($1,$2,$3,$4) RETURNING id, name, description, is_active, sort_order")
            .bind(name).bind(description).bind(is_active).bind(sort_order).fetch_one(pool).await.map_err(db_err),
    }
}

pub struct NewTicket<'a> {
    pub customer_id: Uuid,
    pub opened_by: Uuid,
    pub department_id: Uuid,
    pub service_id: Option<Uuid>,
    pub subject: &'a str,
    pub priority: &'a str,
    pub body: &'a str,
    pub by_staff: bool,
    pub attachments: &'a [Uuid],
}

/// Opens a ticket with its first message. Department must be active (for
/// clients) and the service, if any, must belong to the same customer.
pub async fn open(pool: &PgPool, t: NewTicket<'_>) -> AppResult<Ticket> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let dep_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ticket_departments WHERE id = $1 AND (is_active OR $2))",
    )
    .bind(t.department_id)
    .bind(t.by_staff)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if !dep_ok {
        return Err(AppError::Validation("error.invalid_department"));
    }
    if let Some(sid) = t.service_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM services WHERE id = $1 AND customer_id = $2)",
        )
        .bind(sid)
        .bind(t.customer_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        if !ok {
            return Err(AppError::Validation("error.invalid_service"));
        }
    }
    let status = if t.by_staff { "answered" } else { "open" };
    let id: Uuid = sqlx::query_scalar("INSERT INTO tickets (customer_id, opened_by, department_id, service_id, subject, priority, status) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
        .bind(t.customer_id).bind(t.opened_by).bind(t.department_id).bind(t.service_id).bind(t.subject).bind(t.priority).bind(status)
        .fetch_one(&mut *tx).await.map_err(db_err)?;
    let mid: Uuid = sqlx::query_scalar("INSERT INTO ticket_messages (ticket_id, author_id, is_staff, body) VALUES ($1,$2,$3,$4) RETURNING id")
        .bind(id).bind(t.opened_by).bind(t.by_staff).bind(t.body).fetch_one(&mut *tx).await.map_err(db_err)?;
    crate::ticket_attachments::link(&mut tx, t.opened_by, id, mid, t.attachments).await?;
    tx.commit().await.map_err(db_err)?;
    get(pool, id, None).await?.ok_or(AppError::NotFound)
}

/// `customer`: restrict to that customer's tickets (client area).
pub async fn get(pool: &PgPool, id: Uuid, customer: Option<Uuid>) -> AppResult<Option<Ticket>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} {FROM} WHERE t.id = $1 AND ($2::uuid IS NULL OR t.customer_id = $2)"
    ))
    .bind(id)
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn list(
    pool: &PgPool,
    customer: Option<Uuid>,
    status: Option<&str>,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Ticket>, i64)> {
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        t: Ticket,
        total: i64,
    }
    let like = q.map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS}, count(*) OVER() AS total {FROM}
         WHERE ($1::uuid IS NULL OR t.customer_id = $1)
           AND ($2::text IS NULL OR ($2 = 'active' AND t.status <> 'closed') OR t.status = $2)
           AND ($3::text IS NULL OR t.subject ILIKE $3 OR COALESCE(c.email::text, t.guest_email) ILIKE $3 OR t.number::text = $4)
         ORDER BY (t.status = 'closed'), t.last_reply_at DESC LIMIT $5 OFFSET $6"
    ))
    .bind(customer).bind(status).bind(like.as_deref()).bind(q).bind(limit).bind(offset)
    .fetch_all(pool).await.map_err(db_err)?;
    let total = rows.first().map(|r| r.total).unwrap_or(0);
    Ok((rows.into_iter().map(|r| r.t).collect(), total))
}

pub async fn messages(
    pool: &PgPool,
    ticket: Uuid,
    include_internal: bool,
) -> AppResult<Vec<Message>> {
    sqlx::query_as(
        "SELECT m.id, m.author_id, NULLIF(u.display_name, '') AS author_name, u.avatar_version AS author_avatar_version, m.is_staff, m.is_internal, m.body, m.created_at, m.edited_at
         FROM ticket_messages m LEFT JOIN users u ON u.id = m.author_id
         WHERE m.ticket_id = $1 AND (NOT m.is_internal OR $2) ORDER BY m.created_at, m.id",
    )
    .bind(ticket).bind(include_internal).fetch_all(pool).await.map_err(db_err)
}

/// Adds a reply and moves the status (staff reply -> answered, client reply
/// -> customer_reply, internal notes leave it). Replying reopens a closed ticket.
pub async fn reply(
    pool: &PgPool,
    ticket: Uuid,
    author: Uuid,
    is_staff: bool,
    internal: bool,
    body: &str,
    attachments: &[Uuid],
) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let mid: Uuid = sqlx::query_scalar("INSERT INTO ticket_messages (ticket_id, author_id, is_staff, is_internal, body) VALUES ($1,$2,$3,$4,$5) RETURNING id")
        .bind(ticket).bind(author).bind(is_staff).bind(internal).bind(body).fetch_one(&mut *tx).await.map_err(db_err)?;
    crate::ticket_attachments::link(&mut tx, author, ticket, mid, attachments).await?;
    if !internal {
        let st = if is_staff {
            "answered"
        } else {
            "customer_reply"
        };
        sqlx::query(
            "UPDATE tickets SET status = $2, last_reply_at = now(), closed_at = NULL,
                first_response_at = CASE WHEN $3 THEN COALESCE(first_response_at, now()) ELSE first_response_at END
             WHERE id = $1",
        )
        .bind(ticket)
        .bind(st)
        .bind(is_staff)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    status: Option<&str>,
    priority: Option<&str>,
    department: Option<Uuid>,
    assigned_to: Option<Option<Uuid>>,
) -> AppResult<()> {
    let r = sqlx::query(
        "UPDATE tickets SET status = COALESCE($2, status), priority = COALESCE($3, priority), department_id = COALESCE($4, department_id),
            assigned_to = CASE WHEN $5 THEN $6 ELSE assigned_to END,
            closed_at = CASE WHEN COALESCE($2, status) = 'closed' THEN COALESCE(closed_at, now()) ELSE NULL END
         WHERE id = $1",
    )
    .bind(id).bind(status).bind(priority).bind(department).bind(assigned_to.is_some()).bind(assigned_to.flatten())
    .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Count of tickets waiting on staff (dashboard badge).
pub async fn awaiting_staff(pool: &PgPool) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM tickets t JOIN ticket_statuses st ON st.slug = t.status WHERE st.awaiting_staff")
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Replace a message's text. Returns the previous body.
pub async fn edit_message(pool: &PgPool, ticket: Uuid, message: Uuid, body: &str) -> AppResult<String> {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 20000 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let old: Option<String> = sqlx::query_scalar("SELECT body FROM ticket_messages WHERE id = $1 AND ticket_id = $2 FOR UPDATE")
        .bind(message).bind(ticket).fetch_optional(&mut *tx).await.map_err(db_err)?;
    let old = old.ok_or(AppError::NotFound)?;
    sqlx::query("UPDATE ticket_messages SET body = $2, edited_at = now() WHERE id = $1")
        .bind(message).bind(body).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(old)
}

/// Delete a reply and its attachments. The opening message cannot be deleted
/// (delete the ticket instead).
pub async fn delete_message(pool: &PgPool, ticket: Uuid, message: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let first: Option<Uuid> = sqlx::query_scalar("SELECT id FROM ticket_messages WHERE ticket_id = $1 ORDER BY created_at, id LIMIT 1 FOR UPDATE")
        .bind(ticket).fetch_optional(&mut *tx).await.map_err(db_err)?;
    if first == Some(message) {
        return Err(AppError::Conflict("error.ticket_first_message"));
    }
    let r = sqlx::query("DELETE FROM ticket_messages WHERE id = $1 AND ticket_id = $2")
        .bind(message).bind(ticket).execute(&mut *tx).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}
