//! WHMCS "General Settings": general, system URLs, maintenance mode,
//! localisation, ordering (security lives with auth settings, social links
//! with the website footer), plus admin-only custom CSS/JS.
//! Stored as JSON in `settings`; every section has serde defaults so older
//! rows keep loading after new fields are added.
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const KEY: &str = "general_settings";
pub const CODE_KEY: &str = "custom_code";
pub const DATE_FORMATS: [&str; 4] = ["DD/MM/YYYY", "MM/DD/YYYY", "YYYY-MM-DD", "DD.MM.YYYY"];
pub const ORDER_FORMS: [&str; 3] = ["standard", "compact", "cards"];

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(default)]
pub struct General {
    pub company_name: String,
    pub email: String,
    pub domain: String,
    pub logo_url: String,
    pub pay_to_text: String,
    pub records_per_page: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(default)]
pub struct SystemUrls {
    pub system_url: String,
    pub terms_url: String,
    pub privacy_url: String,
    pub support_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(default)]
pub struct Maintenance {
    pub enabled: bool,
    pub message: String,
    pub redirect_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Localisation {
    pub default_language: String,
    pub allow_language_choice: bool,
    pub date_format: String,
    pub client_date_format: String,
    pub timezone: String,
    pub default_country: String,
}

impl Default for Localisation {
    fn default() -> Self {
        Self { default_language: "en".into(), allow_language_choice: true, date_format: "DD/MM/YYYY".into(), client_date_format: "DD/MM/YYYY".into(), timezone: "UTC".into(), default_country: "US".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Ordering {
    pub default_order_form: String,
    pub tos_required: bool,
    pub allow_domain_only: bool,
    pub show_cart_after_add: bool,
    pub grace_days: u32,
    pub enable_promo_field: bool,
}

impl Default for Ordering {
    fn default() -> Self {
        Self { default_order_form: "standard".into(), tos_required: true, allow_domain_only: true, show_cart_after_add: true, grace_days: 0, enable_promo_field: true }
    }
}

/// Hard ceiling for any configured attachment limit (the request body cap).
pub const ATTACHMENT_HARD_MAX_MB: u32 = 50;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Support {
    /// Max size per ticket attachment, in MiB (1..=50).
    pub attachment_max_mb: u32,
    /// Max files per ticket message (1..=20).
    pub attachments_per_message: u32,
    /// Show the public live-chat widget.
    pub live_chat: bool,
}

impl Default for Support {
    fn default() -> Self {
        Self { attachment_max_mb: 5, attachments_per_message: 5, live_chat: false }
    }
}

impl Support {
    pub fn max_bytes(&self) -> usize {
        self.attachment_max_mb.clamp(1, ATTACHMENT_HARD_MAX_MB) as usize * 1024 * 1024
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(default)]
pub struct GeneralSettings {
    pub general: General,
    pub system_urls: SystemUrls,
    pub maintenance: Maintenance,
    pub localisation: Localisation,
    pub ordering: Ordering,
    pub support: Support,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(default)]
pub struct CustomCode {
    pub css: String,
    pub head_js: String,
    pub footer_js: String,
}

fn url_ok(u: &str) -> bool {
    u.is_empty() || (u.starts_with("https://") && u.len() <= 300 && !u.contains(char::is_whitespace))
}

fn email_ok(e: &str) -> bool {
    e.is_empty() || (e.len() <= 254 && e.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.')))
}

pub fn validate(s: &GeneralSettings) -> AppResult<()> {
    let g = &s.general;
    let u = &s.system_urls;
    let l = &s.localisation;
    let ok = g.company_name.len() <= 200 && g.pay_to_text.len() <= 2000 && email_ok(&g.email) && url_ok(&g.domain) && url_ok(&g.logo_url)
        && (g.records_per_page == 0 || (10..=500).contains(&g.records_per_page))
        && [&u.system_url, &u.terms_url, &u.privacy_url, &u.support_url].iter().all(|x| url_ok(x))
        && s.maintenance.message.len() <= 2000 && url_ok(&s.maintenance.redirect_url)
        && ["en", "ro"].contains(&l.default_language.as_str())
        && DATE_FORMATS.contains(&l.date_format.as_str()) && DATE_FORMATS.contains(&l.client_date_format.as_str())
        && !l.timezone.is_empty() && l.timezone.len() <= 64 && l.timezone.chars().all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c))
        && l.default_country.len() == 2 && l.default_country.chars().all(|c| c.is_ascii_uppercase())
        && ORDER_FORMS.contains(&s.ordering.default_order_form.as_str()) && s.ordering.grace_days <= 365
        && (1..=ATTACHMENT_HARD_MAX_MB).contains(&s.support.attachment_max_mb) && (1..=20).contains(&s.support.attachments_per_message);
    if ok { Ok(()) } else { Err(AppError::Validation("error.validation")) }
}

pub fn validate_code(c: &CustomCode) -> AppResult<()> {
    if c.css.len() > 100_000 || c.head_js.len() > 100_000 || c.footer_js.len() > 100_000 || c.css.to_ascii_lowercase().contains("</style") {
        return Err(AppError::Validation("error.validation"));
    }
    Ok(())
}

pub async fn load(pool: &PgPool) -> AppResult<GeneralSettings> {
    Ok(crate::settings::get(pool, KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn save(pool: &PgPool, s: &GeneralSettings) -> AppResult<()> {
    validate(s)?;
    crate::settings::set(pool, KEY, serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?).await
}

pub async fn load_code(pool: &PgPool) -> AppResult<CustomCode> {
    Ok(crate::settings::get(pool, CODE_KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn save_code(pool: &PgPool, c: &CustomCode) -> AppResult<()> {
    validate_code(c)?;
    crate::settings::set(pool, CODE_KEY, serde_json::to_value(c).map_err(|e| AppError::Internal(e.to_string()))?).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_bad_values_fail() {
        let d = GeneralSettings::default();
        assert!(validate(&d).is_ok());
        let mut s = d.clone();
        s.system_urls.system_url = "http://insecure".into();
        assert!(validate(&s).is_err());
        let mut s = d.clone();
        s.localisation.date_format = "YY".into();
        assert!(validate(&s).is_err());
        let mut s = d;
        s.maintenance.redirect_url = "javascript:alert(1)".into();
        assert!(validate(&s).is_err());
        assert!(validate_code(&CustomCode { css: "a{}</style><script>".into(), ..Default::default() }).is_err());
    }

    #[test]
    fn old_rows_load_with_defaults() {
        let s: GeneralSettings = serde_json::from_value(serde_json::json!({ "maintenance": { "enabled": true } })).unwrap();
        assert!(s.maintenance.enabled);
        assert_eq!(s.localisation.date_format, "DD/MM/YYYY");
        assert_eq!(s.support.max_bytes(), 5 * 1024 * 1024);
        let mut s = s;
        s.support.attachment_max_mb = 51;
        assert!(validate(&s).is_err());
    }
}
