//! Operator overrides on top of the active theme package: token edits and
//! scoped custom CSS. Stored in `settings` under [`KEY`].
use crate::settings;
use pelty_core::AppResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeMap;

pub const KEY: &str = "theme.customization";
pub const MAX_CSS: usize = 32_768;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ThemeCustomization {
    #[serde(default)]
    pub tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub custom_css: String,
    #[serde(default)]
    pub color_scheme: Option<String>,
}

pub async fn load(pool: &PgPool) -> AppResult<ThemeCustomization> {
    let v = settings::get(pool, KEY).await?;
    Ok(match v {
        Some(v) => serde_json::from_value(v).unwrap_or_default(),
        None => ThemeCustomization::default(),
    })
}

pub async fn save(pool: &PgPool, c: &ThemeCustomization) -> AppResult<()> {
    settings::set(pool, KEY, serde_json::to_value(c).unwrap_or(Value::Null)).await
}
