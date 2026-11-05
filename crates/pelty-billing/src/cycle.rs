//! Billing cycles (monthly, annually, ...) and date arithmetic for renewals.
use time::{Date, Month};

pub const CYCLES: &[&str] = &[
    "one_time",
    "monthly",
    "quarterly",
    "semiannual",
    "annual",
    "biennial",
    "triennial",
];

/// Months in a billing cycle; `None` for one-time.
pub fn months(cycle: &str) -> Option<u32> {
    Some(match cycle {
        "monthly" => 1,
        "quarterly" => 3,
        "semiannual" => 6,
        "annual" => 12,
        "biennial" => 24,
        "triennial" => 36,
        _ => return None,
    })
}

pub fn is_valid(cycle: &str) -> bool {
    CYCLES.contains(&cycle)
}

/// Add whole months, clamping the day to the target month's length
/// (Jan 31 + 1 month = Feb 28/29).
pub fn add_months(d: Date, n: u32) -> Date {
    let total = d.year() * 12 + (d.month() as i32 - 1) + n as i32;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let month = Month::try_from(m as u8).unwrap_or(Month::January);
    let day = d.day().min(month.length(y));
    Date::from_calendar_date(y, month, day).unwrap_or(d)
}

/// Subtract whole months (same day clamping as [`add_months`]).
pub fn sub_months(d: Date, n: u32) -> Date {
    let total = d.year() * 12 + (d.month() as i32 - 1) - n as i32;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let month = Month::try_from(m as u8).unwrap_or(Month::January);
    let day = d.day().min(month.length(y));
    Date::from_calendar_date(y, month, day).unwrap_or(d)
}

/// The period that starts at `start` for this cycle: `[start, end)`.
pub fn period(start: Date, cycle: &str) -> Option<(Date, Date)> {
    months(cycle).map(|m| (start, add_months(start, m)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn month_math_clamps_day() {
        assert_eq!(add_months(date!(2026 - 01 - 31), 1), date!(2026 - 02 - 28));
        assert_eq!(add_months(date!(2028 - 01 - 31), 1), date!(2028 - 02 - 29));
        assert_eq!(add_months(date!(2026 - 11 - 15), 3), date!(2027 - 02 - 15));
        assert_eq!(
            period(date!(2026 - 10 - 01), "annual"),
            Some((date!(2026 - 10 - 01), date!(2027 - 10 - 01)))
        );
        assert_eq!(period(date!(2026 - 10 - 01), "one_time"), None);
        assert_eq!(sub_months(date!(2026 - 03 - 31), 1), date!(2026 - 02 - 28));
        assert_eq!(sub_months(date!(2026 - 01 - 15), 3), date!(2025 - 10 - 15));
    }
}
