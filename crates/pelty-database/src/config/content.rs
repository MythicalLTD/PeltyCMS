//! Knowledge base and announcements. Public reads only ever return published
//! rows; `locale` NULL matches every language.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Category {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub sort_order: i32,
    pub article_count: i64,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Article {
    pub id: Uuid,
    pub category_id: Uuid,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub locale: Option<String>,
    pub is_published: bool,
    pub views: i64,
    pub helpful_yes: i64,
    pub helpful_no: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Announcement {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub locale: Option<String>,
    pub is_published: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: OffsetDateTime,
}

pub struct ArticleInput<'a> {
    pub category_id: Uuid,
    pub slug: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub locale: Option<&'a str>,
    pub is_published: bool,
}

pub struct AnnouncementInput<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub locale: Option<&'a str>,
    pub is_published: bool,
    pub published_at: Option<OffsetDateTime>,
}

const ART: &str = "id, category_id, slug, title, body, locale, is_published, views, helpful_yes, helpful_no, updated_at";
const ANN: &str = "id, title, body, locale, is_published, published_at";

pub async fn categories(pool: &PgPool, published_only: bool, locale: Option<&str>) -> AppResult<Vec<Category>> {
    sqlx::query_as(
        "SELECT c.id, c.slug, c.name, c.description, c.sort_order,
                count(a.id) FILTER (WHERE (NOT $1 OR a.is_published) AND ($2::text IS NULL OR a.locale IS NULL OR a.locale = $2)) AS article_count
         FROM kb_categories c LEFT JOIN kb_articles a ON a.category_id = c.id
         GROUP BY c.id ORDER BY c.sort_order, c.name",
    )
    .bind(published_only)
    .bind(locale)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_category(pool: &PgPool, slug: &str, name: &str, description: &str, sort_order: i32) -> AppResult<Uuid> {
    sqlx::query_scalar("INSERT INTO kb_categories (slug, name, description, sort_order) VALUES ($1,$2,$3,$4) RETURNING id")
        .bind(slug)
        .bind(name)
        .bind(description)
        .bind(sort_order)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn update_category(pool: &PgPool, id: Uuid, slug: &str, name: &str, description: &str, sort_order: i32) -> AppResult<bool> {
    let r = sqlx::query("UPDATE kb_categories SET slug=$2, name=$3, description=$4, sort_order=$5 WHERE id=$1")
        .bind(id)
        .bind(slug)
        .bind(name)
        .bind(description)
        .bind(sort_order)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Fails with a FK violation (mapped by `db_err`) while articles remain.
pub async fn delete_category(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM kb_categories WHERE id=$1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Article list. `q` uses Postgres full-text search (simple config, prefix match).
pub async fn articles(pool: &PgPool, published_only: bool, locale: Option<&str>, category: Option<Uuid>, q: Option<&str>, limit: i64) -> AppResult<Vec<Article>> {
    let tsq = q.map(|q| {
        q.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .take(8)
            .map(|w| format!("{w}:*"))
            .collect::<Vec<_>>()
            .join(" & ")
    }).filter(|s| !s.is_empty());
    sqlx::query_as(&format!(
        "SELECT {ART} FROM kb_articles
         WHERE (NOT $1 OR is_published) AND ($2::text IS NULL OR locale IS NULL OR locale = $2)
           AND ($3::uuid IS NULL OR category_id = $3)
           AND ($4::text IS NULL OR search @@ to_tsquery('simple', $4))
         ORDER BY CASE WHEN $4::text IS NULL THEN 0 ELSE -ts_rank(search, to_tsquery('simple', $4)) END, views DESC, title
         LIMIT $5"
    ))
    .bind(published_only)
    .bind(locale)
    .bind(category)
    .bind(tsq)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// OR-of-prefixes tsquery for "related articles" suggestions: words of 3+
/// letters, lowercased, deduped, at most 10. `None` when nothing is usable.
pub fn suggest_query(text: &str) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3 && seen.insert(w.clone()))
        .take(10)
        .map(|w| format!("{w}:*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" | "))
}

/// Published articles most relevant to free text (e.g. a new ticket's subject).
pub async fn suggest(pool: &PgPool, locale: Option<&str>, text: &str, limit: i64) -> AppResult<Vec<Article>> {
    let Some(q) = suggest_query(text) else { return Ok(Vec::new()) };
    sqlx::query_as(&format!(
        "SELECT {ART} FROM kb_articles
         WHERE is_published AND ($1::text IS NULL OR locale IS NULL OR locale = $1) AND search @@ to_tsquery('simple', $2)
         ORDER BY ts_rank(search, to_tsquery('simple', $2)) DESC, views DESC, title LIMIT $3"
    ))
    .bind(locale)
    .bind(q)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn article(pool: &PgPool, id: Uuid) -> AppResult<Option<Article>> {
    sqlx::query_as(&format!("SELECT {ART} FROM kb_articles WHERE id=$1")).bind(id).fetch_optional(pool).await.map_err(db_err)
}

/// Published article by slug; increments the view counter.
pub async fn view_article(pool: &PgPool, slug: &str) -> AppResult<Option<Article>> {
    sqlx::query_as(&format!("UPDATE kb_articles SET views = views + 1 WHERE slug=$1 AND is_published RETURNING {ART}"))
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn vote(pool: &PgPool, slug: &str, helpful: bool) -> AppResult<bool> {
    let r = sqlx::query(
        "UPDATE kb_articles SET helpful_yes = helpful_yes + CASE WHEN $2 THEN 1 ELSE 0 END,
                                helpful_no = helpful_no + CASE WHEN $2 THEN 0 ELSE 1 END
         WHERE slug=$1 AND is_published",
    )
    .bind(slug)
    .bind(helpful)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn create_article(pool: &PgPool, a: &ArticleInput<'_>) -> AppResult<Article> {
    sqlx::query_as(&format!(
        "INSERT INTO kb_articles (category_id, slug, title, body, locale, is_published) VALUES ($1,$2,$3,$4,$5,$6) RETURNING {ART}"
    ))
    .bind(a.category_id)
    .bind(a.slug)
    .bind(a.title)
    .bind(a.body)
    .bind(a.locale)
    .bind(a.is_published)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update_article(pool: &PgPool, id: Uuid, a: &ArticleInput<'_>) -> AppResult<Option<Article>> {
    sqlx::query_as(&format!(
        "UPDATE kb_articles SET category_id=$2, slug=$3, title=$4, body=$5, locale=$6, is_published=$7, updated_at=now()
         WHERE id=$1 RETURNING {ART}"
    ))
    .bind(id)
    .bind(a.category_id)
    .bind(a.slug)
    .bind(a.title)
    .bind(a.body)
    .bind(a.locale)
    .bind(a.is_published)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn delete_article(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM kb_articles WHERE id=$1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn announcements(pool: &PgPool, published_only: bool, locale: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Announcement>, i64)> {
    let filter = "(NOT $1 OR (is_published AND published_at <= now())) AND ($2::text IS NULL OR locale IS NULL OR locale = $2)";
    let items = sqlx::query_as(&format!("SELECT {ANN} FROM announcements WHERE {filter} ORDER BY published_at DESC LIMIT $3 OFFSET $4"))
        .bind(published_only)
        .bind(locale)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let total = sqlx::query_scalar(&format!("SELECT count(*) FROM announcements WHERE {filter}"))
        .bind(published_only)
        .bind(locale)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok((items, total))
}

pub async fn announcement(pool: &PgPool, id: Uuid, published_only: bool) -> AppResult<Option<Announcement>> {
    sqlx::query_as(&format!("SELECT {ANN} FROM announcements WHERE id=$1 AND (NOT $2 OR (is_published AND published_at <= now()))"))
        .bind(id)
        .bind(published_only)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn create_announcement(pool: &PgPool, a: &AnnouncementInput<'_>) -> AppResult<Announcement> {
    sqlx::query_as(&format!(
        "INSERT INTO announcements (title, body, locale, is_published, published_at) VALUES ($1,$2,$3,$4,COALESCE($5, now())) RETURNING {ANN}"
    ))
    .bind(a.title)
    .bind(a.body)
    .bind(a.locale)
    .bind(a.is_published)
    .bind(a.published_at)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update_announcement(pool: &PgPool, id: Uuid, a: &AnnouncementInput<'_>) -> AppResult<Option<Announcement>> {
    sqlx::query_as(&format!(
        "UPDATE announcements SET title=$2, body=$3, locale=$4, is_published=$5, published_at=COALESCE($6, published_at), updated_at=now()
         WHERE id=$1 RETURNING {ANN}"
    ))
    .bind(id)
    .bind(a.title)
    .bind(a.body)
    .bind(a.locale)
    .bind(a.is_published)
    .bind(a.published_at)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn delete_announcement(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM announcements WHERE id=$1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

#[cfg(test)]
mod suggest_tests {
    #[test]
    fn query() {
        assert_eq!(super::suggest_query("My VPS is DOWN, vps down!").as_deref(), Some("vps:* | down:*"));
        assert_eq!(super::suggest_query("a b !!"), None);
        assert_eq!(super::suggest_query("o'reilly & co"), Some("reilly:*".into()));
    }
}
