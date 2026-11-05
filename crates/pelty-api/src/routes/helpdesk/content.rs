//! Knowledge base and announcements: public reads (published only) and the
//! staff editor (`content.manage`). Bodies are plain text / Markdown; the
//! frontend renders them as text, never as raw HTML.
use crate::routes::{paged, Page};
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::content::{self, AnnouncementInput, ArticleInput};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/kb", get(public_kb))
        .route("/public/kb/suggest", get(public_suggest))
        .route("/public/kb/articles/{slug}", get(public_article))
        .route("/public/kb/articles/{slug}/vote", post(public_vote))
        .route("/public/announcements", get(public_announcements))
        .route("/public/announcements/{id}", get(public_announcement))
        .route("/admin/kb/categories", get(admin_categories).post(create_category))
        .route("/admin/kb/categories/{id}", put(update_category).delete(delete_category))
        .route("/admin/kb/articles", get(admin_articles).post(create_article))
        .route("/admin/kb/articles/{id}", get(admin_article).put(update_article).delete(delete_article))
        .route("/admin/announcements", get(admin_announcements).post(create_announcement))
        .route("/admin/announcements/{id}", put(update_announcement).delete(delete_announcement))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct KbQuery {
    pub locale: Option<String>,
    pub category: Option<Uuid>,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AnnQuery {
    pub locale: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CategoryBody {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ArticleBody {
    pub category_id: Uuid,
    pub slug: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub is_published: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AnnouncementBody {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub is_published: bool,
    /// RFC 3339; omit for "now" (create) or "unchanged" (update).
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct VoteBody {
    pub helpful: bool,
}

/// Unknown locales are ignored (treated as "any") for public reads.
fn public_locale<'a>(s: &AppState, l: &'a Option<String>) -> Option<&'a str> {
    l.as_deref().filter(|l| s.i18n.has(l))
}

fn content_locale(s: &AppState, l: &Option<String>) -> Result<Option<String>, AppError> {
    match l.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        None => Ok(None),
        Some(l) if s.i18n.has(l) => Ok(Some(l.to_string())),
        Some(_) => Err(AppError::Validation("error.unknown_locale")),
    }
}

fn valid_slug(s: &str, max: usize) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= max
        && b[0] != b'-'
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn text(s: &str, max: usize, code: &'static str) -> Result<String, AppError> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > max {
        return Err(AppError::Validation(code));
    }
    Ok(t.to_string())
}

fn body(s: &str, max: usize) -> Result<(), AppError> {
    if s.trim().is_empty() || s.chars().count() > max {
        return Err(AppError::Validation("error.invalid_body"));
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/public/kb", params(KbQuery), responses((status = 200)))]
pub async fn public_kb(State(s): State<AppState>, Query(q): Query<KbQuery>) -> ApiResult<Json<Value>> {
    let loc = public_locale(&s, &q.locale);
    let cats = content::categories(&s.pool, true, loc).await?;
    let search = q.q.as_deref().map(str::trim).filter(|v| !v.is_empty() && v.len() <= 200);
    let arts = content::articles(&s.pool, true, loc, q.category, search, 100).await?;
    let arts: Vec<Value> = arts
        .into_iter()
        .map(|a| json!({ "id": a.id, "category_id": a.category_id, "slug": a.slug, "title": a.title, "excerpt": excerpt(&a.body), "views": a.views, "updated_at": a.updated_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default() }))
        .collect();
    Ok(Json(json!({ "categories": cats.into_iter().filter(|c| c.article_count > 0).collect::<Vec<_>>(), "articles": arts })))
}

/// Up to 5 published articles related to `q` (used while opening a ticket).
#[utoipa::path(get, path = "/api/v1/public/kb/suggest", params(KbQuery), responses((status = 200)))]
pub async fn public_suggest(State(s): State<AppState>, Query(q): Query<KbQuery>) -> ApiResult<Json<Value>> {
    let loc = public_locale(&s, &q.locale);
    let text: String = q.q.as_deref().unwrap_or("").chars().take(300).collect();
    let arts = content::suggest(&s.pool, loc, &text, 5).await?;
    let arts: Vec<Value> = arts.into_iter().map(|a| json!({ "slug": a.slug, "title": a.title, "excerpt": excerpt(&a.body) })).collect();
    Ok(Json(json!({ "articles": arts })))
}

fn excerpt(b: &str) -> String {
    let one: String = b.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= 180 {
        one
    } else {
        format!("{}…", one.chars().take(180).collect::<String>())
    }
}

#[utoipa::path(get, path = "/api/v1/public/kb/articles/{slug}", responses((status = 200), (status = 404)))]
pub async fn public_article(State(s): State<AppState>, Path(slug): Path<String>) -> ApiResult<Json<Value>> {
    let a = content::view_article(&s.pool, &slug).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "article": a })))
}

#[utoipa::path(post, path = "/api/v1/public/kb/articles/{slug}/vote", request_body = VoteBody, responses((status = 204), (status = 404)))]
pub async fn public_vote(State(s): State<AppState>, Path(slug): Path<String>, Json(b): Json<VoteBody>) -> ApiResult<StatusCode> {
    if !content::vote(&s.pool, &slug, b.helpful).await? {
        return Err(AppError::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/public/announcements", params(AnnQuery), responses((status = 200)))]
pub async fn public_announcements(State(s): State<AppState>, Query(q): Query<AnnQuery>) -> ApiResult<Json<Value>> {
    let (lim, off, page, per) = Page { page: q.page, per_page: q.per_page }.bounds();
    let (items, total) = content::announcements(&s.pool, true, public_locale(&s, &q.locale), lim, off).await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/public/announcements/{id}", responses((status = 200), (status = 404)))]
pub async fn public_announcement(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let a = content::announcement(&s.pool, id, true).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "announcement": a })))
}

#[utoipa::path(get, path = "/api/v1/admin/kb/categories", responses((status = 200), (status = 403)))]
pub async fn admin_categories(State(s): State<AppState>, _p: RequirePermission<perms::ContentManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": content::categories(&s.pool, false, None).await? })))
}

fn check_category(b: &CategoryBody) -> Result<(String, String), AppError> {
    if !valid_slug(&b.slug, 80) {
        return Err(AppError::Validation("error.invalid_slug"));
    }
    if b.description.chars().count() > 500 {
        return Err(AppError::Validation("error.validation"));
    }
    Ok((b.slug.clone(), text(&b.name, 120, "error.invalid_name")?))
}

#[utoipa::path(post, path = "/api/v1/admin/kb/categories", request_body = CategoryBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create_category(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Json(b): Json<CategoryBody>) -> ApiResult<Json<Value>> {
    let (slug, name) = check_category(&b)?;
    let id = content::create_category(&s.pool, &slug, &name, b.description.trim(), b.sort_order).await?;
    audit::record(&s.pool, Some(u.user_id), "kb.category_created", Some(&id.to_string()), u.ip.as_deref(), json!({ "slug": slug })).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(put, path = "/api/v1/admin/kb/categories/{id}", request_body = CategoryBody, responses((status = 204), (status = 404)))]
pub async fn update_category(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>, Json(b): Json<CategoryBody>) -> ApiResult<StatusCode> {
    let (slug, name) = check_category(&b)?;
    if !content::update_category(&s.pool, id, &slug, &name, b.description.trim(), b.sort_order).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "kb.category_updated", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/admin/kb/categories/{id}", responses((status = 204), (status = 404), (status = 409)))]
pub async fn delete_category(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = content::delete_category(&s.pool, id).await.map_err(|e| match e {
        AppError::Validation("error.invalid_reference") => AppError::Conflict("error.category_not_empty"),
        e => e,
    })?;
    if !deleted {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "kb.category_deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/kb/articles", params(KbQuery), responses((status = 200)))]
pub async fn admin_articles(State(s): State<AppState>, _p: RequirePermission<perms::ContentManage>, Query(q): Query<KbQuery>) -> ApiResult<Json<Value>> {
    let search = q.q.as_deref().map(str::trim).filter(|v| !v.is_empty() && v.len() <= 200);
    Ok(Json(json!({ "items": content::articles(&s.pool, false, None, q.category, search, 500).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/kb/articles/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_article(State(s): State<AppState>, _p: RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "article": content::article(&s.pool, id).await?.ok_or(AppError::NotFound)? })))
}

fn check_article(s: &AppState, b: &ArticleBody) -> Result<(String, Option<String>), AppError> {
    if !valid_slug(&b.slug, 120) {
        return Err(AppError::Validation("error.invalid_slug"));
    }
    body(&b.body, 100_000)?;
    Ok((text(&b.title, 200, "error.invalid_title")?, content_locale(s, &b.locale)?))
}

#[utoipa::path(post, path = "/api/v1/admin/kb/articles", request_body = ArticleBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create_article(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Json(b): Json<ArticleBody>) -> ApiResult<Json<Value>> {
    let (title, locale) = check_article(&s, &b)?;
    let a = content::create_article(&s.pool, &ArticleInput { category_id: b.category_id, slug: &b.slug, title: &title, body: &b.body, locale: locale.as_deref(), is_published: b.is_published }).await?;
    audit::record(&s.pool, Some(u.user_id), "kb.article_created", Some(&a.id.to_string()), u.ip.as_deref(), json!({ "slug": a.slug })).await?;
    Ok(Json(json!({ "article": a })))
}

#[utoipa::path(put, path = "/api/v1/admin/kb/articles/{id}", request_body = ArticleBody, responses((status = 200), (status = 404)))]
pub async fn update_article(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>, Json(b): Json<ArticleBody>) -> ApiResult<Json<Value>> {
    let (title, locale) = check_article(&s, &b)?;
    let a = content::update_article(&s.pool, id, &ArticleInput { category_id: b.category_id, slug: &b.slug, title: &title, body: &b.body, locale: locale.as_deref(), is_published: b.is_published })
        .await?
        .ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "kb.article_updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "published": a.is_published })).await?;
    Ok(Json(json!({ "article": a })))
}

#[utoipa::path(delete, path = "/api/v1/admin/kb/articles/{id}", responses((status = 204), (status = 404)))]
pub async fn delete_article(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    if !content::delete_article(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "kb.article_deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/announcements", params(Page), responses((status = 200)))]
pub async fn admin_announcements(State(s): State<AppState>, _p: RequirePermission<perms::ContentManage>, Query(p): Query<Page>) -> ApiResult<Json<Value>> {
    let (lim, off, page, per) = p.bounds();
    let (items, total) = content::announcements(&s.pool, false, None, lim, off).await?;
    Ok(Json(paged(items, page, per, total)))
}

fn check_announcement(s: &AppState, b: &AnnouncementBody) -> Result<(String, Option<String>), AppError> {
    body(&b.body, 50_000)?;
    Ok((text(&b.title, 200, "error.invalid_title")?, content_locale(s, &b.locale)?))
}

#[utoipa::path(post, path = "/api/v1/admin/announcements", request_body = AnnouncementBody, responses((status = 200), (status = 422)))]
pub async fn create_announcement(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Json(b): Json<AnnouncementBody>) -> ApiResult<Json<Value>> {
    let (title, locale) = check_announcement(&s, &b)?;
    let a = content::create_announcement(&s.pool, &AnnouncementInput { title: &title, body: &b.body, locale: locale.as_deref(), is_published: b.is_published, published_at: b.published_at }).await?;
    audit::record(&s.pool, Some(u.user_id), "announcement.created", Some(&a.id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "announcement": a })))
}

#[utoipa::path(put, path = "/api/v1/admin/announcements/{id}", request_body = AnnouncementBody, responses((status = 200), (status = 404)))]
pub async fn update_announcement(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>, Json(b): Json<AnnouncementBody>) -> ApiResult<Json<Value>> {
    let (title, locale) = check_announcement(&s, &b)?;
    let a = content::update_announcement(&s.pool, id, &AnnouncementInput { title: &title, body: &b.body, locale: locale.as_deref(), is_published: b.is_published, published_at: b.published_at })
        .await?
        .ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "announcement.updated", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "announcement": a })))
}

#[utoipa::path(delete, path = "/api/v1/admin/announcements/{id}", responses((status = 204), (status = 404)))]
pub async fn delete_announcement(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ContentManage>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    if !content::delete_announcement(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "announcement.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}
