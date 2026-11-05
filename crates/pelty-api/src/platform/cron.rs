//! Cron expressions for scheduled tasks (built-in and `<addon>:<op>`).
//!
//! Five fields (`minute hour day-of-month month day-of-week`), evaluated in
//! the company time zone (Admin > General > Localisation; the offset comes
//! from [`crate::routes::automation::local_now`]). A task with a schedule is
//! queued once per matching minute (dedupe key `cron:<task>:<unix>`); a task
//! without one keeps its built-in interval and start hour.
use croner::Cron;
use std::str::FromStr;
use time::OffsetDateTime;

/// Parse and validate a 5-field expression.
pub fn parse(expr: &str) -> Result<Cron, String> {
    let expr = expr.trim();
    if expr.split_whitespace().count() != 5 {
        return Err("expected 5 fields: minute hour day-of-month month day-of-week".into());
    }
    Cron::from_str(expr).map_err(|e| e.to_string())
}

fn to_chrono(t: OffsetDateTime) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let off = chrono::FixedOffset::east_opt(t.offset().whole_seconds())?;
    chrono::DateTime::from_timestamp(t.unix_timestamp(), 0).map(|d| d.with_timezone(&off))
}

/// The next `n` run times (unix seconds) strictly after `now` (company time).
pub fn next_runs(cron: &Cron, now: OffsetDateTime, n: usize) -> Vec<i64> {
    let Some(start) = to_chrono(now) else { return vec![] };
    cron.iter_after(start).take(n).map(|d| d.timestamp()).collect()
}

/// The latest run time at or before `now` (company time).
pub fn prev_run(cron: &Cron, now: OffsetDateTime) -> Option<i64> {
    cron.find_previous_occurrence(&to_chrono(now)?, true).ok().map(|d| d.timestamp())
}

/// English description of an expression ("At minute 0, at hour 3 ...").
pub fn describe(cron: &Cron) -> String {
    cron.describe()
}

/// The expression equivalent to a legacy interval and start hour, when one
/// exists (5 min -> `*/5 * * * *`, hourly -> `0 * * * *`, daily -> `0 H * * *`).
pub fn from_interval(secs: i64, run_hour: Option<i16>) -> Option<String> {
    match secs {
        86_400 => Some(format!("0 {} * * *", run_hour.unwrap_or(0))),
        s if s >= 3_600 && s % 3_600 == 0 && 24 % (s / 3_600) == 0 => Some(if s == 3_600 { "0 * * * *".into() } else { format!("0 */{} * * *", s / 3_600) }),
        s if (60..3_600).contains(&s) && s % 60 == 0 && 60 % (s / 60) == 0 => Some(if s == 60 { "* * * * *".into() } else { format!("*/{} * * * *", s / 60) }),
        _ => None,
    }
}

/// Whether a scheduled task is due: its latest run time is no older than a
/// day and not before the schedule was saved (no catch-up of old runs).
pub fn due(cron: &Cron, now: OffsetDateTime, saved_at: i64) -> Option<i64> {
    prev_run(cron, now).filter(|&t| t >= saved_at && t > now.unix_timestamp() - 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_next_in_offset() {
        assert!(parse("0 3 * * *").is_ok());
        assert!(parse("0 0 3 * * *").is_err(), "seconds field rejected");
        assert!(parse("61 * * * *").is_err());
        let c = parse("0 3 * * *").unwrap();
        // 2026-11-01 12:00 UTC == 13:00 at +01:00; next 03:00 local is 02:00 UTC on 11-02.
        let now = OffsetDateTime::from_unix_timestamp(1_793_534_400).unwrap().to_offset(time::UtcOffset::from_hms(1, 0, 0).unwrap());
        let n = next_runs(&c, now, 2);
        assert_eq!(n[0], 1_793_584_800);
        assert_eq!(n[1] - n[0], 86_400);
        assert_eq!(prev_run(&c, now), Some(1_793_584_800 - 86_400));
        assert_eq!(due(&c, now, 0), Some(1_793_584_800 - 86_400));
        assert_eq!(due(&c, now, now.unix_timestamp()), None);
    }

    #[test]
    fn legacy_intervals() {
        assert_eq!(from_interval(300, None).as_deref(), Some("*/5 * * * *"));
        assert_eq!(from_interval(3_600, None).as_deref(), Some("0 * * * *"));
        assert_eq!(from_interval(86_400, Some(4)).as_deref(), Some("0 4 * * *"));
        assert_eq!(from_interval(7_200, None).as_deref(), Some("0 */2 * * *"));
        assert_eq!(from_interval(90, None), None);
        for s in [60, 300, 3_600, 21_600, 86_400] {
            assert!(parse(&from_interval(s, None).unwrap()).is_ok());
        }
    }
}
