//! Automatic exchange-rate updates: feed parsing (ECB XML or a JSON
//! `{"base": "EUR", "rates": {...}}` document) and applying the rates, with an
//! optional markup, relative to the default currency. Fetching lives in the
//! API crate behind the mockable HTTP transport.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::collections::BTreeMap;
use std::str::FromStr;
use time::OffsetDateTime;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RateSettings {
    pub enabled: bool,
    pub source: String,
    pub url: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub markup_percent: Decimal,
    pub update_prices: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RateSettingsInput {
    pub enabled: bool,
    pub source: String,
    pub url: String,
    #[schema(value_type = String)]
    pub markup_percent: Decimal,
    #[serde(default)]
    pub update_prices: bool,
}

/// Feed rates: units of each currency per 1 unit of `base`.
#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    pub base: String,
    pub rates: BTreeMap<String, Decimal>,
}

fn bad() -> AppError {
    AppError::Validation("error.rate_feed_invalid")
}

fn valid_code(c: &str) -> bool {
    c.len() == 3 && c.chars().all(|ch| ch.is_ascii_uppercase())
}

/// Parse the ECB daily reference XML (`<Cube currency='USD' rate='1.0812'/>`).
/// The base is always EUR.
pub fn parse_ecb(xml: &str) -> AppResult<Feed> {
    let mut rates = BTreeMap::new();
    for tag in xml.split('<').skip(1) {
        let t = tag.trim_start();
        if !t.starts_with("Cube") || !t.contains("currency") {
            continue;
        }
        let attr = |name: &str| -> Option<String> {
            let i = t.find(&format!("{name}="))? + name.len() + 1;
            let q = t[i..].chars().next()?;
            if q != '\'' && q != '"' {
                return None;
            }
            let rest = &t[i + 1..];
            Some(rest[..rest.find(q)?].to_string())
        };
        let (Some(c), Some(r)) = (attr("currency"), attr("rate")) else { continue };
        let r = Decimal::from_str(r.trim()).map_err(|_| bad())?;
        if valid_code(&c) && r > Decimal::ZERO {
            rates.insert(c, r);
        }
    }
    if rates.is_empty() {
        return Err(bad());
    }
    rates.insert("EUR".into(), Decimal::ONE);
    Ok(Feed { base: "EUR".into(), rates })
}

/// Parse `{"base": "USD", "rates": {"EUR": 0.92, ...}}` (exchangerate.host /
/// openexchangerates style; numbers or numeric strings).
pub fn parse_json(body: &str) -> AppResult<Feed> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| bad())?;
    let base = v["base"].as_str().or_else(|| v["base_code"].as_str()).ok_or_else(bad)?.to_ascii_uppercase();
    if !valid_code(&base) {
        return Err(bad());
    }
    let obj = v["rates"].as_object().or_else(|| v["conversion_rates"].as_object()).ok_or_else(bad)?;
    let mut rates = BTreeMap::new();
    for (k, x) in obj {
        let s = match x {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            _ => continue,
        };
        let Ok(r) = Decimal::from_str(&s).or_else(|_| Decimal::from_scientific(&s)) else { continue };
        let k = k.to_ascii_uppercase();
        if valid_code(&k) && r > Decimal::ZERO {
            rates.insert(k, r);
        }
    }
    rates.insert(base.clone(), Decimal::ONE);
    if rates.len() < 2 {
        return Err(bad());
    }
    Ok(Feed { base, rates })
}

/// Rate of `code` per 1 unit of `default`, with `markup` percent added
/// (never applied to the default itself), rounded to 10 places.
pub fn relative_rate(feed: &Feed, default: &str, code: &str, markup: Decimal) -> Option<Decimal> {
    let d = *feed.rates.get(default)?;
    let c = *feed.rates.get(code)?;
    if d.is_zero() {
        return None;
    }
    if code == default {
        return Some(Decimal::ONE);
    }
    let r = c / d * (Decimal::ONE + markup / Decimal::from(100));
    let r = r.round_dp_with_strategy(10, RoundingStrategy::MidpointAwayFromZero);
    (r > Decimal::ZERO).then_some(r)
}

pub async fn settings(pool: &PgPool) -> AppResult<RateSettings> {
    sqlx::query_as("SELECT enabled, source, url, markup_percent, update_prices, last_run_at, last_error FROM currency_rate_settings")
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn save_settings(pool: &PgPool, s: &RateSettingsInput) -> AppResult<()> {
    if !matches!(s.source.as_str(), "ecb" | "json") {
        return Err(AppError::Validation("error.validation"));
    }
    let url = s.url.trim();
    if !url.starts_with("https://") || url.len() > 500 || url.chars().any(char::is_whitespace) {
        return Err(AppError::Validation("error.invalid_url"));
    }
    if s.markup_percent < Decimal::from(-50) || s.markup_percent > Decimal::from(100) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query("UPDATE currency_rate_settings SET enabled=$1, source=$2, url=$3, markup_percent=$4, update_prices=$5")
        .bind(s.enabled)
        .bind(&s.source)
        .bind(url)
        .bind(s.markup_percent)
        .bind(s.update_prices)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn record_run(pool: &PgPool, error: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE currency_rate_settings SET last_run_at = now(), last_error = $1")
        .bind(error)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub updated: Vec<String>,
    pub missing: Vec<String>,
}

/// Apply a feed to every currency with `auto_update` (except the default,
/// which stays at 1). Changes are logged in `currency_rate_history`.
pub async fn apply(pool: &PgPool, feed: &Feed, markup: Decimal, source: &str) -> AppResult<Applied> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let default: String = sqlx::query_scalar("SELECT code::text FROM currencies WHERE is_default")
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    if !feed.rates.contains_key(&default) {
        return Err(AppError::Validation("error.rate_feed_missing_default"));
    }
    let rows: Vec<(String, Decimal)> =
        sqlx::query_as("SELECT code::text, rate FROM currencies WHERE auto_update AND NOT is_default ORDER BY code FOR UPDATE")
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
    let mut out = Applied { updated: vec![], missing: vec![] };
    for (code, old) in rows {
        let Some(new) = relative_rate(feed, &default, &code, markup) else {
            out.missing.push(code);
            continue;
        };
        let raw = relative_rate(feed, &default, &code, Decimal::ZERO).unwrap_or(new);
        sqlx::query("UPDATE currencies SET rate = $2, rate_source = $3, rate_updated_at = now(), updated_at = now() WHERE code = $1")
            .bind(&code)
            .bind(new)
            .bind(raw)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        if new != old {
            sqlx::query("INSERT INTO currency_rate_history (code, old_rate, new_rate, source) VALUES ($1,$2,$3,$4)")
                .bind(&code)
                .bind(old)
                .bind(new)
                .bind(source)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        out.updated.push(code);
    }
    tx.commit().await.map_err(db_err)?;
    Ok(out)
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct HistoryRow {
    pub code: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub old_rate: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub new_rate: Decimal,
    pub source: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn history(pool: &PgPool, limit: i64) -> AppResult<Vec<HistoryRow>> {
    sqlx::query_as("SELECT code::text AS code, old_rate, new_rate, source, created_at FROM currency_rate_history ORDER BY id DESC LIMIT $1")
        .bind(limit.clamp(1, 500))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECB: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01"><Cube><Cube time='2026-10-29'>
<Cube currency='USD' rate='1.0812'/><Cube currency="JPY" rate="161.20"/><Cube currency='RON' rate='4.9750'/>
<Cube currency='bad' rate='1'/><Cube currency='GBP' rate='x'/></Cube></Cube></gesmes:Envelope>"#;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn parses_ecb() {
        assert!(parse_ecb(ECB).is_err(), "a malformed rate rejects the feed");
        let ok = ECB.replace("rate='x'", "rate='0.8501'");
        let f = parse_ecb(&ok).unwrap();
        assert_eq!(f.base, "EUR");
        assert_eq!(f.rates["USD"], d("1.0812"));
        assert_eq!(f.rates["JPY"], d("161.20"));
        assert_eq!(f.rates["EUR"], Decimal::ONE);
        assert!(!f.rates.contains_key("bad"));
        assert!(parse_ecb("<html></html>").is_err());
    }

    #[test]
    fn parses_json() {
        let f = parse_json(r#"{"base":"usd","rates":{"EUR":0.92,"RON":"4.6","XX":1,"JPY":"abc"}}"#).unwrap();
        assert_eq!(f.base, "USD");
        assert_eq!(f.rates["EUR"], d("0.92"));
        assert_eq!(f.rates["USD"], Decimal::ONE);
        assert!(!f.rates.contains_key("JPY"));
        assert!(parse_json(r#"{"rates":{}}"#).is_err());
    }

    #[test]
    fn rebases_and_marks_up() {
        let f = parse_json(r#"{"base":"USD","rates":{"EUR":"0.8","RON":"4"}}"#).unwrap();
        // Default EUR: RON per EUR = 4 / 0.8 = 5.
        assert_eq!(relative_rate(&f, "EUR", "RON", Decimal::ZERO), Some(d("5")));
        assert_eq!(relative_rate(&f, "EUR", "USD", Decimal::ZERO), Some(d("1.25")));
        assert_eq!(relative_rate(&f, "EUR", "RON", d("2")), Some(d("5.1")));
        assert_eq!(relative_rate(&f, "EUR", "EUR", d("2")), Some(Decimal::ONE));
        assert_eq!(relative_rate(&f, "EUR", "GBP", Decimal::ZERO), None);
    }
}
