//! Ticket attachments: pending uploads are owned by the uploader and linked
//! to a message atomically inside the reply transaction.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

pub const MAX_PER_MESSAGE: usize = 5;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Attachment {
    pub id: Uuid,
    pub message_id: Option<Uuid>,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i32,
}

pub async fn create_pending(
    pool: &PgPool,
    user: Uuid,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> AppResult<Attachment> {
    // Drop this user's stale, never-linked uploads.
    sqlx::query("DELETE FROM ticket_attachments WHERE uploaded_by = $1 AND message_id IS NULL AND created_at < now() - interval '1 day'")
        .bind(user).execute(pool).await.map_err(db_err)?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ticket_attachments WHERE uploaded_by = $1 AND message_id IS NULL",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if pending >= MAX_PENDING_PER_USER {
        return Err(AppError::Validation("error.too_many_attachments"));
    }
    sqlx::query_as("INSERT INTO ticket_attachments (uploaded_by, filename, content_type, size_bytes, data) VALUES ($1,$2,$3,$4,$5)
                    RETURNING id, message_id, filename, content_type, size_bytes")
        .bind(user).bind(filename).bind(content_type).bind(data.len() as i32).bind(data)
        .fetch_one(pool).await.map_err(db_err)
}

/// Max never-linked uploads a single user may hold at once.
pub const MAX_PENDING_PER_USER: i64 = 20;

/// Delete every never-linked upload older than `max_age_hours` (all users).
/// Run periodically by the worker so abandoned uploads don't pile up.
pub async fn purge_orphans(pool: &PgPool, max_age_hours: i32) -> AppResult<u64> {
    let n = sqlx::query("DELETE FROM ticket_attachments WHERE message_id IS NULL AND created_at < now() - make_interval(hours => $1)")
        .bind(max_age_hours).execute(pool).await.map_err(db_err)?.rows_affected();
    Ok(n)
}

/// Links pending uploads of `user` to a message. Every id must be a pending
/// upload of that user, otherwise the whole reply is rejected.
pub async fn link(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    ticket: Uuid,
    message: Uuid,
    ids: &[Uuid],
) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    if ids.len() > MAX_PER_MESSAGE {
        return Err(AppError::Validation("error.too_many_attachments"));
    }
    let mut uniq = ids.to_vec();
    uniq.sort();
    uniq.dedup();
    let n = sqlx::query("UPDATE ticket_attachments SET ticket_id = $3, message_id = $4 WHERE id = ANY($1) AND uploaded_by = $2 AND message_id IS NULL")
        .bind(&uniq).bind(user).bind(ticket).bind(message).execute(&mut **tx).await.map_err(db_err)?.rows_affected();
    if n as usize != uniq.len() {
        return Err(AppError::Validation("error.invalid_attachment"));
    }
    Ok(())
}

/// Attachment metadata for a ticket (internal-note files only for staff).
pub async fn for_ticket(
    pool: &PgPool,
    ticket: Uuid,
    include_internal: bool,
) -> AppResult<Vec<Attachment>> {
    sqlx::query_as("SELECT a.id, a.message_id, a.filename, a.content_type, a.size_bytes FROM ticket_attachments a
                    JOIN ticket_messages m ON m.id = a.message_id
                    WHERE a.ticket_id = $1 AND (NOT m.is_internal OR $2) ORDER BY a.created_at, a.id")
        .bind(ticket).bind(include_internal).fetch_all(pool).await.map_err(db_err)
}

/// (filename, content_type, bytes) of a linked attachment on `ticket`.
pub async fn download(
    pool: &PgPool,
    ticket: Uuid,
    id: Uuid,
    include_internal: bool,
) -> AppResult<Option<(String, String, Vec<u8>)>> {
    sqlx::query_as("SELECT a.filename, a.content_type, a.data FROM ticket_attachments a JOIN ticket_messages m ON m.id = a.message_id
                    WHERE a.id = $1 AND a.ticket_id = $2 AND (NOT m.is_internal OR $3)")
        .bind(id).bind(ticket).bind(include_internal).fetch_optional(pool).await.map_err(db_err)
}
