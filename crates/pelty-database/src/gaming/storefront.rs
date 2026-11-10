//! Gaming storefront (module `gaming_storefront`): hero banner, accent,
//! package layout and Tebex/CraftingStore-style sidebar widgets on the game
//! store (`store_mode: "game"`). Settings live in `feature_modules.settings`.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "gaming_storefront";
pub const ACCENTS: &[&str] = &["default", "emerald", "violet", "amber", "rose", "sky"];
/// Public-site accent presets (store mode). `panel` (the default) matches the
/// PeltyCMS panel and the site brand; the game presets only change the accent
/// colour. `default` is the old name of `panel` and still accepted.
pub const THEMES: &[&str] = &["panel", "default", "minecraft", "fivem", "rust", "dark"];
pub const PERIODS: &[&str] = &["day", "week", "month", "year", "all"];
const MAX_WIDGETS: usize = 12;

/// Public display name: first game account, else "First L.", else "Anonymous".
macro_rules! public_name {
    () => {
        "COALESCE((SELECT a.value FROM customer_game_accounts a WHERE a.customer_id = c.id AND a.field = 'username' ORDER BY a.created_at LIMIT 1),
                  NULLIF(trim(c.first_name || ' ' || left(c.last_name, 1) || CASE WHEN c.last_name <> '' THEN '.' ELSE '' END), ''),
                  'Anonymous')"
    };
}

/// One sidebar widget, in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[schema(as = StorefrontWidget)]
pub enum Widget {
    /// Plain text box (rules, about, contact). Rendered as text, never HTML.
    Text { title: String, body: String },
    /// Highest spender in a period.
    TopCustomer {
        period: String,
        #[serde(default)]
        show_amount: bool,
    },
    /// Latest paid purchases (product names only).
    RecentPayments { limit: i64 },
    /// One highlighted package.
    FeaturedPackage { product_id: Uuid },
    /// Live status from `server_community` (all servers, or one).
    ServerStatus {
        #[serde(default)]
        server_id: Option<Uuid>,
    },
    /// The `server_community` monthly goal.
    Goal {},
    /// Discord invite button.
    Discord { invite_url: String, label: String },
}

/// Storefront settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = StorefrontConfig)]
pub struct Config {
    #[serde(default)]
    pub hero_title: String,
    #[serde(default)]
    pub hero_subtitle: String,
    /// `https://` URL or a site-relative `/path`; empty for none.
    #[serde(default)]
    pub hero_image: String,
    /// One of [`ACCENTS`].
    #[serde(default = "default_accent")]
    pub accent: String,
    /// `list` (dense rows) or `grid` (cards).
    #[serde(default = "default_layout")]
    pub layout: String,
    /// `left` or `right`.
    #[serde(default = "default_side")]
    pub sidebar: String,
    #[serde(default)]
    pub widgets: Vec<Widget>,
    /// One of [`THEMES`]: accent preset of the public site in store mode.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Server shown in the hero (address, players); `None` = the first enabled one.
    #[serde(default)]
    pub server_id: Option<Uuid>,
    /// Discord invite for the hero and the store nav; empty for none.
    #[serde(default)]
    pub discord_url: String,
    /// Server logo in the hero (`https://` or `/path`); empty uses the site logo.
    #[serde(default)]
    pub logo_url: String,
}

fn default_accent() -> String {
    "default".into()
}
fn default_theme() -> String {
    "panel".into()
}
fn default_layout() -> String {
    "list".into()
}
fn default_side() -> String {
    "right".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hero_title: String::new(),
            hero_subtitle: String::new(),
            hero_image: String::new(),
            accent: default_accent(),
            layout: default_layout(),
            sidebar: default_side(),
            widgets: vec![
                Widget::RecentPayments { limit: 5 },
                Widget::ServerStatus { server_id: None },
            ],
            theme: default_theme(),
            server_id: None,
            discord_url: String::new(),
            logo_url: String::new(),
        }
    }
}

fn image_ok(u: &str) -> bool {
    u.is_empty()
        || (u.len() <= 500
            && (u.starts_with("https://") || (u.starts_with('/') && !u.starts_with("//")))
            && !u.chars().any(|c| c.is_whitespace() || c == '"' || c == '<'))
}

fn discord_ok(u: &str) -> bool {
    u.len() <= 200
        && ["https://discord.gg/", "https://discord.com/invite/"]
            .iter()
            .any(|p| {
                u.strip_prefix(p).is_some_and(|code| {
                    !code.is_empty()
                        && code
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                })
            })
}

impl Config {
    pub fn validate(&self) -> AppResult<()> {
        let bad = Err(AppError::Validation("error.storefront_invalid"));
        let short = |s: &str, n: usize| s.chars().count() <= n;
        if !short(&self.hero_title, 120)
            || !short(&self.hero_subtitle, 300)
            || !image_ok(&self.hero_image)
            || !ACCENTS.contains(&self.accent.as_str())
            || !["list", "grid"].contains(&self.layout.as_str())
            || !["left", "right"].contains(&self.sidebar.as_str())
            || self.widgets.len() > MAX_WIDGETS
            || !THEMES.contains(&self.theme.as_str())
            || !(self.discord_url.is_empty() || discord_ok(&self.discord_url))
            || !image_ok(&self.logo_url)
        {
            return bad;
        }
        for w in &self.widgets {
            let ok = match w {
                Widget::Text { title, body } => {
                    short(title, 80) && short(body, 2000) && !body.trim().is_empty()
                }
                Widget::TopCustomer { period, .. } => PERIODS.contains(&period.as_str()),
                Widget::RecentPayments { limit } => (1..=10).contains(limit),
                Widget::Discord { invite_url, label } => discord_ok(invite_url) && short(label, 60),
                Widget::FeaturedPackage { .. } | Widget::ServerStatus { .. } | Widget::Goal {} => {
                    true
                }
            };
            if !ok {
                return bad;
            }
        }
        Ok(())
    }
}

/// Saved settings; anything unreadable falls back to the defaults.
pub async fn config(pool: &PgPool) -> AppResult<Config> {
    let v = crate::feature_modules::settings(pool, MODULE).await?;
    Ok(parse(v))
}

pub fn parse(v: Value) -> Config {
    if v.as_object().is_none_or(|o| o.is_empty()) {
        return Config::default();
    }
    serde_json::from_value::<Config>(v)
        .ok()
        .filter(|c| c.validate().is_ok())
        .unwrap_or_default()
}

/// Validate and store the settings without touching the enabled flag.
pub async fn save(pool: &PgPool, c: &Config, by: Uuid) -> AppResult<()> {
    c.validate()?;
    sqlx::query(
        "INSERT INTO feature_modules (id, enabled, settings, updated_by) VALUES ($1, false, $2, $3)
         ON CONFLICT (id) DO UPDATE SET settings = $2, updated_by = $3, updated_at = now()",
    )
    .bind(MODULE)
    .bind(serde_json::to_value(c).map_err(|e| AppError::Internal(e.to_string()))?)
    .bind(by)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// The store's default currency (`EUR` if none is marked default).
pub async fn default_currency(pool: &PgPool) -> AppResult<String> {
    let c: Option<String> = sqlx::query_scalar(
        "SELECT code::text FROM currencies WHERE is_default AND enabled ORDER BY code LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(c.unwrap_or_else(|| "EUR".into()))
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = StorefrontTopCustomer)]
pub struct TopCustomer {
    pub name: String,
    #[schema(value_type = String)]
    pub total: Decimal,
}

fn since(period: &str) -> &'static str {
    match period {
        "day" => "date_trunc('day', now())",
        "week" => "date_trunc('week', now())",
        "year" => "date_trunc('year', now())",
        "all" => "'-infinity'::timestamptz",
        _ => "date_trunc('month', now())",
    }
}

/// Highest spender (paid invoices in `currency`) since the start of `period`.
pub async fn top_customer(
    pool: &PgPool,
    period: &str,
    currency: &str,
) -> AppResult<Option<TopCustomer>> {
    let sql = format!(
        concat!(
            "SELECT ",
            public_name!(),
            " AS name, sum(i.total) AS total
               FROM invoices i JOIN customers c ON c.id = i.customer_id
              WHERE i.status = 'paid' AND i.kind = 'invoice' AND i.currency = $1 AND i.total > 0 AND i.paid_at >= {}
              GROUP BY c.id ORDER BY total DESC LIMIT 1"
        ),
        since(period)
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(currency)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = StorefrontRecentPayment)]
pub struct RecentPayment {
    pub name: String,
    /// Product names on the invoice (active products only).
    pub products: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub paid_at: OffsetDateTime,
}

/// Latest paid invoices that bought (order) or renewed (service line) at
/// least one active catalog product.
pub async fn recent_payments(pool: &PgPool, limit: i64) -> AppResult<Vec<RecentPayment>> {
    sqlx::query_as(concat!(
        "SELECT ",
        public_name!(),
        " AS name, p.products, i.paid_at
           FROM invoices i
           JOIN customers c ON c.id = i.customer_id
           JOIN LATERAL (
                SELECT array_agg(DISTINCT pr.name ORDER BY pr.name) AS products
                  FROM products pr
                 WHERE pr.status = 'active'
                   AND (pr.id IN (SELECT oi.product_id FROM order_items oi WHERE oi.order_id = i.order_id)
                        OR pr.id IN (SELECT sv.product_id FROM invoice_items ii JOIN services sv ON sv.id = ii.service_id
                                      WHERE ii.invoice_id = i.id))
           ) p ON p.products IS NOT NULL
          WHERE i.status = 'paid' AND i.kind = 'invoice' AND i.total > 0 AND i.paid_at IS NOT NULL
          ORDER BY i.paid_at DESC LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = StorefrontFeatured)]
pub struct Featured {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub image_url: String,
}

/// An active product for the featured widget.
pub async fn featured(pool: &PgPool, id: Uuid) -> AppResult<Option<Featured>> {
    sqlx::query_as(
        "SELECT id, name, description, image_url FROM products WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_and_lenient_parse() {
        assert_eq!(parse(json!({})), Config::default());
        assert_eq!(parse(json!({"accent": "neon"})), Config::default());
        let c = parse(json!({"accent": "violet", "layout": "grid", "widgets": [{"type": "goal"}]}));
        assert_eq!(c.accent, "violet");
        assert_eq!(c.widgets, vec![Widget::Goal {}]);
    }

    #[test]
    fn theme_server_and_links() {
        let mut c = Config::default();
        assert_eq!(c.theme, "panel");
        c.theme = "fivem".into();
        c.discord_url = "https://discord.gg/rp".into();
        c.logo_url = "/uploads/logo.png".into();
        assert!(c.validate().is_ok());
        c.theme = "neon".into();
        assert!(c.validate().is_err());
        c.theme = "rust".into();
        c.discord_url = "https://evil.example/x".into();
        assert!(c.validate().is_err());
        c.discord_url = String::new();
        c.logo_url = "javascript:x".into();
        assert!(c.validate().is_err());
        // Settings saved before these fields existed still load.
        let old = parse(json!({"accent": "violet"}));
        assert_eq!((old.theme.as_str(), old.server_id), ("panel", None));
    }

    #[test]
    fn validation() {
        let mut c = Config::default();
        assert!(c.validate().is_ok());
        c.hero_image = "javascript:alert(1)".into();
        assert!(c.validate().is_err());
        c.hero_image = "//evil.example/x.png".into();
        assert!(c.validate().is_err());
        c.hero_image = "/uploads/banner.png".into();
        assert!(c.validate().is_ok());
        c.widgets = vec![Widget::Discord {
            invite_url: "https://discord.gg/abc-123".into(),
            label: "Join".into(),
        }];
        assert!(c.validate().is_ok());
        c.widgets = vec![Widget::Discord {
            invite_url: "https://evil.example/discord.gg/x".into(),
            label: "x".into(),
        }];
        assert!(c.validate().is_err());
        c.widgets = vec![Widget::RecentPayments { limit: 50 }];
        assert!(c.validate().is_err());
        c.widgets = vec![Widget::TopCustomer {
            period: "decade".into(),
            show_amount: false,
        }];
        assert!(c.validate().is_err());
        c.widgets = vec![Widget::Text {
            title: "Rules".into(),
            body: " ".into(),
        }];
        assert!(c.validate().is_err());
        assert!(
            serde_json::from_value::<Widget>(json!({"type": "html", "body": "<script>"})).is_err()
        );
    }
}
