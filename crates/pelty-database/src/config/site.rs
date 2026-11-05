//! Public website CMS pages.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Page {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub blocks: Value,
    pub published: bool,
    pub show_in_nav: bool,
    pub sort_order: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub struct PageInput<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub blocks: &'a Value,
    pub published: bool,
    pub show_in_nav: bool,
    pub sort_order: i32,
}

const COLS: &str = "id, slug, title, description, blocks, published, show_in_nav, sort_order, updated_at";

pub async fn list(pool: &PgPool, published_only: bool) -> AppResult<Vec<Page>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM site_pages WHERE published OR NOT $1 ORDER BY sort_order, slug"))
        .bind(published_only)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Page>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM site_pages WHERE id = $1")).bind(id).fetch_optional(pool).await.map_err(db_err)
}

pub async fn by_slug(pool: &PgPool, slug: &str) -> AppResult<Option<Page>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM site_pages WHERE slug = $1")).bind(slug).fetch_optional(pool).await.map_err(db_err)
}

pub async fn create(pool: &PgPool, p: &PageInput<'_>) -> AppResult<Page> {
    sqlx::query_as(&format!(
        "INSERT INTO site_pages (slug, title, description, blocks, published, show_in_nav, sort_order) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {COLS}"
    ))
    .bind(p.slug)
    .bind(p.title)
    .bind(p.description)
    .bind(p.blocks)
    .bind(p.published)
    .bind(p.show_in_nav)
    .bind(p.sort_order)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, p: &PageInput<'_>) -> AppResult<Option<Page>> {
    sqlx::query_as(&format!(
        "UPDATE site_pages SET slug=$2, title=$3, description=$4, blocks=$5, published=$6, show_in_nav=$7, sort_order=$8, updated_at=now() WHERE id=$1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(p.slug)
    .bind(p.title)
    .bind(p.description)
    .bind(p.blocks)
    .bind(p.published)
    .bind(p.show_in_nav)
    .bind(p.sort_order)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM site_pages WHERE id = $1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}
