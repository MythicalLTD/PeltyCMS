//! Invoice branding shown on PDF invoices.
use crate::settings;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const KEY: &str = "billing.invoice_branding";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct InvoiceBranding {
    /// Seller name; empty = the site name.
    pub company_name: String,
    /// Seller address, one line per row.
    pub address: String,
    /// Seller tax / VAT id.
    pub tax_id: String,
    /// Small print at the bottom of each page (bank details, terms…).
    pub footer: String,
    /// Accent colour as `#rrggbb`; empty = neutral.
    pub accent: String,
}

impl InvoiceBranding {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        for (v, max) in [(&mut self.company_name, 200), (&mut self.tax_id, 100), (&mut self.address, 1000), (&mut self.footer, 1000)] {
            *v = v.trim().to_string();
            if v.chars().count() > max {
                return Err("error.field_too_long");
            }
        }
        self.accent = self.accent.trim().to_ascii_lowercase();
        if !self.accent.is_empty() && self.rgb().is_none() {
            return Err("error.invalid_color");
        }
        Ok(self)
    }

    /// Accent as 0..1 RGB.
    pub fn rgb(&self) -> Option<(f32, f32, f32)> {
        let h = self.accent.strip_prefix('#')?;
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| f32::from(v) / 255.0);
        Some((c(0)?, c(2)?, c(4)?))
    }
}

pub async fn get(pool: &PgPool) -> AppResult<InvoiceBranding> {
    Ok(settings::get(pool, KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn put(pool: &PgPool, b: &InvoiceBranding) -> AppResult<()> {
    settings::set(pool, KEY, serde_json::to_value(b).map_err(|e| AppError::Internal(e.to_string()))?).await
}

/// Display exponent of a currency (2 if unknown).
pub async fn exponent(pool: &PgPool, currency: &str) -> AppResult<u32> {
    let e: Option<i16> = sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1")
        .bind(currency)
        .fetch_optional(pool)
        .await
        .map_err(crate::db_err)?;
    Ok(e.map_or(2, |e| e.max(0) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accent_validation() {
        let b = InvoiceBranding { accent: " #1E40AF ".into(), ..Default::default() }.normalized().unwrap();
        assert_eq!(b.accent, "#1e40af");
        assert!(b.rgb().is_some());
        assert!(InvoiceBranding { accent: "blue".into(), ..Default::default() }.normalized().is_err());
        assert!(InvoiceBranding { accent: "#12345g".into(), ..Default::default() }.normalized().is_err());
        assert!(InvoiceBranding { company_name: "x".repeat(201), ..Default::default() }.normalized().is_err());
    }
}
