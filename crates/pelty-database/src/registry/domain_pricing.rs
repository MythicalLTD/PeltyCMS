//! Registrar TLD cost sync with margin rules, premium-name pricing and
//! per-TLD additional order fields. All money is exact `Decimal`.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{FromRow, PgPool};
use std::str::FromStr;
use uuid::Uuid;

// ----- margin rules -----

#[derive(Debug, Clone, PartialEq, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct MarginRule {
    /// TLD without dot, or `*` for the default rule.
    pub tld: String,
    #[schema(value_type = String)]
    pub percent: Decimal,
    #[serde(default)]
    #[schema(value_type = String)]
    pub fixed: Decimal,
    #[serde(default)]
    #[schema(value_type = String)]
    pub min_margin: Decimal,
    /// `cent` (half-up to 0.01), `whole` (up to 1.00) or `ninety_nine` (up to x.99).
    pub rounding: String,
}

impl MarginRule {
    pub fn validate(&self) -> AppResult<()> {
        let tld_ok = self.tld == "*" || (!self.tld.is_empty() && self.tld.len() <= 63 && self.tld.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'));
        let neg = self.percent.is_sign_negative() || self.fixed.is_sign_negative() || self.min_margin.is_sign_negative();
        if !tld_ok || neg || self.percent > Decimal::from(1000) || !["cent", "whole", "ninety_nine"].contains(&self.rounding.as_str()) {
            return Err(AppError::Validation("error.margin_rule_invalid"));
        }
        Ok(())
    }

    /// Sell price for a cost price: cost × (1 + percent/100) + fixed, at
    /// least cost + min_margin, then rounded up per `rounding` (never below
    /// the computed minimum, except `cent`, which is half-up to 0.01).
    pub fn sell(&self, cost: Decimal) -> Decimal {
        let raw = cost * (Decimal::ONE + self.percent / Decimal::from(100)) + self.fixed;
        let raw = raw.max(cost + self.min_margin);
        match self.rounding.as_str() {
            "whole" => raw.ceil(),
            "ninety_nine" => {
                let c = (raw + Decimal::new(1, 2)).ceil() - Decimal::new(1, 2);
                c.round_dp(2)
            }
            _ => raw.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero),
        }
    }
}

pub async fn margin_rules(pool: &PgPool) -> AppResult<Vec<MarginRule>> {
    sqlx::query_as("SELECT tld, percent, fixed, min_margin, rounding FROM tld_margin_rules ORDER BY (tld = '*') DESC, tld").fetch_all(pool).await.map_err(db_err)
}

/// Replace the whole rule set.
pub async fn set_margin_rules(pool: &PgPool, rules: &[MarginRule]) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    for r in rules {
        r.validate()?;
        if !seen.insert(r.tld.clone()) {
            return Err(AppError::Validation("error.margin_rule_invalid"));
        }
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM tld_margin_rules").execute(&mut *tx).await.map_err(db_err)?;
    for r in rules {
        sqlx::query("INSERT INTO tld_margin_rules (tld, percent, fixed, min_margin, rounding) VALUES ($1,$2,$3,$4,$5)")
            .bind(&r.tld).bind(r.percent).bind(r.fixed).bind(r.min_margin).bind(&r.rounding)
            .execute(&mut *tx).await.map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)
}

/// The rule for `tld`: exact match, else `*`.
pub fn rule_for<'a>(rules: &'a [MarginRule], tld: &str) -> Option<&'a MarginRule> {
    rules.iter().find(|r| r.tld == tld).or_else(|| rules.iter().find(|r| r.tld == "*"))
}

// ----- cost sync -----

/// One registrar cost line (parsed from the addon's decimal strings).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cost {
    pub tld: String,
    pub currency: String,
    pub register: Decimal,
    pub renew: Decimal,
    pub transfer: Decimal,
}

impl Cost {
    /// Parse strict non-negative decimals; `None` for anything malformed.
    pub fn parse(tld: &str, currency: &str, register: &str, renew: &str, transfer: &str) -> Option<Cost> {
        let p = |s: &str| Decimal::from_str(s.trim()).ok().filter(|d| !d.is_sign_negative());
        Some(Cost { tld: tld.trim().trim_start_matches('.').to_ascii_lowercase(), currency: currency.trim().to_ascii_uppercase(), register: p(register)?, renew: p(renew)?, transfer: p(transfer)? })
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SyncLine {
    pub tld: String,
    pub currency: String,
    /// `updated`, `unchanged`, `no_rule`, `not_listed` (TLD not in our price list for this registrar/currency).
    pub action: String,
    #[schema(value_type = Option<String>)]
    pub old_register: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub new_register: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub old_renew: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub new_renew: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub old_transfer: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub new_transfer: Option<Decimal>,
}

/// Apply registrar costs to `tld_pricing` rows of `registrar` (same TLD and
/// currency). `dry_run` computes the report without writing. Rows without a
/// matching margin rule only get their cost columns recorded.
pub async fn apply_costs(pool: &PgPool, registrar: &str, costs: &[Cost], dry_run: bool) -> AppResult<Vec<SyncLine>> {
    let rules = margin_rules(pool).await?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let mut out = Vec::new();
    for c in costs {
        let row: Option<(Decimal, Decimal, Decimal)> = sqlx::query_as("SELECT register_price, renew_price, transfer_price FROM tld_pricing WHERE tld = $1 AND currency = $2 AND registrar = $3 FOR UPDATE")
            .bind(&c.tld).bind(&c.currency).bind(registrar).fetch_optional(&mut *tx).await.map_err(db_err)?;
        let Some((or, on, ot)) = row else {
            out.push(SyncLine { tld: c.tld.clone(), currency: c.currency.clone(), action: "not_listed".into(), old_register: None, new_register: None, old_renew: None, new_renew: None, old_transfer: None, new_transfer: None });
            continue;
        };
        let (action, nr, nn, nt) = match rule_for(&rules, &c.tld) {
            Some(r) => {
                let (nr, nn, nt) = (r.sell(c.register), r.sell(c.renew), r.sell(c.transfer));
                (if (nr, nn, nt) == (or, on, ot) { "unchanged" } else { "updated" }, nr, nn, nt)
            }
            None => ("no_rule", or, on, ot),
        };
        if !dry_run {
            sqlx::query("UPDATE tld_pricing SET register_price = $4, renew_price = $5, transfer_price = $6, cost_register = $7, cost_renew = $8, cost_transfer = $9, cost_synced_at = now()
                         WHERE tld = $1 AND currency = $2 AND registrar = $3")
                .bind(&c.tld).bind(&c.currency).bind(registrar).bind(nr).bind(nn).bind(nt).bind(c.register).bind(c.renew).bind(c.transfer)
                .execute(&mut *tx).await.map_err(db_err)?;
        }
        out.push(SyncLine { tld: c.tld.clone(), currency: c.currency.clone(), action: action.into(), old_register: Some(or), new_register: Some(nr), old_renew: Some(on), new_renew: Some(nn), old_transfer: Some(ot), new_transfer: Some(nt) });
    }
    if dry_run {
        tx.rollback().await.map_err(db_err)?;
    } else {
        tx.commit().await.map_err(db_err)?;
    }
    Ok(out)
}

/// Recorded cost columns for the price list screen.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct CostRow {
    pub tld: String,
    pub currency: String,
    pub cost_register: Option<Decimal>,
    pub cost_renew: Option<Decimal>,
    pub cost_transfer: Option<Decimal>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub cost_synced_at: Option<time::OffsetDateTime>,
}

pub async fn cost_rows(pool: &PgPool) -> AppResult<Vec<CostRow>> {
    sqlx::query_as("SELECT tld, currency::text AS currency, cost_register, cost_renew, cost_transfer, cost_synced_at FROM tld_pricing WHERE cost_synced_at IS NOT NULL ORDER BY tld, currency")
        .fetch_all(pool).await.map_err(db_err)
}

// ----- premium names -----

pub const PREMIUM_KEY: &str = "domains.premium";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PremiumSettings {
    pub enabled: bool,
    /// Markup on the registry premium price, percent.
    #[schema(value_type = String)]
    pub markup_percent: Decimal,
    /// Refuse premium names above this register sell price (0 = no cap).
    #[schema(value_type = String)]
    pub max_price: Decimal,
}

impl Default for PremiumSettings {
    fn default() -> Self {
        PremiumSettings { enabled: false, markup_percent: Decimal::ZERO, max_price: Decimal::ZERO }
    }
}

impl PremiumSettings {
    pub fn validate(&self) -> AppResult<()> {
        if self.markup_percent.is_sign_negative() || self.markup_percent > Decimal::from(1000) || self.max_price.is_sign_negative() {
            return Err(AppError::Validation("error.validation"));
        }
        Ok(())
    }
    /// Sell price: cost × (1 + markup/100), half-up to 0.01.
    pub fn sell(&self, cost: Decimal) -> Decimal {
        (cost * (Decimal::ONE + self.markup_percent / Decimal::from(100))).round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
    }
}

pub async fn premium_settings(pool: &PgPool) -> AppResult<PremiumSettings> {
    Ok(crate::settings::get(pool, PREMIUM_KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn set_premium_settings(pool: &PgPool, p: &PremiumSettings) -> AppResult<()> {
    p.validate()?;
    crate::settings::set(pool, PREMIUM_KEY, serde_json::to_value(p).map_err(|_| AppError::Validation("error.validation"))?).await
}

/// Decide premium pricing for an order: `Ok(None)` = standard name;
/// `Ok(Some((register_sell, renew_sell)))` = premium; errors when premium
/// names are disabled, the currency differs or the cap is exceeded.
pub fn premium_quote(p: &PremiumSettings, currency: &str, q: Option<(&str, &str, &str)>) -> AppResult<Option<(Decimal, Decimal)>> {
    let Some((qc, reg, ren)) = q else { return Ok(None) };
    if !p.enabled {
        return Err(AppError::Validation("error.domain_premium_disabled"));
    }
    if !qc.eq_ignore_ascii_case(currency) {
        return Err(AppError::Validation("error.domain_premium_currency"));
    }
    let parse = |s: &str| Decimal::from_str(s.trim()).ok().filter(|d| !d.is_sign_negative()).ok_or(AppError::Validation("error.registrar_unavailable"));
    let (r, n) = (p.sell(parse(reg)?), p.sell(parse(ren)?));
    if !p.max_price.is_zero() && r > p.max_price {
        return Err(AppError::Validation("error.domain_premium_too_expensive"));
    }
    Ok(Some((r, n)))
}

// ----- per-TLD additional fields -----

#[derive(Debug, Clone, PartialEq, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TldField {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub tld: String,
    pub key: String,
    pub label: String,
    /// `text`, `select` or `checkbox`.
    pub kind: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "d200")]
    pub max_length: i32,
    #[serde(default)]
    pub sort_order: i32,
}

fn d200() -> i32 {
    200
}

impl TldField {
    pub fn validate(&self) -> AppResult<()> {
        // Case is kept: registrar parameter names are case-sensitive.
        let key_ok = self.key.len() <= 64 && self.key.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && self.key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        let kind_ok = match self.kind.as_str() {
            "select" => !self.options.is_empty() && self.options.iter().all(|o| !o.trim().is_empty() && o.len() <= 120),
            "text" | "checkbox" => true,
            _ => false,
        };
        let label_ok = !self.label.trim().is_empty() && self.label.len() <= 120;
        if !key_ok || !kind_ok || !label_ok || !(1..=1000).contains(&self.max_length) || self.tld.is_empty() {
            return Err(AppError::Validation("error.tld_field_invalid"));
        }
        Ok(())
    }
}

pub async fn tld_fields(pool: &PgPool, tld: &str) -> AppResult<Vec<TldField>> {
    sqlx::query_as("SELECT id, tld, key, label, kind, options, required, max_length, sort_order FROM tld_fields WHERE tld = $1 ORDER BY sort_order, key")
        .bind(tld).fetch_all(pool).await.map_err(db_err)
}

pub async fn all_tld_fields(pool: &PgPool) -> AppResult<Vec<TldField>> {
    sqlx::query_as("SELECT id, tld, key, label, kind, options, required, max_length, sort_order FROM tld_fields ORDER BY tld, sort_order, key").fetch_all(pool).await.map_err(db_err)
}

/// Replace the field set of one TLD.
pub async fn set_tld_fields(pool: &PgPool, tld: &str, fields: &[TldField]) -> AppResult<Vec<TldField>> {
    let mut keys = std::collections::HashSet::new();
    for f in fields {
        if f.tld != tld {
            return Err(AppError::Validation("error.tld_field_invalid"));
        }
        f.validate()?;
        if !keys.insert(f.key.to_ascii_lowercase()) {
            return Err(AppError::Validation("error.tld_field_invalid"));
        }
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM tld_fields WHERE tld = $1").bind(tld).execute(&mut *tx).await.map_err(db_err)?;
    for f in fields {
        sqlx::query("INSERT INTO tld_fields (tld, key, label, kind, options, required, max_length, sort_order) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(tld).bind(&f.key).bind(f.label.trim()).bind(&f.kind).bind(&f.options).bind(f.required).bind(f.max_length).bind(f.sort_order)
            .execute(&mut *tx).await.map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    tld_fields(pool, tld).await
}

/// Validate submitted values against the TLD's fields; unknown keys are
/// dropped. Returns the clean object to store on the domain.
pub fn validate_extra(fields: &[TldField], input: &Value) -> AppResult<Value> {
    let mut out = Map::new();
    for f in fields {
        let v = input.get(&f.key);
        match f.kind.as_str() {
            "checkbox" => {
                let b = v.and_then(Value::as_bool).unwrap_or(false);
                if f.required && !b {
                    return Err(AppError::Validation("error.domain_field_required"));
                }
                out.insert(f.key.clone(), Value::Bool(b));
            }
            _ => {
                let s = v.and_then(Value::as_str).map(str::trim).unwrap_or("");
                if s.is_empty() {
                    if f.required {
                        return Err(AppError::Validation("error.domain_field_required"));
                    }
                    continue;
                }
                if s.chars().count() > f.max_length as usize || (f.kind == "select" && !f.options.iter().any(|o| o == s)) {
                    return Err(AppError::Validation("error.domain_field_invalid"));
                }
                out.insert(f.key.clone(), Value::String(s.into()));
            }
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    fn rule(p: &str, f: &str, m: &str, r: &str) -> MarginRule {
        MarginRule { tld: "*".into(), percent: d(p), fixed: d(f), min_margin: d(m), rounding: r.into() }
    }

    #[test]
    fn margin_math() {
        assert_eq!(rule("20", "0", "0", "cent").sell(d("8.50")), d("10.20"));
        assert_eq!(rule("10", "0.005", "0", "cent").sell(d("1.00")), d("1.11")); // 1.105 half-up
        assert_eq!(rule("0", "0", "2", "cent").sell(d("9.10")), d("11.10"));
        assert_eq!(rule("20", "0", "0", "whole").sell(d("8.50")), d("11"));
        assert_eq!(rule("20", "0", "0", "ninety_nine").sell(d("8.50")), d("10.99"));
        assert_eq!(rule("0", "0", "0", "ninety_nine").sell(d("10.99")), d("10.99"));
        assert_eq!(rule("0", "0", "0", "ninety_nine").sell(d("11.00")), d("11.99"));
    }

    #[test]
    fn rule_lookup_and_validation() {
        let mut com = rule("5", "0", "0", "cent");
        com.tld = "com".into();
        let rules = vec![rule("20", "0", "0", "cent"), com];
        assert_eq!(rule_for(&rules, "com").unwrap().percent, d("5"));
        assert_eq!(rule_for(&rules, "net").unwrap().percent, d("20"));
        assert!(rule_for(&[], "net").is_none());
        assert!(rule("-1", "0", "0", "cent").validate().is_err());
        assert!(rule("1", "0", "0", "floor").validate().is_err());
    }

    #[test]
    fn premium_rules() {
        let mut p = PremiumSettings::default();
        assert!(premium_quote(&p, "EUR", None).unwrap().is_none());
        assert!(premium_quote(&p, "EUR", Some(("EUR", "250", "30"))).is_err());
        p.enabled = true;
        p.markup_percent = d("10");
        assert_eq!(premium_quote(&p, "EUR", Some(("EUR", "250.00", "30.00"))).unwrap(), Some((d("275.00"), d("33.00"))));
        assert!(premium_quote(&p, "USD", Some(("EUR", "250", "30"))).is_err());
        p.max_price = d("100");
        assert!(premium_quote(&p, "EUR", Some(("EUR", "250", "30"))).is_err());
    }

    #[test]
    fn extra_fields() {
        let f = vec![
            TldField { id: None, tld: "ro".into(), key: "cnp".into(), label: "CNP".into(), kind: "text".into(), options: vec![], required: true, max_length: 13, sort_order: 0 },
            TldField { id: None, tld: "ro".into(), key: "entity".into(), label: "Entity".into(), kind: "select".into(), options: vec!["person".into(), "company".into()], required: false, max_length: 200, sort_order: 1 },
            TldField { id: None, tld: "ro".into(), key: "agree".into(), label: "Agree".into(), kind: "checkbox".into(), options: vec![], required: true, max_length: 200, sort_order: 2 },
        ];
        assert!(f.iter().all(|x| x.validate().is_ok()));
        let ok = validate_extra(&f, &json!({"cnp": " 1234567890123 ", "entity": "person", "agree": true, "junk": 1})).unwrap();
        assert_eq!(ok, json!({"cnp": "1234567890123", "entity": "person", "agree": true}));
        assert!(validate_extra(&f, &json!({"agree": true})).is_err());
        assert!(validate_extra(&f, &json!({"cnp": "12345678901234", "agree": true})).is_err());
        assert!(validate_extra(&f, &json!({"cnp": "1", "entity": "alien", "agree": true})).is_err());
        assert!(validate_extra(&f, &json!({"cnp": "1"})).is_err());
    }

    #[test]
    fn field_keys_keep_case() {
        let k = |key: &str| TldField { id: None, tld: "us".into(), key: key.into(), label: "L".into(), kind: "text".into(), options: vec![], required: false, max_length: 200, sort_order: 0 };
        for ok in ["RegistrantNexus", "X-CA-LEGALTYPE", "contactRegistrant.nexusCategory", "us_purpose", &"a".repeat(64)] {
            assert!(k(ok).validate().is_ok(), "{ok}");
        }
        for bad in ["", "1abc", "_x", "-x", "a b", "a=b", "a&b", "é", &"a".repeat(65)] {
            assert!(k(bad).validate().is_err(), "{bad}");
        }
        // Values are looked up by the exact key.
        let f = vec![k("RegistrantNexus")];
        assert_eq!(validate_extra(&f, &json!({"RegistrantNexus": "C11", "registrantnexus": "x"})).unwrap(), json!({"RegistrantNexus": "C11"}));
    }

    #[test]
    fn cost_parse() {
        let c = Cost::parse(".COM", "eur", "8.50", "9.10", "8.5").unwrap();
        assert_eq!((c.tld.as_str(), c.currency.as_str(), c.renew), ("com", "EUR", d("9.10")));
        assert!(Cost::parse("com", "EUR", "-1", "1", "1").is_none());
        assert!(Cost::parse("com", "EUR", "x", "1", "1").is_none());
    }
}
