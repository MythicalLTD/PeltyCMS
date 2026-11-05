//! Currency administration: exchange rates against the default currency,
//! and converting base-currency product prices into other currencies.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Currency {
    pub code: String,
    pub exponent: i16,
    pub enabled: bool,
    #[serde(with = "rust_decimal::serde::str")]
    pub rate: Decimal,
    pub is_default: bool,
    pub prefix: String,
    pub suffix: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub decimal_sep: String,
    pub thousands_sep: String,
    pub auto_update: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub rate_updated_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CurrencyInput {
    /// ISO 4217 code (create only).
    #[serde(default)]
    pub code: Option<String>,
    pub exponent: i16,
    pub enabled: bool,
    #[schema(value_type = String)]
    pub rate: Decimal,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default = "dot")]
    pub decimal_sep: String,
    #[serde(default = "comma")]
    pub thousands_sep: String,
    #[serde(default = "yes")]
    pub auto_update: bool,
}

fn dot() -> String {
    ".".into()
}
fn comma() -> String {
    ",".into()
}
fn yes() -> bool {
    true
}

/// Format `amount` with a currency's prefix/suffix, separators and exponent
/// (e.g. `1.234,50 lei`). Pure; used by PDFs, mails and tests.
pub fn format_amount(amount: Decimal, exp: u32, prefix: &str, suffix: &str, dec: &str, thou: &str) -> String {
    let r = amount.round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero);
    let neg = r.is_sign_negative() && !r.is_zero();
    let s = format!("{:.*}", exp as usize, r.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push_str(thou);
        }
        grouped.push(ch);
    }
    let num = if frac.is_empty() { grouped } else { format!("{grouped}{dec}{frac}") };
    format!("{}{prefix}{num}{suffix}", if neg { "-" } else { "" })
}

fn v() -> AppError {
    AppError::Validation("error.validation")
}

/// Pure validation (unit-tested).
pub fn check(c: &CurrencyInput) -> AppResult<()> {
    if !matches!(c.decimal_sep.as_str(), "." | ",") || !matches!(c.thousands_sep.as_str(), "," | "." | " " | "'" | "") || c.decimal_sep == c.thousands_sep {
        return Err(v());
    }
    if !(0..=4).contains(&c.exponent) || c.prefix.chars().count() > 8 || c.suffix.chars().count() > 8 {
        return Err(v());
    }
    if c.rate <= Decimal::ZERO || c.rate > Decimal::from(1_000_000_000) {
        return Err(AppError::Validation("error.invalid_rate"));
    }
    if let Some(code) = &c.code {
        if code.len() != 3 || !code.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return Err(AppError::Validation("error.invalid_currency"));
        }
    }
    Ok(())
}

/// Convert `amount` from a currency with rate `from` to one with rate `to`
/// (rates are units per 1 default-currency unit), rounded half-up to `exp`.
pub fn convert(amount: Decimal, from: Decimal, to: Decimal, exp: u32) -> Decimal {
    if from.is_zero() {
        return Decimal::ZERO;
    }
    (amount * to / from).round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero)
}

const COLS: &str = "code::text AS code, exponent, enabled, rate, is_default, prefix, suffix, updated_at, decimal_sep, thousands_sep, auto_update, rate_updated_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Currency>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM currencies ORDER BY is_default DESC, code"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn create(pool: &PgPool, c: &CurrencyInput) -> AppResult<String> {
    check(c)?;
    let code = c.code.as_deref().ok_or_else(v)?.to_ascii_uppercase();
    let r = sqlx::query(
        "INSERT INTO currencies (code, exponent, enabled, rate, prefix, suffix, decimal_sep, thousands_sep, auto_update) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (code) DO NOTHING",
    )
    .bind(&code)
    .bind(c.exponent)
    .bind(c.enabled)
    .bind(c.rate)
    .bind(c.prefix.trim())
    .bind(c.suffix.trim())
    .bind(&c.decimal_sep)
    .bind(&c.thousands_sep)
    .bind(c.auto_update)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.currency_exists"));
    }
    Ok(code)
}

/// Update one currency. The default currency keeps rate 1 and stays enabled.
/// The exponent can only change while no prices exist in that currency.
pub async fn update(pool: &PgPool, code: &str, c: &CurrencyInput) -> AppResult<()> {
    check(c)?;
    let code = code.to_ascii_uppercase();
    let cur: Option<(bool, i16)> = sqlx::query_as("SELECT is_default, exponent FROM currencies WHERE code = $1")
        .bind(&code)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let (is_default, exp) = cur.ok_or(AppError::NotFound)?;
    if is_default && (!c.enabled || c.rate != Decimal::ONE) {
        return Err(AppError::Validation("error.default_currency_locked"));
    }
    if exp != c.exponent {
        let used: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM product_prices WHERE currency = $1) OR EXISTS(SELECT 1 FROM invoices WHERE currency = $1)",
        )
        .bind(&code)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
        if used {
            return Err(AppError::Conflict("error.currency_in_use"));
        }
    }
    sqlx::query("UPDATE currencies SET exponent = $2, enabled = $3, rate = $4, prefix = $5, suffix = $6, decimal_sep = $7, thousands_sep = $8, auto_update = $9, updated_at = now() WHERE code = $1")
        .bind(&code)
        .bind(c.exponent)
        .bind(c.enabled)
        .bind(c.rate)
        .bind(c.prefix.trim())
        .bind(c.suffix.trim())
        .bind(&c.decimal_sep)
        .bind(&c.thousands_sep)
        .bind(c.auto_update)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Make `code` the default: all rates are rebased so it has rate 1.
pub async fn set_default(pool: &PgPool, code: &str) -> AppResult<()> {
    let code = code.to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r: Option<(Decimal, bool)> = sqlx::query_as("SELECT rate, enabled FROM currencies WHERE code = $1 FOR UPDATE")
        .bind(&code)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let (rate, enabled) = r.ok_or(AppError::NotFound)?;
    if !enabled {
        return Err(AppError::Validation("error.currency_disabled"));
    }
    sqlx::query("UPDATE currencies SET is_default = FALSE WHERE is_default")
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE currencies SET rate = CASE WHEN code = $1 THEN 1 ELSE round(rate / $2, 10) END, is_default = (code = $1), updated_at = now()")
        .bind(&code)
        .bind(rate)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

/// Recalculate product (and config option) prices in every other enabled
/// currency from the default-currency prices. `only_missing` keeps prices
/// that already exist. Returns the number of price rows written.
pub async fn sync_prices(pool: &PgPool, only_missing: bool) -> AppResult<u64> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let conflict = if only_missing { "DO NOTHING" } else { "DO UPDATE SET amount = EXCLUDED.amount" };
    let a = sqlx::query(&format!(
        "INSERT INTO product_prices (id, product_id, currency, cycle, amount, setup_fee)
         SELECT gen_random_uuid(), pp.product_id, c.code, pp.cycle,
                round(pp.amount * c.rate, c.exponent), round(pp.setup_fee * c.rate, c.exponent)
         FROM product_prices pp
         JOIN currencies d ON d.is_default AND pp.currency = d.code
         JOIN currencies c ON c.enabled AND NOT c.is_default
         ON CONFLICT (product_id, currency, cycle) {}",
        if only_missing { "DO NOTHING" } else { "DO UPDATE SET amount = EXCLUDED.amount, setup_fee = EXCLUDED.setup_fee" }
    ))
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    let b = sqlx::query(&format!(
        "INSERT INTO product_config_prices (choice_id, currency, cycle, amount)
         SELECT cp.choice_id, c.code, cp.cycle, round(cp.amount * c.rate, c.exponent)
         FROM product_config_prices cp
         JOIN currencies d ON d.is_default AND cp.currency = d.code
         JOIN currencies c ON c.enabled AND NOT c.is_default
         ON CONFLICT (choice_id, currency, cycle) {conflict}"
    ))
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    tx.commit().await.map_err(db_err)?;
    Ok(a + b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn converts_and_rounds() {
        assert_eq!(convert(d("10.00"), d("1"), d("4.9750"), 2), d("49.75"));
        assert_eq!(convert(d("10.00"), d("1"), d("161.2345"), 0), d("1612"));
        assert_eq!(convert(d("49.75"), d("4.975"), d("1"), 2), d("10.00"));
        assert_eq!(convert(d("0.005"), d("1"), d("1"), 2), d("0.01"));
    }

    #[test]
    fn formats() {
        assert_eq!(format_amount(d("1234.5"), 2, "", " lei", ",", "."), "1.234,50 lei");
        assert_eq!(format_amount(d("1234567.891"), 2, "$", "", ".", ","), "$1,234,567.89");
        assert_eq!(format_amount(d("1612"), 0, "¥", "", ".", ","), "¥1,612");
        assert_eq!(format_amount(d("-5"), 3, "", " KWD", ".", ""), "-5.000 KWD");
        assert_eq!(format_amount(d("999"), 2, "", "", ".", ","), "999.00");
    }

    #[test]
    fn validates() {
        let ok = CurrencyInput { code: Some("CHF".into()), exponent: 2, enabled: true, rate: d("0.95"), prefix: "".into(), suffix: " CHF".into(), decimal_sep: ".".into(), thousands_sep: ",".into(), auto_update: true };
        assert!(check(&ok).is_ok());
        assert!(check(&CurrencyInput { rate: d("0"), ..ok_clone(&ok) }).is_err());
        assert!(check(&CurrencyInput { exponent: 5, ..ok_clone(&ok) }).is_err());
        assert!(check(&CurrencyInput { code: Some("CH1".into()), ..ok_clone(&ok) }).is_err());
    }

    fn ok_clone(c: &CurrencyInput) -> CurrencyInput {
        CurrencyInput { code: c.code.clone(), exponent: c.exponent, enabled: c.enabled, rate: c.rate, prefix: c.prefix.clone(), suffix: c.suffix.clone(), decimal_sep: c.decimal_sep.clone(), thousands_sep: c.thousands_sep.clone(), auto_update: c.auto_update }
    }
}
