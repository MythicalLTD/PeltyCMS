//! Live chat sessions and messages (polling; ids are monotonic so clients
//! fetch `after` the last id they have).
use crate::db_err;
pub use crate::live_chat_ext::*;
use pelty_core::{AppError, AppResult};
use rand::Rng;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const MAX_BODY: usize = 2000;
/// Open sessions one visitor IP/email may hold at once.
pub const MAX_OPEN_PER_EMAIL: i64 = 3;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Message {
    pub id: i64,
    pub author: String,
    #[schema(value_type = Option<String>)]
    pub staff_user_id: Option<Uuid>,
    pub staff_name: Option<String>,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Session {
    #[schema(value_type = String)]
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub page: String,
    pub status: String,
    #[schema(value_type = Option<String>)]
    pub customer_id: Option<Uuid>,
    #[schema(value_type = Option<String>)]
    pub assigned_to: Option<Uuid>,
    pub department_id: Option<Uuid>,
    pub ticket_id: Option<Uuid>,
    pub staff_unread: i32,
    pub last_body: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub last_message_at: OffsetDateTime,
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn clean_body(b: &str) -> AppResult<String> {
    let b = b.trim();
    if b.is_empty() || b.chars().count() > MAX_BODY {
        return Err(AppError::Validation("error.chat_message_invalid"));
    }
    Ok(b.to_string())
}

/// Compatibility entry point for callers that do not select a department.
pub async fn start(
    pool: &PgPool,
    name: &str,
    email: &str,
    page: &str,
    customer: Option<Uuid>,
    body: &str,
) -> AppResult<(Uuid, String)> {
    start_in_department(pool, name, email, page, customer, body, None).await
}

/// Start a session and its first message atomically. Returns (id, bearer token).
pub async fn start_in_department(
    pool: &PgPool,
    name: &str,
    email: &str,
    page: &str,
    customer: Option<Uuid>,
    body: &str,
    department: Option<Uuid>,
) -> AppResult<(Uuid, String)> {
    let name = name.trim();
    let email = email.trim().to_ascii_lowercase();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::Validation("error.chat_name_invalid"));
    }
    if email.len() > 254
        || !email
            .split_once('@')
            .is_some_and(|(a, d)| !a.is_empty() && d.contains('.'))
    {
        return Err(AppError::Validation("error.invalid_email"));
    }
    let body = clean_body(body)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    // Serialize the email quota, including concurrent starts on other instances.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 327))")
        .bind(&email)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_sessions WHERE email = $1 AND status = 'open'",
    )
    .bind(&email)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if open >= MAX_OPEN_PER_EMAIL {
        return Err(AppError::RateLimited);
    }
    let mut raw = [0u8; 32];
    rand::rng().fill_bytes(&mut raw);
    let token = hex::encode(raw);
    let id = Uuid::new_v4();
    let page: String = page.chars().take(500).collect();
    sqlx::query("INSERT INTO chat_sessions (id, token_hash, customer_id, name, email, page, staff_unread, department_id) VALUES ($1,$2,$3,$4,$5,$6,1,$7)")
        .bind(id).bind(hash(&token)).bind(customer).bind(name).bind(&email).bind(&page).bind(department)
        .execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("INSERT INTO chat_messages (session_id, author, body) VALUES ($1,'visitor',$2)")
        .bind(id)
        .bind(&body)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((id, token))
}

/// Check a visitor token; returns the session status.
pub async fn authorize(pool: &PgPool, id: Uuid, token: &str) -> AppResult<String> {
    let st: Option<String> =
        sqlx::query_scalar("SELECT status FROM chat_sessions WHERE id = $1 AND token_hash = $2")
            .bind(id)
            .bind(hash(token))
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    st.ok_or(AppError::NotFound)
}

pub async fn status(pool: &PgPool, id: Uuid) -> AppResult<String> {
    let st: Option<String> = sqlx::query_scalar("SELECT status FROM chat_sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    st.ok_or(AppError::NotFound)
}

/// Messages after `after`, oldest first (max 200). Marks them read for `reader`.
pub async fn messages(
    pool: &PgPool,
    id: Uuid,
    after: i64,
    reader: &str,
) -> AppResult<Vec<Message>> {
    let col = if reader == "staff" {
        "staff_unread"
    } else {
        "visitor_unread"
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE chat_sessions SET {col} = 0 WHERE id = $1 AND {col} <> 0"
    )))
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    sqlx::query_as(
        "SELECT m.id, m.author, m.staff_user_id, NULLIF(u.display_name, '') AS staff_name, m.body, m.created_at
         FROM chat_messages m LEFT JOIN users u ON u.id = m.staff_user_id
         WHERE m.session_id = $1 AND m.id > $2 ORDER BY m.id LIMIT 200",
    )
    .bind(id).bind(after).fetch_all(pool).await.map_err(db_err)
}

/// Append a message; refused once the session is closed.
pub async fn post(
    pool: &PgPool,
    id: Uuid,
    author: &str,
    staff: Option<Uuid>,
    body: &str,
) -> AppResult<i64> {
    let body = clean_body(body)?;
    let bump = if author == "staff" {
        "visitor_unread"
    } else {
        "staff_unread"
    };
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE chat_sessions SET {bump} = {bump} + 1, last_message_at = now(), assigned_to = COALESCE(assigned_to, $2::uuid) WHERE id = $1 AND status = 'open'")))
        .bind(id).bind(staff).execute(&mut *tx).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.chat_closed"));
    }
    let mid: i64 = sqlx::query_scalar("INSERT INTO chat_messages (session_id, author, staff_user_id, body) VALUES ($1,$2,$3,$4) RETURNING id")
        .bind(id).bind(author).bind(staff).bind(&body).fetch_one(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(mid)
}

/// Staff suggestion card for the visitor (system JSON body); bumps visitor unread.
pub async fn post_action(pool: &PgPool, id: Uuid, staff: Uuid, body: &str) -> AppResult<i64> {
    let body = clean_body(body)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query(
        "UPDATE chat_sessions SET visitor_unread = visitor_unread + 1, last_message_at = now(), assigned_to = COALESCE(assigned_to, $2) WHERE id = $1 AND status = 'open'",
    )
    .bind(id)
    .bind(staff)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.chat_closed"));
    }
    let mid: i64 = sqlx::query_scalar(
        "INSERT INTO chat_messages (session_id, author, staff_user_id, body) VALUES ($1,'system',$2,$3) RETURNING id",
    )
    .bind(id)
    .bind(staff)
    .bind(&body)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(mid)
}

/// True when the product id exists (any status; staff may still suggest it).
pub async fn product_exists(pool: &PgPool, product: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1)")
        .bind(product)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn close(pool: &PgPool, id: Uuid, by: &str) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("UPDATE chat_sessions SET status = 'closed', closed_at = now() WHERE id = $1 AND status = 'open'")
        .bind(id).execute(&mut *tx).await.map_err(db_err)?;
    if r.rows_affected() > 0 {
        let note = if by == "staff" {
            "closed_by_staff"
        } else {
            "closed_by_visitor"
        };
        sqlx::query("INSERT INTO chat_messages (session_id, author, body) VALUES ($1,'system',$2)")
            .bind(id)
            .bind(note)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

/// Staff inbox: open sessions first, newest activity first.
pub async fn list(pool: &PgPool, status: Option<&str>) -> AppResult<Vec<Session>> {
    sqlx::query_as(
        "SELECT s.id, s.name, s.email, s.page, s.status, s.customer_id, s.assigned_to, s.department_id, s.ticket_id, s.staff_unread,
                (SELECT body FROM chat_messages m WHERE m.session_id = s.id AND m.author <> 'system' ORDER BY m.id DESC LIMIT 1) AS last_body,
                s.created_at, s.last_message_at
         FROM chat_sessions s WHERE ($1::text IS NULL OR s.status = $1)
         ORDER BY (s.status = 'open') DESC, s.last_message_at DESC LIMIT 200",
    )
    .bind(status).fetch_all(pool).await.map_err(db_err)
}

pub async fn unread_total(pool: &PgPool) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(staff_unread), 0)::bigint FROM chat_sessions WHERE status = 'open'",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)
}
