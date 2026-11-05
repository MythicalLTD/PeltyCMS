//! Admin-defined ticket statuses (built-ins are seeded by the migration).
use crate::db_err;
use pelty_core::AppError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TicketStatus {
    pub slug: String,
    pub name: String,
    pub color: String,
    pub sort_order: i32,
    pub awaiting_staff: bool,
    pub builtin: bool,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct StatusInput {
    #[serde(default)]
    pub slug: String,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub awaiting_staff: bool,
}

fn default_color() -> String {
    "secondary".into()
}

pub const COLORS: &[&str] = &["default", "secondary", "destructive", "outline"];

pub fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

pub fn validate(i: &StatusInput, creating: bool) -> Result<(), AppError> {
    let n = i.name.trim().chars().count();
    if (creating && !valid_slug(&i.slug))
        || n == 0
        || n > 60
        || !COLORS.contains(&i.color.as_str())
        || !(-1000..=1000).contains(&i.sort_order)
    {
        return Err(AppError::Validation("error.invalid_ticket_status"));
    }
    Ok(())
}

pub async fn list(pool: &PgPool) -> Result<Vec<TicketStatus>, AppError> {
    sqlx::query_as("SELECT slug, name, color, sort_order, awaiting_staff, builtin FROM ticket_statuses ORDER BY sort_order, slug")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn exists(pool: &PgPool, slug: &str) -> Result<bool, AppError> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM ticket_statuses WHERE slug = $1)")
        .bind(slug)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn create(pool: &PgPool, i: &StatusInput) -> Result<(), AppError> {
    let r = sqlx::query("INSERT INTO ticket_statuses (slug, name, color, sort_order, awaiting_staff) VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(&i.slug).bind(i.name.trim()).bind(&i.color).bind(i.sort_order).bind(i.awaiting_staff)
        .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.ticket_status_exists"));
    }
    Ok(())
}

/// Built-ins can be renamed/recoloured, but their workflow flag is fixed.
pub async fn update(pool: &PgPool, slug: &str, i: &StatusInput) -> Result<(), AppError> {
    let r = sqlx::query(
        "UPDATE ticket_statuses SET name = $2, color = $3, sort_order = $4,
            awaiting_staff = CASE WHEN builtin THEN awaiting_staff ELSE $5 END WHERE slug = $1",
    )
    .bind(slug)
    .bind(i.name.trim())
    .bind(&i.color)
    .bind(i.sort_order)
    .bind(i.awaiting_staff)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete(pool: &PgPool, slug: &str) -> Result<(), AppError> {
    let row: Option<(bool, i64)> = sqlx::query_as("SELECT builtin, (SELECT count(*) FROM tickets WHERE status = $1) FROM ticket_statuses WHERE slug = $1")
        .bind(slug).fetch_optional(pool).await.map_err(db_err)?;
    match row {
        None => Err(AppError::NotFound),
        Some((true, _)) => Err(AppError::Conflict("error.ticket_status_builtin")),
        Some((_, n)) if n > 0 => Err(AppError::Conflict("error.ticket_status_in_use")),
        _ => {
            sqlx::query("DELETE FROM ticket_statuses WHERE slug = $1")
                .bind(slug)
                .execute(pool)
                .await
                .map_err(db_err)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slugs() {
        assert!(valid_slug("waiting_vendor"));
        assert!(!valid_slug("A"));
        assert!(!valid_slug("1abc"));
        assert!(!valid_slug("has-dash"));
        let i = StatusInput {
            slug: "x".into(),
            name: "X".into(),
            color: "default".into(),
            sort_order: 0,
            awaiting_staff: false,
        };
        assert!(validate(&i, true).is_err());
        assert!(validate(&i, false).is_ok());
    }
}
