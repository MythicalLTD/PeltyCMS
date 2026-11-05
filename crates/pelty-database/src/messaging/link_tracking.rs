//! Link tracking: short `/go/{slug}` links that redirect and count clicks.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Link {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub target_url: String,
    pub clicks: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_click_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Validate slug/name/url; returns normalized values.
pub fn validate(slug: &str, name: &str, url: &str) -> AppResult<(String, String, String)> {
    let slug = slug.trim().to_ascii_lowercase();
    let ok_slug = (2..=63).contains(&slug.len())
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-');
    let name = name.trim().to_string();
    let url = url.trim().to_string();
    let ok_url = (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() <= 2000
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && url
            .split_once("://")
            .is_some_and(|(_, h)| !h.is_empty() && !h.starts_with('/'));
    if !ok_slug || name.is_empty() || name.chars().count() > 200 || !ok_url {
        return Err(AppError::Validation("error.validation"));
    }
    Ok((slug, name, url))
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<Link>> {
    sqlx::query_as("SELECT id, slug, name, target_url, clicks, last_click_at, created_at FROM tracked_links ORDER BY created_at DESC, id")
        .fetch_all(pool).await.map_err(db_err)
}

pub async fn create(pool: &PgPool, slug: &str, name: &str, url: &str) -> AppResult<Uuid> {
    let (slug, name, url) = validate(slug, name, url)?;
    let id = Uuid::new_v4();
    let r = sqlx::query("INSERT INTO tracked_links (id, slug, name, target_url) VALUES ($1,$2,$3,$4) ON CONFLICT (slug) DO NOTHING")
        .bind(id).bind(slug).bind(name).bind(url).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.link_slug_taken"));
    }
    Ok(id)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM tracked_links WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Count a click and return the target, if the slug exists.
pub async fn hit(pool: &PgPool, slug: &str) -> AppResult<Option<String>> {
    sqlx::query_scalar("UPDATE tracked_links SET clicks = clicks + 1, last_click_at = now() WHERE slug = $1 RETURNING target_url")
        .bind(slug.to_ascii_lowercase()).fetch_optional(pool).await.map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::validate;
    #[test]
    fn validates() {
        assert!(validate("promo-1", "Promo", "https://example.com/x?a=1").is_ok());
        assert_eq!(validate(" Promo ", "P", "http://a.b").unwrap().0, "promo");
        assert!(validate("-x", "P", "https://a.b").is_err());
        assert!(validate("a/b", "P", "https://a.b").is_err());
        assert!(validate("ok", "P", "javascript:alert(1)").is_err());
        assert!(validate("ok", "P", "https:///evil").is_err());
        assert!(validate("ok", " ", "https://a.b").is_err());
    }
}
