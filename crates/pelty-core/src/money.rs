//! Exact money type. Never use floats for money.
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// ISO-4217 currency code (3 uppercase ASCII letters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Currency([u8; 3]);

impl Currency {
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() == 3 && b.iter().all(|c| c.is_ascii_uppercase()) {
            Some(Currency([b[0], b[1], b[2]]))
        } else {
            None
        }
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("XXX")
    }
    /// Minor units (decimal places) for common currencies.
    pub fn minor_units(&self) -> u32 {
        match self.as_str() {
            "JPY" | "KRW" | "HUF" => 0,
            "BHD" | "KWD" | "OMR" => 3,
            _ => 2,
        }
    }
}
impl TryFrom<String> for Currency {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        Currency::parse(&s).ok_or_else(|| format!("invalid currency: {s}"))
    }
}
impl From<Currency> for String {
    fn from(c: Currency) -> String {
        c.as_str().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    pub amount: Decimal,
    pub currency: Currency,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MoneyError {
    CurrencyMismatch,
    Overflow,
}

impl Money {
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Money { amount, currency }
    }
    pub fn checked_add(self, o: Money) -> Result<Money, MoneyError> {
        if self.currency != o.currency {
            return Err(MoneyError::CurrencyMismatch);
        }
        let amount = self
            .amount
            .checked_add(o.amount)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money {
            amount,
            currency: self.currency,
        })
    }
    pub fn checked_sub(self, o: Money) -> Result<Money, MoneyError> {
        if self.currency != o.currency {
            return Err(MoneyError::CurrencyMismatch);
        }
        let amount = self
            .amount
            .checked_sub(o.amount)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money {
            amount,
            currency: self.currency,
        })
    }
    /// Round to currency minor units (banker's rounding).
    pub fn rounded(self) -> Money {
        Money {
            amount: self.amount.round_dp(self.currency.minor_units()),
            currency: self.currency,
        }
    }
    /// Amount in minor units (e.g. cents) for gateway APIs.
    pub fn to_minor_units(self) -> Option<i64> {
        use rust_decimal::prelude::ToPrimitive;
        let r = self.rounded();
        (r.amount * Decimal::from(10i64.pow(self.currency.minor_units()))).to_i64()
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dp = self.currency.minor_units() as usize;
        write!(
            f,
            "{:.dp$} {}",
            self.rounded().amount,
            self.currency.as_str()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn eur(s: &str) -> Money {
        Money::new(
            Decimal::from_str(s).unwrap(),
            Currency::parse("EUR").unwrap(),
        )
    }
    #[test]
    fn exact_addition() {
        assert_eq!(eur("0.1").checked_add(eur("0.2")).unwrap(), eur("0.3"));
    }
    #[test]
    fn currency_mismatch() {
        let usd = Money::new(Decimal::ONE, Currency::parse("USD").unwrap());
        assert_eq!(eur("1").checked_add(usd), Err(MoneyError::CurrencyMismatch));
    }
    #[test]
    fn currency_parse() {
        assert!(Currency::parse("eur").is_none());
        assert!(Currency::parse("EURO").is_none());
        assert_eq!(Currency::parse("JPY").unwrap().minor_units(), 0);
    }
    #[test]
    fn minor_units() {
        assert_eq!(eur("12.345").to_minor_units(), Some(1234));
        assert_eq!(eur("12.355").to_minor_units(), Some(1236));
        assert_eq!(eur("10.5").sub_ok(), "5.50 EUR");
    }
    impl Money {
        fn sub_ok(self) -> String {
            self.checked_sub(eur("5")).unwrap().rounded().to_string()
        }
    }
}
