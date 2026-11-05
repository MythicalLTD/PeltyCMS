//! Day-based proration for mid-period plan changes. Exact decimals only.
use crate::math::round;
use rust_decimal::Decimal;
use serde::Serialize;
use time::Date;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proration {
    pub days_total: i64,
    pub days_remaining: i64,
    /// Unused value of the old plan for the rest of the period.
    pub credit: Decimal,
    /// Cost of the new plan for the rest of the period.
    pub charge: Decimal,
    /// `charge - credit` (negative for a downgrade).
    pub net: Decimal,
}

/// Prorate a change on `today` within the period `[start, end)`. Both
/// amounts are per full period; results are rounded to the currency
/// exponent. `None` when `today` is outside the period.
pub fn prorate(
    old: Decimal,
    new: Decimal,
    start: Date,
    end: Date,
    today: Date,
    exponent: u32,
) -> Option<Proration> {
    if end <= start || today < start || today >= end {
        return None;
    }
    let total = (end - start).whole_days();
    let rem = (end - today).whole_days();
    let (t, r) = (Decimal::from(total), Decimal::from(rem));
    let credit = round(old.checked_mul(r)? / t, exponent);
    let charge = round(new.checked_mul(r)? / t, exponent);
    Some(Proration {
        days_total: total,
        days_remaining: rem,
        credit,
        charge,
        net: charge - credit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    use time::macros::date;

    #[test]
    fn half_month_upgrade() {
        // 30-day period, 15 days left: credit 5.00, charge 10.00.
        let p = prorate(
            d("10"),
            d("20"),
            date!(2026 - 11 - 01),
            date!(2026 - 12 - 01),
            date!(2026 - 11 - 16),
            2,
        )
        .unwrap();
        assert_eq!((p.days_total, p.days_remaining), (30, 15));
        assert_eq!(
            (p.credit, p.charge, p.net),
            (d("5.00"), d("10.00"), d("5.00"))
        );
    }

    #[test]
    fn rounds_exactly_and_handles_downgrade() {
        // 31 days, 10 left: 9.99*10/31 = 3.2225.. -> 3.22 ; 4.99*10/31 = 1.6096.. -> 1.61
        let p = prorate(
            d("9.99"),
            d("4.99"),
            date!(2026 - 10 - 01),
            date!(2026 - 11 - 01),
            date!(2026 - 10 - 22),
            2,
        )
        .unwrap();
        assert_eq!(
            (p.credit, p.charge, p.net),
            (d("3.22"), d("1.61"), d("-1.61"))
        );
        // First day: full period.
        let f = prorate(
            d("12"),
            d("24"),
            date!(2026 - 10 - 01),
            date!(2026 - 11 - 01),
            date!(2026 - 10 - 01),
            2,
        )
        .unwrap();
        assert_eq!(f.net, d("12"));
    }

    #[test]
    fn outside_period_is_none() {
        assert!(prorate(
            d("1"),
            d("2"),
            date!(2026 - 10 - 01),
            date!(2026 - 11 - 01),
            date!(2026 - 11 - 01),
            2
        )
        .is_none());
        assert!(prorate(
            d("1"),
            d("2"),
            date!(2026 - 10 - 01),
            date!(2026 - 11 - 01),
            date!(2026 - 09 - 30),
            2
        )
        .is_none());
    }
}
