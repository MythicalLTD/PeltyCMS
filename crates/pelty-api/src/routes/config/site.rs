//! Public website configuration (brand, menus, footer, homepage) and CMS
//! pages made of declarative blocks rendered natively by the frontend.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::{AppError, AppResult};
use pelty_database::site::{self, PageInput};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const CONFIG_KEY: &str = "site.config";
pub const BLOCK_TYPES: &[&str] = &[
    "hero", "features", "pricing", "faq", "cta", "text", "image", "stats",
];
pub const FONTS: &[&str] = &["manrope", "inter", "system"];
pub const MODES: &[&str] = &["dark", "light", "system"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/site", get(public_site))
        .route("/public/site/pages/{slug}", get(public_page))
        .route("/admin/site", get(admin_get).put(admin_put))
        .route("/admin/site/pages", get(admin_pages).post(admin_create))
        .route(
            "/admin/site/pages/{id}",
            get(admin_page).put(admin_update).delete(admin_delete),
        )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct Link {
    pub label: String,
    pub href: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct FooterColumn {
    pub title: String,
    #[serde(default)]
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct SocialLink {
    pub network: String,
    pub href: String,
}

pub const SOCIAL_NETWORKS: &[&str] = &[
    "x",
    "github",
    "discord",
    "facebook",
    "instagram",
    "linkedin",
    "youtube",
    "mastodon",
    "email",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(default)]
pub struct SiteConfig {
    pub site_name: String,
    pub tagline: String,
    pub logo_url: String,
    pub primary_color: String,
    pub font: String,
    pub default_mode: String,
    pub homepage: String,
    pub nav: Vec<Link>,
    pub footer_columns: Vec<FooterColumn>,
    pub footer_text: String,
    pub social: Vec<SocialLink>,
    pub copyright: String,
    pub show_store_in_nav: bool,
    pub client_dashboard_widgets: Vec<String>,
}

impl Default for SiteConfig {
    fn default() -> Self {
        let l = |a: &str, b: &str| Link {
            label: a.into(),
            href: b.into(),
        };
        SiteConfig {
            site_name: "PeltyCMS".into(),
            tagline: "Hosting that just works.".into(),
            logo_url: String::new(),
            primary_color: String::new(),
            font: "manrope".into(),
            default_mode: "dark".into(),
            homepage: "home".into(),
            nav: vec![
                l("Plans", "/store"),
                l("Knowledgebase", "/kb"),
                l("Status", "/status"),
            ],
            footer_columns: vec![
                FooterColumn {
                    title: "Product".into(),
                    links: vec![l("Plans", "/store"), l("Status", "/status")],
                },
                FooterColumn {
                    title: "Support".into(),
                    links: vec![
                        l("Knowledgebase", "/kb"),
                        l("Announcements", "/announcements"),
                        l("Open a ticket", "/tickets"),
                    ],
                },
                FooterColumn {
                    title: "Company".into(),
                    links: vec![
                        l("About us", "/page/about"),
                        l("Announcements", "/announcements"),
                    ],
                },
                FooterColumn {
                    title: "Legal".into(),
                    links: vec![
                        l("Terms of Service", "/page/terms"),
                        l("Privacy Policy", "/page/privacy"),
                    ],
                },
                FooterColumn {
                    title: "Contact".into(),
                    links: vec![l("Contact us", "/tickets"), l("Network status", "/status")],
                },
            ],
            footer_text: String::new(),
            social: Vec::new(),
            copyright: String::new(),
            show_store_in_nav: true,
            client_dashboard_widgets: vec![
                "services".into(),
                "invoices".into(),
                "tickets".into(),
                "announcements".into(),
                "kb".into(),
            ],
        }
    }
}

fn href_ok(h: &str) -> bool {
    h.len() <= 500
        && (h.starts_with('/') && !h.starts_with("//")
            || h.starts_with("https://")
            || h.starts_with("http://")
            || h.starts_with("mailto:")
            || h.starts_with('#'))
        && !h.chars().any(|c| c.is_control() || c == ' ')
}

fn text_ok(s: &str, max: usize) -> bool {
    s.chars().count() <= max
}

pub fn validate_config(c: &SiteConfig) -> AppResult<()> {
    let bad = AppError::Validation("error.site_config");
    if c.site_name.trim().is_empty()
        || !text_ok(&c.site_name, 80)
        || !text_ok(&c.tagline, 200)
        || !text_ok(&c.footer_text, 1000)
    {
        return Err(bad);
    }
    if !(c.logo_url.is_empty() || href_ok(&c.logo_url)) {
        return Err(AppError::Validation("error.site_link"));
    }
    let hex = |s: &str| {
        s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
    };
    if !(c.primary_color.is_empty() || hex(&c.primary_color)) {
        return Err(AppError::Validation("error.site_color"));
    }
    if !FONTS.contains(&c.font.as_str()) || !MODES.contains(&c.default_mode.as_str()) {
        return Err(bad);
    }
    if !slug_ok(&c.homepage)
        || c.nav.len() > 20
        || c.footer_columns.len() > 6
        || c.client_dashboard_widgets.len() > 10
    {
        return Err(bad);
    }
    let links = c
        .nav
        .iter()
        .chain(c.footer_columns.iter().flat_map(|f| f.links.iter()));
    for l in links {
        if l.label.trim().is_empty() || !text_ok(&l.label, 60) || !href_ok(&l.href) {
            return Err(AppError::Validation("error.site_link"));
        }
    }
    if c.social.len() > 12 || !text_ok(&c.copyright, 200) {
        return Err(bad);
    }
    if c.social
        .iter()
        .any(|x| !SOCIAL_NETWORKS.contains(&x.network.as_str()) || !href_ok(&x.href))
    {
        return Err(AppError::Validation("error.site_link"));
    }
    if c.footer_columns
        .iter()
        .any(|f| f.links.len() > 12 || !text_ok(&f.title, 60))
    {
        return Err(bad);
    }
    if c.client_dashboard_widgets.iter().any(|w| {
        ![
            "services",
            "invoices",
            "tickets",
            "announcements",
            "kb",
            "domains",
        ]
        .contains(&w.as_str())
    }) {
        return Err(bad);
    }
    Ok(())
}

pub fn slug_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
}

/// Blocks: `[{type, props}]`, known types only, bounded size, links checked.
pub fn validate_blocks(v: &Value) -> AppResult<()> {
    let bad = AppError::Validation("error.site_blocks");
    let arr = v
        .as_array()
        .ok_or(AppError::Validation("error.site_blocks"))?;
    if arr.len() > 60 || v.to_string().len() > 200_000 {
        return Err(bad);
    }
    for b in arr {
        let o = b
            .as_object()
            .ok_or(AppError::Validation("error.site_blocks"))?;
        let t = o.get("type").and_then(Value::as_str).unwrap_or_default();
        if !BLOCK_TYPES.contains(&t)
            || !o.get("props").is_some_and(Value::is_object)
            || o.keys().any(|k| k != "type" && k != "props" && k != "id")
        {
            return Err(AppError::Validation("error.site_blocks"));
        }
        check_links(&o["props"])?;
    }
    Ok(())
}

fn check_links(v: &Value) -> AppResult<()> {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if (k == "href" || k.ends_with("_href") || k == "src")
                    && x.as_str().is_some_and(|h| !h.is_empty() && !href_ok(h))
                {
                    return Err(AppError::Validation("error.site_link"));
                }
                check_links(x)?;
            }
        }
        Value::Array(a) => a.iter().try_for_each(check_links)?,
        _ => {}
    }
    Ok(())
}

pub async fn load_config(s: &AppState) -> AppResult<SiteConfig> {
    Ok(pelty_database::settings::get(&s.pool, CONFIG_KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

#[utoipa::path(get, path = "/api/v1/public/site", responses((status = 200)))]
pub async fn public_site(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let c = load_config(&s).await?;
    let pages: Vec<Value> = site::list(&s.pool, true)
        .await?
        .into_iter()
        .filter(|p| p.show_in_nav)
        .map(|p| json!({ "slug": p.slug, "title": p.title }))
        .collect();
    Ok(Json(json!({ "config": c, "nav_pages": pages })))
}

#[utoipa::path(get, path = "/api/v1/public/site/pages/{slug}", responses((status = 200), (status = 404)))]
pub async fn public_page(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let p = site::by_slug(&s.pool, &slug)
        .await?
        .filter(|p| p.published)
        .ok_or(AppError::NotFound)?;
    Ok(Json(
        json!({ "slug": p.slug, "title": p.title, "description": p.description, "blocks": p.blocks }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/site", responses((status = 200), (status = 403)))]
pub async fn admin_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "config": load_config(&s).await?, "fonts": FONTS, "modes": MODES, "block_types": BLOCK_TYPES }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/site", responses((status = 200), (status = 422)))]
pub async fn admin_put(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(c): Json<SiteConfig>,
) -> ApiResult<Json<Value>> {
    validate_config(&c)?;
    pelty_database::settings::set(
        &s.pool,
        CONFIG_KEY,
        serde_json::to_value(&c).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "site.config_updated",
        None,
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "config": c })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PageBody {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "empty_arr")]
    pub blocks: Value,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub show_in_nav: bool,
    #[serde(default)]
    pub sort_order: i32,
}
fn empty_arr() -> Value {
    json!([])
}

fn input(b: &PageBody) -> AppResult<PageInput<'_>> {
    if !slug_ok(&b.slug)
        || b.title.trim().is_empty()
        || !text_ok(&b.title, 200)
        || !text_ok(&b.description, 500)
    {
        return Err(AppError::Validation("error.site_page"));
    }
    validate_blocks(&b.blocks)?;
    Ok(PageInput {
        slug: &b.slug,
        title: b.title.trim(),
        description: &b.description,
        blocks: &b.blocks,
        published: b.published,
        show_in_nav: b.show_in_nav,
        sort_order: b.sort_order,
    })
}

#[utoipa::path(get, path = "/api/v1/admin/site/pages", responses((status = 200), (status = 403)))]
pub async fn admin_pages(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ContentManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": site::list(&s.pool, false).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/site/pages/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_page(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ContentManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<site::Page>> {
    Ok(Json(
        site::get(&s.pool, id).await?.ok_or(AppError::NotFound)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/site/pages", responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ContentManage>,
    Json(b): Json<PageBody>,
) -> ApiResult<Json<site::Page>> {
    let p = site::create(&s.pool, &input(&b)?).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "site.page_created",
        Some(&p.id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": p.slug }),
    )
    .await?;
    Ok(Json(p))
}

#[utoipa::path(put, path = "/api/v1/admin/site/pages/{id}", responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ContentManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<PageBody>,
) -> ApiResult<Json<site::Page>> {
    let p = site::update(&s.pool, id, &input(&b)?)
        .await?
        .ok_or(AppError::NotFound)?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "site.page_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": p.slug }),
    )
    .await?;
    Ok(Json(p))
}

#[utoipa::path(delete, path = "/api/v1/admin/site/pages/{id}", responses((status = 204), (status = 404)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ContentManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !site::delete(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "site.page_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_validation() {
        assert!(validate_config(&SiteConfig::default()).is_ok());
        let mut c = SiteConfig {
            primary_color: "#59b5d3".into(),
            ..Default::default()
        };
        assert!(validate_config(&c).is_ok());
        c.primary_color = "red".into();
        assert!(validate_config(&c).is_err());
        let mut c = SiteConfig::default();
        c.nav.push(Link {
            label: "x".into(),
            href: "javascript:alert(1)".into(),
        });
        assert!(validate_config(&c).is_err());
        let c = SiteConfig {
            font: "comic".into(),
            ..Default::default()
        };
        assert!(validate_config(&c).is_err());
        assert!(!href_ok("//evil.example"));
    }

    #[test]
    fn block_validation() {
        assert!(validate_blocks(
            &json!([{"type": "hero", "props": {"title": "x", "primary_href": "/store"}}])
        )
        .is_ok());
        assert!(validate_blocks(&json!([{"type": "script", "props": {}}])).is_err());
        assert!(validate_blocks(&json!([{"type": "hero"}])).is_err());
        assert!(
            validate_blocks(&json!([{"type": "cta", "props": {"href": "javascript:x"}}])).is_err()
        );
        assert!(
            validate_blocks(&json!([{"type": "image", "props": {"src": "data:text/html,x"}}]))
                .is_err()
        );
        assert!(validate_blocks(&json!({})).is_err());
    }
}
