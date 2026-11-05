//! Product license status: Free Tier (≤50 active clients), Soft overage notices,
//! and admin acceptance of LICENSE + EULA (stored in `system_settings`).
use crate::db_err;
use crate::settings;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const KEY: &str = "license.acceptance";
/// SAL / EULA document version recorded on accept.
pub const VERSION: &str = "1.0";
/// Production Free Tier: Active Clients at or below this need no paid license.
pub const FREE_ACTIVE_CLIENTS: i64 = 50;
pub const COMMERCIAL_URL: &str = "https://peltycms.com/pricing";
pub const EULA_URL: &str = "https://peltycms.com/eula";
pub const LICENSE_URL: &str = "https://peltycms.com/license";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Acceptance {
    pub accepted: bool,
    pub version: String,
    pub accepted_at: Option<String>,
    pub accepted_by: Option<Uuid>,
    /// Source: `setup` | `admin` | `headless`.
    pub source: String,
    pub overage_notice_dismissed: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Status {
    pub version: &'static str,
    pub free_limit: i64,
    pub active_clients: i64,
    pub over_free_limit: bool,
    /// Always Soft: UI may warn; features are never locked for overage alone.
    pub enforcement: &'static str,
    pub accepted: bool,
    pub accepted_version: String,
    pub show_accept_gate: bool,
    pub show_overage_banner: bool,
    pub commercial_url: &'static str,
    pub eula_url: &'static str,
    pub license_url: &'static str,
    pub summary: LicenseSummary,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LicenseSummary {
    pub free_tier: &'static str,
    pub overage: &'static str,
    pub soft: &'static str,
}

fn summary() -> LicenseSummary {
    LicenseSummary {
        free_tier: "Free in production with 50 or fewer Active Clients (status active, not merged), and free for non-production use.",
        overage: "Above 50 Active Clients you need a Commercial License (COMMERCIAL.md / peltycms.com/pricing).",
        soft: "PeltyCMS does not lock features or stop the server for overage alone. Notices are reminders; the License and EULA still apply.",
    }
}

pub async fn load(pool: &PgPool) -> AppResult<Acceptance> {
    Ok(settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

async fn save(pool: &PgPool, a: &Acceptance) -> AppResult<()> {
    settings::set(
        pool,
        KEY,
        serde_json::to_value(a).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

/// Customers that count toward the Free Tier (active, not merged).
pub async fn count_active_clients(pool: &PgPool) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT count(*)::bigint FROM customers
         WHERE status = 'active' AND merged_into IS NULL",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn status(pool: &PgPool) -> AppResult<Status> {
    let a = load(pool).await?;
    let active_clients = count_active_clients(pool).await?;
    let over = active_clients > FREE_ACTIVE_CLIENTS;
    let accepted = a.accepted && a.version == VERSION;
    Ok(Status {
        version: VERSION,
        free_limit: FREE_ACTIVE_CLIENTS,
        active_clients,
        over_free_limit: over,
        enforcement: "soft",
        accepted,
        accepted_version: a.version.clone(),
        show_accept_gate: !accepted,
        show_overage_banner: over && !a.overage_notice_dismissed,
        commercial_url: COMMERCIAL_URL,
        eula_url: EULA_URL,
        license_url: LICENSE_URL,
        summary: summary(),
    })
}

pub async fn accept(pool: &PgPool, user_id: Option<Uuid>, source: &str) -> AppResult<Status> {
    let now = OffsetDateTime::now_utc();
    let stamp = now
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now.to_string());
    let prev = load(pool).await?;
    save(
        pool,
        &Acceptance {
            accepted: true,
            version: VERSION.into(),
            accepted_at: Some(stamp),
            accepted_by: user_id.or(prev.accepted_by),
            source: source.into(),
            overage_notice_dismissed: prev.overage_notice_dismissed,
        },
    )
    .await?;
    status(pool).await
}

/// After CLI / headless setup: EULA was already accepted via `eula.txt`.
pub async fn accept_from_setup(pool: &PgPool) -> AppResult<()> {
    let a = load(pool).await?;
    if a.accepted && a.version == VERSION {
        return Ok(());
    }
    let _ = accept(pool, None, "setup").await?;
    Ok(())
}

pub async fn set_overage_dismissed(pool: &PgPool, dismissed: bool) -> AppResult<Status> {
    let mut a = load(pool).await?;
    a.overage_notice_dismissed = dismissed;
    save(pool, &a).await?;
    status(pool).await
}
