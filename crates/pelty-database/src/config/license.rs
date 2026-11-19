//! Install notice acceptance: the admin confirms once per deployment that
//! they read the GNU AGPL (`LICENSE`) and the notice in `EULA.md`. Stored in
//! `system_settings`. PeltyCMS has no client cap and no feature lock; paid
//! extras live in the optional Premium module (`premium`).
use crate::settings;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const KEY: &str = "license.acceptance";
/// Notice version recorded on accept (2.0: GNU AGPL v3 or later).
pub const VERSION: &str = "2.0";
pub const SPDX: &str = "AGPL-3.0-or-later";
pub const COMMERCIAL_URL: &str = "https://peltycms.com/pricing";
pub const EULA_URL: &str = "https://peltycms.com/eula";
pub const LICENSE_URL: &str = "https://www.gnu.org/licenses/agpl-3.0.html";
pub const SOURCE_URL: &str = "https://github.com/MythicalLTD/PeltyCMS";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Acceptance {
    pub accepted: bool,
    pub version: String,
    pub accepted_at: Option<String>,
    pub accepted_by: Option<Uuid>,
    /// Source: `setup` | `admin` | `headless`.
    pub source: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Status {
    pub version: &'static str,
    /// SPDX id of the PeltyCMS license.
    pub license: &'static str,
    pub accepted: bool,
    pub accepted_version: String,
    pub show_accept_gate: bool,
    pub commercial_url: &'static str,
    pub eula_url: &'static str,
    pub license_url: &'static str,
    pub source_url: &'static str,
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

pub async fn status(pool: &PgPool) -> AppResult<Status> {
    let a = load(pool).await?;
    let accepted = a.accepted && a.version == VERSION;
    Ok(Status {
        version: VERSION,
        license: SPDX,
        accepted,
        accepted_version: a.version.clone(),
        show_accept_gate: !accepted,
        commercial_url: COMMERCIAL_URL,
        eula_url: EULA_URL,
        license_url: LICENSE_URL,
        source_url: SOURCE_URL,
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
