//! Exact invoice math with rust_decimal. Rounding: half away from zero
//! (commercial rounding) to the currency exponent, per line, then summed, //! so the printed lines always add up to the printed total.
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;

pub fn round(v: Decimal, exponent: u32) -> Decimal {
    v.round_dp_with_strategy(exponent, RoundingStrategy::MidpointAwayFromZero)
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineInput {
    pub quantity: Decimal,
    pub unit_price: Decimal,
    /// Percent, e.g. 19 for 19 %.
    pub tax_rate: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    pub amount: Decimal,
    pub tax_amount: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Totals {
    pub subtotal: Decimal,
    pub tax_total: Decimal,
    pub total: Decimal,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MathError {
    Overflow,
    InvalidQuantity,
    InvalidTaxRate,
}

pub fn line(i: &LineInput, exponent: u32) -> Result<Line, MathError> {
    if i.quantity <= Decimal::ZERO {
        return Err(MathError::InvalidQuantity);
    }
    if i.tax_rate < Decimal::ZERO || i.tax_rate > Decimal::ONE_HUNDRED {
        return Err(MathError::InvalidTaxRate);
    }
    let amount = round(
        i.quantity
            .checked_mul(i.unit_price)
            .ok_or(MathError::Overflow)?,
        exponent,
    );
    let tax = amount.checked_mul(i.tax_rate).ok_or(MathError::Overflow)? / Decimal::ONE_HUNDRED;
    Ok(Line {
        amount,
        tax_amount: round(tax, exponent),
    })
}

pub fn totals(lines: &[Line]) -> Result<Totals, MathError> {
    let mut subtotal = Decimal::ZERO;
    let mut tax_total = Decimal::ZERO;
    for l in lines {
        subtotal = subtotal.checked_add(l.amount).ok_or(MathError::Overflow)?;
        tax_total = tax_total
            .checked_add(l.tax_amount)
            .ok_or(MathError::Overflow)?;
    }
    let total = subtotal.checked_add(tax_total).ok_or(MathError::Overflow)?;
    Ok(Totals {
        subtotal,
        tax_total,
        total,
    })
}

/// Parse a money amount from an API string; rejects more decimals than the
/// currency allows, negatives, and anything that is not a plain number.
pub fn parse_amount(s: &str, exponent: u32) -> Option<Decimal> {
    let s = s.trim();
    if s.is_empty() || s.len() > 24 || !s.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let d: Decimal = s.parse().ok()?;
    (d.scale() <= exponent && d >= Decimal::ZERO).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn rounds_per_currency_exponent() {
        let l = line(
            &LineInput {
                quantity: d("3"),
                unit_price: d("0.335"),
                tax_rate: d("19"),
            },
            2,
        )
        .unwrap();
        assert_eq!(l.amount, d("1.01")); // 1.005 -> 1.01 (away from zero)
        assert_eq!(l.tax_amount, d("0.19")); // 0.1919
        let jpy = line(
            &LineInput {
                quantity: d("1"),
                unit_price: d("1234.5"),
                tax_rate: d("10"),
            },
            0,
        )
        .unwrap();
        assert_eq!(jpy.amount, d("1235"));
        assert_eq!(jpy.tax_amount, d("124")); // 123.5 -> 124
        let kwd = line(
            &LineInput {
                quantity: d("2"),
                unit_price: d("1.2345"),
                tax_rate: d("0"),
            },
            3,
        )
        .unwrap();
        assert_eq!(kwd.amount, d("2.469"));
    }

    #[test]
    fn floats_would_fail_here() {
        // 0.1 + 0.2 must be exactly 0.3 and lines must sum to the total.
        let ls: Vec<Line> = ["0.1", "0.2"]
            .iter()
            .map(|p| {
                line(
                    &LineInput {
                        quantity: d("1"),
                        unit_price: d(p),
                        tax_rate: d("0"),
                    },
                    2,
                )
                .unwrap()
            })
            .collect();
        assert_eq!(totals(&ls).unwrap().total, d("0.3"));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            line(
                &LineInput {
                    quantity: d("0"),
                    unit_price: d("1"),
                    tax_rate: d("0")
                },
                2
            ),
            Err(MathError::InvalidQuantity)
        );
        assert_eq!(
            line(
                &LineInput {
                    quantity: d("1"),
                    unit_price: d("1"),
                    tax_rate: d("101")
                },
                2
            ),
            Err(MathError::InvalidTaxRate)
        );
        assert_eq!(parse_amount("10.50", 2), Some(d("10.50")));
        assert_eq!(parse_amount("10.505", 2), None);
        assert_eq!(parse_amount("-1", 2), None);
        assert_eq!(parse_amount("1e3", 2), None);
        assert_eq!(parse_amount("5", 0), Some(d("5")));
    }
}
