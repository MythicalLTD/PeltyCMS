//! Per-gateway fees, min/max amounts and currency restrictions. All money is
//! `Decimal`; limits and fixed fees are per currency (no FX conversion).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CurrencyLimit {
    #[schema(value_type = Option<String>)]
    pub min: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub max: Option<Decimal>,
    #[schema(value_type = String)]
    pub fee_fixed: Decimal,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GatewayRules {
    /// Percentage fee, e.g. `2.9`.
    #[schema(value_type = String)]
    pub fee_percent: Decimal,
    /// Allowed currencies; empty = all.
    pub currencies: Vec<String>,
    pub limits: BTreeMap<String, CurrencyLimit>,
}

fn code_ok(c: &str) -> bool {
    c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase())
}

impl GatewayRules {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        if self.fee_percent < Decimal::ZERO || self.fee_percent > Decimal::ONE_HUNDRED || self.fee_percent.scale() > 4 {
            return Err("error.invalid_fee");
        }
        let mut cur: Vec<String> = self.currencies.iter().map(|c| c.trim().to_ascii_uppercase()).collect();
        cur.sort();
        cur.dedup();
        if cur.len() > 50 || !cur.iter().all(|c| code_ok(c)) {
            return Err("error.invalid_currency");
        }
        self.currencies = cur;
        let mut limits = BTreeMap::new();
        for (k, l) in self.limits {
            let k = k.trim().to_ascii_uppercase();
            if !code_ok(&k) {
                return Err("error.invalid_currency");
            }
            let neg = |v: Option<Decimal>| v.is_some_and(|v| v < Decimal::ZERO);
            if neg(l.min) || neg(l.max) || l.fee_fixed < Decimal::ZERO {
                return Err("error.invalid_amount");
            }
            if let (Some(a), Some(b)) = (l.min, l.max) {
                if a > b {
                    return Err("error.invalid_limits");
                }
            }
            if l != CurrencyLimit::default() {
                limits.insert(k, l);
            }
        }
        self.limits = limits;
        Ok(self)
    }

    /// `Err(code)` when this gateway cannot take `amount` in `currency`.
    pub fn check(&self, currency: &str, amount: Decimal) -> Result<(), &'static str> {
        if !self.currencies.is_empty() && !self.currencies.iter().any(|c| c == currency) {
            return Err("error.gateway_currency");
        }
        if let Some(l) = self.limits.get(currency) {
            if l.min.is_some_and(|m| amount < m) {
                return Err("error.gateway_below_min");
            }
            if l.max.is_some_and(|m| amount > m) {
                return Err("error.gateway_above_max");
            }
        }
        Ok(())
    }

    /// Processing fee for `amount`, rounded half-up to the currency exponent.
    pub fn fee(&self, currency: &str, amount: Decimal, exp: u32) -> Decimal {
        let fixed = self.limits.get(currency).map_or(Decimal::ZERO, |l| l.fee_fixed);
        (amount * self.fee_percent / Decimal::ONE_HUNDRED + fixed).round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero)
    }
}

pub async fn get(pool: &PgPool, gateway: &str) -> AppResult<GatewayRules> {
    let row: Option<(Decimal, Vec<String>, serde_json::Value)> = sqlx::query_as("SELECT fee_percent, currencies, limits FROM gateway_rules WHERE gateway = $1")
        .bind(gateway)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(row
        .map(|(p, c, l)| GatewayRules { fee_percent: p.normalize(), currencies: c, limits: serde_json::from_value(l).unwrap_or_default() })
        .unwrap_or_default())
}

pub async fn put(pool: &PgPool, gateway: &str, r: &GatewayRules) -> AppResult<()> {
    let limits = serde_json::to_value(&r.limits).map_err(|e| AppError::Internal(e.to_string()))?;
    sqlx::query(
        "INSERT INTO gateway_rules (gateway, fee_percent, currencies, limits) VALUES ($1, $2, $3, $4)
         ON CONFLICT (gateway) DO UPDATE SET fee_percent = $2, currencies = $3, limits = $4, updated_at = now()",
    )
    .bind(gateway)
    .bind(r.fee_percent)
    .bind(&r.currencies)
    .bind(limits)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Store the fee once (first computation wins, so retries don't change it).
pub async fn record_fee(pool: &PgPool, payment_id: Uuid, fee: Decimal) -> AppResult<()> {
    sqlx::query("UPDATE payments SET fee = $2 WHERE id = $1 AND fee IS NULL").bind(payment_id).bind(fee).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn fee_of(pool: &PgPool, payment_id: Uuid) -> AppResult<Option<Decimal>> {
    sqlx::query_scalar("SELECT fee FROM payments WHERE id = $1").bind(payment_id).fetch_optional(pool).await.map_err(db_err).map(Option::flatten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn checks_and_fees() {
        let r = GatewayRules {
            fee_percent: d("2.9"),
            currencies: vec!["eur".into(), "RON".into(), "EUR".into()],
            limits: BTreeMap::from([("eur".into(), CurrencyLimit { min: Some(d("1.00")), max: Some(d("5000")), fee_fixed: d("0.25") }), ("USD".into(), CurrencyLimit::default())]),
        }
        .normalized()
        .unwrap();
        assert_eq!(r.currencies, vec!["EUR", "RON"]);
        assert_eq!(r.limits.len(), 1);
        assert_eq!(r.check("USD", d("10")), Err("error.gateway_currency"));
        assert_eq!(r.check("EUR", d("0.99")), Err("error.gateway_below_min"));
        assert_eq!(r.check("EUR", d("5000.01")), Err("error.gateway_above_max"));
        assert_eq!(r.check("EUR", d("5000")), Ok(()));
        assert_eq!(r.check("RON", d("999999")), Ok(()));
        // 100.00 * 2.9% + 0.25 = 3.15; 10.10 * 2.9% = 0.2929 + 0.25 = 0.5429 → 0.54
        assert_eq!(r.fee("EUR", d("100.00"), 2), d("3.15"));
        assert_eq!(r.fee("EUR", d("10.10"), 2), d("0.54"));
        assert_eq!(r.fee("RON", d("0.50"), 2), d("0.01"));
        assert_eq!(GatewayRules { fee_percent: d("101"), ..Default::default() }.normalized().unwrap_err(), "error.invalid_fee");
        let bad = GatewayRules { limits: BTreeMap::from([("EUR".into(), CurrencyLimit { min: Some(d("5")), max: Some(d("1")), fee_fixed: d("0") })]), ..Default::default() };
        assert_eq!(bad.normalized().unwrap_err(), "error.invalid_limits");
        assert!(GatewayRules::default().check("JPY", d("1")).is_ok());
    }
}
