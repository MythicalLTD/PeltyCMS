//! System cleanup tasks from the WHMCS cron: prune old ticket attachments
//! and keep customer active/inactive status in sync with what they own.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const KEY: &str = "system.housekeeping";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HousekeepingSettings {
    /// Delete attachments of tickets closed this many days ago (0 = keep).
    pub prune_attachments_days: i32,
    /// Mark customers inactive when they own no live service or domain, and
    /// active again when they do (WHMCS "Client Status Update"). Closed
    /// customers are never touched.
    pub client_status_sync: bool,
    /// Probe the TLS certificate of active client domains once a day.
    pub ssl_monitoring: bool,
    /// Delete operational logs (email, module, mail-piping, notification and
    /// plugin deliveries) older than this many days (0 = keep). The audit log
    /// is never pruned here.
    pub log_retention_days: i32,
}

impl HousekeepingSettings {
    pub fn validate(&self) -> Result<(), AppError> {
        if (0..=3650).contains(&self.prune_attachments_days)
            && (self.log_retention_days == 0 || (30..=3650).contains(&self.log_retention_days))
        {
            Ok(())
        } else {
            Err(AppError::Validation("error.validation"))
        }
    }
}

pub async fn get(pool: &PgPool) -> AppResult<HousekeepingSettings> {
    Ok(crate::settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put(pool: &PgPool, s: &HousekeepingSettings) -> AppResult<()> {
    s.validate()?;
    crate::settings::set(
        pool,
        KEY,
        serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub attachments_pruned: u64,
    pub clients_inactivated: u64,
    pub clients_reactivated: u64,
    pub log_rows_pruned: u64,
}

/// Log tables pruned by `log_retention_days`.
const LOG_TABLES: [&str; 5] = [
    "email_log",
    "module_log",
    "ticket_pipe_log",
    "notification_deliveries",
    "plugin_deliveries",
];

const LIVE: &str = "(EXISTS (SELECT 1 FROM services s WHERE s.customer_id = c.id AND s.status IN ('pending','active','suspended'))
     OR EXISTS (SELECT 1 FROM domains d WHERE d.customer_id = c.id AND d.status IN ('pending','active','grace','redemption','transferring')))";

pub async fn run(pool: &PgPool) -> AppResult<Report> {
    let cfg = get(pool).await?;
    let mut r = Report::default();
    if cfg.prune_attachments_days > 0 {
        r.attachments_pruned = sqlx::query(
            "DELETE FROM ticket_attachments a USING tickets t
             WHERE a.ticket_id = t.id AND t.status = 'closed' AND t.closed_at < now() - make_interval(days => $1)",
        )
        .bind(cfg.prune_attachments_days)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    }
    if cfg.client_status_sync {
        r.clients_inactivated = sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE customers c SET status = 'inactive' WHERE c.status = 'active' AND c.merged_into IS NULL AND NOT {LIVE}")))
            .execute(pool).await.map_err(db_err)?.rows_affected();
        r.clients_reactivated = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE customers c SET status = 'active' WHERE c.status = 'inactive' AND {LIVE}"
        )))
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    }
    if cfg.log_retention_days > 0 {
        for t in LOG_TABLES {
            r.log_rows_pruned += sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {t} WHERE created_at < now() - make_interval(days => $1)"
            )))
            .bind(cfg.log_retention_days)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        }
    }
    Ok(r)
}
