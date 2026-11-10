//! Feature modules: optional domain packs an admin toggles per install.
//! Off by default; enabling one checks its `requires` list, disabling one
//! that another enabled module needs is refused.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
pub struct ModuleDef {
    pub id: &'static str,
    /// Domain pack the module belongs to (`gaming`, `digital`, `marketplace`,
    /// `commerce`).
    pub pack: &'static str,
    /// Modules that must be enabled first.
    pub requires: &'static [&'static str],
}

/// Every optional module, in display order.
pub const MODULES: &[ModuleDef] = &[
    ModuleDef {
        id: "game_profiles",
        pack: "gaming",
        requires: &[],
    },
    ModuleDef {
        id: "game_delivery",
        pack: "gaming",
        requires: &["game_profiles"],
    },
    ModuleDef {
        id: "server_community",
        pack: "gaming",
        requires: &["game_profiles"],
    },
    ModuleDef {
        id: "gaming_storefront",
        pack: "gaming",
        requires: &[],
    },
    ModuleDef {
        id: "digital_products",
        pack: "digital",
        requires: &[],
    },
    ModuleDef {
        id: "creator_marketplace",
        pack: "marketplace",
        requires: &["digital_products"],
    },
    ModuleDef {
        id: "product_reviews",
        pack: "marketplace",
        requires: &[],
    },
    ModuleDef {
        id: "timed_sales",
        pack: "commerce",
        requires: &[],
    },
    ModuleDef {
        id: "creator_codes",
        pack: "commerce",
        requires: &[],
    },
    ModuleDef {
        id: "discord_roles",
        pack: "gaming",
        requires: &[],
    },
];

pub fn def(id: &str) -> Option<&'static ModuleDef> {
    MODULES.iter().find(|m| m.id == id)
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ModuleState {
    pub id: String,
    pub pack: String,
    pub requires: Vec<String>,
    /// Enabled modules that require this one.
    pub required_by: Vec<String>,
    pub enabled: bool,
    pub settings: Value,
    #[serde(with = "time::serde::rfc3339::option")]
    pub updated_at: Option<time::OffsetDateTime>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleUpdate {
    pub enabled: bool,
    /// Module-specific settings (object). Omit to keep the current ones.
    #[serde(default)]
    pub settings: Option<Value>,
}

type Row = (String, bool, Value, time::OffsetDateTime);

async fn rows(pool: &PgPool) -> AppResult<Vec<Row>> {
    sqlx::query_as("SELECT id, enabled, settings, updated_at FROM feature_modules")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Ids of the enabled modules (known ids only).
pub async fn enabled(pool: &PgPool) -> AppResult<Vec<String>> {
    let on: Vec<String> =
        sqlx::query_scalar("SELECT id FROM feature_modules WHERE enabled ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    Ok(MODULES
        .iter()
        .filter(|m| on.iter().any(|o| o == m.id))
        .map(|m| m.id.to_string())
        .collect())
}

pub async fn is_enabled(pool: &PgPool, id: &str) -> AppResult<bool> {
    let v: Option<bool> = sqlx::query_scalar("SELECT enabled FROM feature_modules WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(v.unwrap_or(false) && def(id).is_some())
}

/// Settings object of a module (`{}` when never saved).
pub async fn settings(pool: &PgPool, id: &str) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar("SELECT settings FROM feature_modules WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(v.unwrap_or_else(|| json!({})))
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<ModuleState>> {
    let rows = rows(pool).await?;
    let on = |id: &str| rows.iter().any(|r| r.0 == id && r.1);
    Ok(MODULES
        .iter()
        .map(|m| {
            let r = rows.iter().find(|r| r.0 == m.id);
            ModuleState {
                id: m.id.into(),
                pack: m.pack.into(),
                requires: m.requires.iter().map(|s| s.to_string()).collect(),
                required_by: MODULES
                    .iter()
                    .filter(|o| o.requires.contains(&m.id) && on(o.id))
                    .map(|o| o.id.to_string())
                    .collect(),
                enabled: r.is_some_and(|r| r.1),
                settings: r.map(|r| r.2.clone()).unwrap_or_else(|| json!({})),
                updated_at: r.map(|r| r.3),
            }
        })
        .collect())
}

/// Dependency check for a toggle given the currently enabled set.
pub fn check_toggle(id: &str, enable: bool, on: &[String]) -> AppResult<()> {
    let m = def(id).ok_or(AppError::NotFound)?;
    if enable {
        if m.requires.iter().any(|r| !on.iter().any(|o| o == r)) {
            return Err(AppError::Conflict("error.module_requires"));
        }
    } else if MODULES
        .iter()
        .any(|o| o.requires.contains(&id) && on.iter().any(|x| x == o.id))
    {
        return Err(AppError::Conflict("error.module_required_by"));
    }
    Ok(())
}

pub async fn set(pool: &PgPool, id: &str, u: &ModuleUpdate, by: Uuid) -> AppResult<ModuleState> {
    if let Some(s) = &u.settings {
        if !s.is_object() || s.to_string().len() > 32_000 {
            return Err(AppError::Validation("error.validation"));
        }
    }
    // Serialize toggles so two concurrent changes cannot break a dependency.
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('feature_modules'))")
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let on: Vec<String> = sqlx::query_scalar("SELECT id FROM feature_modules WHERE enabled")
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
    check_toggle(id, u.enabled, &on)?;
    sqlx::query(
        "INSERT INTO feature_modules (id, enabled, settings, updated_by) VALUES ($1,$2,COALESCE($3,'{}'::jsonb),$4)
         ON CONFLICT (id) DO UPDATE SET enabled=$2, settings=COALESCE($3, feature_modules.settings), updated_by=$4, updated_at=now()",
    )
    .bind(id)
    .bind(u.enabled)
    .bind(&u.settings)
    .bind(by)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    list(pool)
        .await?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent() {
        let mut seen = std::collections::HashSet::new();
        for m in MODULES {
            assert!(seen.insert(m.id), "duplicate {}", m.id);
            for r in m.requires {
                assert!(def(r).is_some(), "{} requires unknown {r}", m.id);
            }
        }
    }

    #[test]
    fn toggles_respect_dependencies() {
        let none: Vec<String> = vec![];
        assert!(check_toggle("game_profiles", true, &none).is_ok());
        assert!(matches!(
            check_toggle("game_delivery", true, &none),
            Err(AppError::Conflict("error.module_requires"))
        ));
        let on = vec!["game_profiles".to_string(), "game_delivery".to_string()];
        assert!(check_toggle("game_delivery", true, &on).is_ok());
        assert!(matches!(
            check_toggle("game_profiles", false, &on),
            Err(AppError::Conflict("error.module_required_by"))
        ));
        assert!(check_toggle("game_delivery", false, &on).is_ok());
        assert!(matches!(
            check_toggle("nope", true, &none),
            Err(AppError::NotFound)
        ));
    }
}
