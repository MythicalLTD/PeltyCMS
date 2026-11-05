//! EU VAT number verification against VIES (REST API) through the swappable
//! HTTP transport. The result is stored on the customer and, when the tax
//! setting `require_vies` is on, gates the reverse charge.
use crate::AppState;
use pelty_core::{AppError, AppResult};
use pelty_payments::HttpRequestSpec;
use serde::Serialize;

pub const DEFAULT_BASE: &str = "https://ec.europa.eu/taxation_customs/vies/rest-api";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VatCheck {
    pub vat_number: String,
    pub valid: bool,
    pub name: String,
}

/// Split `DE123456789` into (`DE`, `123456789`); Greece uses `EL` in VIES.
pub fn split(vat: &str) -> Option<(String, String)> {
    let v: String = vat.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '.').collect::<String>().to_ascii_uppercase();
    if v.len() < 4 || v.len() > 15 || !v.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let (cc, num) = v.split_at(2);
    if !cc.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let cc = if cc == "GR" { "EL" } else { cc };
    Some((cc.to_string(), num.to_string()))
}

pub async fn check(s: &AppState, vat: &str) -> AppResult<VatCheck> {
    let (cc, num) = split(vat).ok_or(AppError::Validation("error.invalid_vat"))?;
    if !pelty_billing::vat_format::is_valid_format(&cc, &num) {
        return Err(AppError::Validation("error.invalid_vat"));
    }
    // EU VIES VAT check API base URL (default: the public EU endpoint).
    let base = std::env::var("PELTY_VIES_URL").unwrap_or_else(|_| DEFAULT_BASE.into());
    let url = format!("{}/ms/{cc}/vat/{num}", base.trim_end_matches('/'));
    let (status, body) = s
        .http()
        .send(HttpRequestSpec { method: "GET", url, headers: vec![("accept".into(), "application/json".into())], body: String::new() })
        .await
        .map_err(|_| AppError::Conflict("error.vies_unavailable"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| AppError::Conflict("error.vies_unavailable"))?;
    let valid = match v.get("isValid").or_else(|| v.get("valid")).and_then(|x| x.as_bool()) {
        Some(b) if (200..300).contains(&status) => b,
        _ => return Err(AppError::Conflict("error.vies_unavailable")),
    };
    let name = v["name"].as_str().filter(|n| *n != "---").unwrap_or("").chars().take(300).collect();
    Ok(VatCheck { vat_number: format!("{}{num}", if cc == "EL" { "EL" } else { &cc }), valid, name })
}

/// Verify a customer's stored VAT number and persist the outcome.
pub async fn verify_customer(s: &AppState, customer: uuid::Uuid) -> AppResult<VatCheck> {
    let vat = pelty_database::tax::vat_number_of(&s.pool, customer).await?;
    let vat = vat.ok_or(AppError::NotFound)?;
    if vat.is_empty() {
        return Err(AppError::Validation("error.invalid_vat"));
    }
    let r = check(s, &vat).await?;
    pelty_database::tax::set_vat_result(&s.pool, customer, &vat, r.valid, &r.name).await?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits() {
        assert_eq!(split("de 123-456-789"), Some(("DE".into(), "123456789".into())));
        assert_eq!(split("GR123456789"), Some(("EL".into(), "123456789".into())));
        assert_eq!(split("1234"), None);
        assert_eq!(split("DE"), None);
    }
}
