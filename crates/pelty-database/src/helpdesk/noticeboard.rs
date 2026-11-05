//! Staff noticeboard: notices visible to every staff member.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Notice {
    pub id: Uuid,
    pub author_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub body: String,
    pub pinned: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn list(pool: &PgPool, limit: i64) -> AppResult<Vec<Notice>> {
    sqlx::query_as(
        "SELECT n.id, n.author_id, NULLIF(u.display_name, '') AS author_name, n.body, n.pinned, n.created_at
         FROM staff_notices n LEFT JOIN users u ON u.id = n.author_id
         ORDER BY n.pinned DESC, n.created_at DESC, n.id LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create(pool: &PgPool, author: Uuid, body: &str, pinned: bool) -> AppResult<Uuid> {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 5000 {
        return Err(AppError::Validation("error.validation"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO staff_notices (id, author_id, body, pinned) VALUES ($1,$2,$3,$4)")
        .bind(id).bind(author).bind(body).bind(pinned).execute(pool).await.map_err(db_err)?;
    Ok(id)
}

/// Delete a notice; staff without `any` may only remove their own.
pub async fn delete(pool: &PgPool, id: Uuid, user: Uuid, any: bool) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM staff_notices WHERE id = $1 AND ($3 OR author_id = $2)")
        .bind(id).bind(user).bind(any).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
