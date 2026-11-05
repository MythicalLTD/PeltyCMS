//! First-run setup wizard (WHMCS Setup Wizard / "Getting started"): a
//! checklist computed from live configuration, plus a dismiss flag.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const KEY: &str = "setup.wizard";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub dismissed: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Step {
    pub id: &'static str,
    pub done: bool,
    /// Optional steps don't block "complete".
    pub optional: bool,
    /// Admin route that configures it.
    pub href: &'static str,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Wizard {
    pub steps: Vec<Step>,
    pub complete: bool,
    pub dismissed: bool,
    /// Show the wizard: not dismissed and required steps open.
    pub show: bool,
}

pub async fn status(pool: &PgPool) -> AppResult<Wizard> {
    let g = crate::general_settings::load(pool).await?;
    let (smtp, gateway, product, tax, tld, server, currency): (bool, bool, bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT COALESCE((SELECT value->>'host' <> '' FROM system_settings WHERE key = 'mail.smtp'), false),
                EXISTS(SELECT 1 FROM payment_gateways WHERE enabled),
                EXISTS(SELECT 1 FROM products),
                EXISTS(SELECT 1 FROM tax_rules),
                EXISTS(SELECT 1 FROM tld_pricing),
                EXISTS(SELECT 1 FROM servers),
                EXISTS(SELECT 1 FROM currencies WHERE enabled)",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let st: State = crate::settings::get(pool, KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let lic = crate::license::load(pool).await?;
    let license_ok = lic.accepted && lic.version == crate::license::VERSION;
    let steps = vec![
        Step { id: "license", done: license_ok, optional: false, href: "/admin/getting-started" },
        Step { id: "company", done: !g.general.company_name.trim().is_empty() && !g.general.email.trim().is_empty(), optional: false, href: "/admin/general-settings" },
        Step { id: "currency", done: currency, optional: false, href: "/admin/currencies" },
        Step { id: "email", done: smtp, optional: false, href: "/admin/mail-settings" },
        Step { id: "gateway", done: gateway, optional: false, href: "/admin/gateways" },
        Step { id: "product", done: product, optional: false, href: "/admin/products" },
        Step { id: "tax", done: tax, optional: true, href: "/admin/tax" },
        Step { id: "domains", done: tld, optional: true, href: "/admin/domains" },
        Step { id: "server", done: server, optional: true, href: "/admin/servers" },
    ];
    let complete = steps.iter().all(|s| s.done || s.optional);
    Ok(Wizard { show: !st.dismissed && !complete, complete, dismissed: st.dismissed, steps })
}

pub async fn set_dismissed(pool: &PgPool, dismissed: bool) -> AppResult<()> {
    crate::settings::set(pool, KEY, serde_json::to_value(State { dismissed }).map_err(|e| AppError::Internal(e.to_string()))?).await
}
