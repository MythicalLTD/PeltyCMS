//! Admin financial reports: revenue, refunds, MRR and churn by month.
use axum::extract::{Query, State};
use axum::{routing::get, Json, Router};
use pelty_core::AppError;
use pelty_database::reports::{self, Report};
use serde::Deserialize;
use time::{Date, Month, OffsetDateTime};

use crate::{
    error::ApiResult,
    extract::{perms, RequirePermission},
    AppState,
};

/// Longest range one request may cover.
pub const MAX_MONTHS: i32 = 36;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/reports", get(get_report))
        .route("/admin/reports/library", get(library))
        .route("/admin/reports/library/{key}", get(run_report))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ReportQuery {
    /// First month, `YYYY-MM` (default: 11 months before `to`).
    pub from: Option<String>,
    /// Last month, `YYYY-MM` (default: current month).
    pub to: Option<String>,
}

fn parse_month(s: &str) -> Option<Date> {
    let (y, m) = s.trim().split_once('-')?;
    if y.len() != 4 || m.len() != 2 {
        return None;
    }
    let y: i32 = y.parse().ok()?;
    let m: u8 = m.parse().ok()?;
    if !(1970..=9999).contains(&y) {
        return None;
    }
    Date::from_calendar_date(y, Month::try_from(m).ok()?, 1).ok()
}

fn index(d: Date) -> i32 {
    d.year() * 12 + i32::from(u8::from(d.month())) - 1
}

fn from_index(i: i32) -> Option<Date> {
    let m = u8::try_from(i.rem_euclid(12) + 1).ok()?;
    Date::from_calendar_date(i.div_euclid(12), Month::try_from(m).ok()?, 1).ok()
}

pub fn range(q: &ReportQuery, today: Date) -> Result<(Date, Date), AppError> {
    let bad = || AppError::Validation("error.invalid_range");
    let to = match &q.to {
        Some(t) => parse_month(t).ok_or_else(bad)?,
        None => today.replace_day(1).map_err(|_| bad())?,
    };
    let from = match &q.from {
        Some(f) => parse_month(f).ok_or_else(bad)?,
        None => from_index(index(to) - 11).ok_or_else(bad)?,
    };
    let span = index(to) - index(from) + 1;
    if !(1..=MAX_MONTHS).contains(&span) {
        return Err(bad());
    }
    Ok((from, to))
}

#[utoipa::path(get, path = "/api/v1/admin/reports", params(ReportQuery), responses((status = 200, body = Report), (status = 403), (status = 422)))]
pub async fn get_report(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ReportsRead>,
    Query(q): Query<ReportQuery>,
) -> ApiResult<Json<Report>> {
    let (from, to) = range(&q, OffsetDateTime::now_utc().date())?;
    Ok(Json(reports::build(&s.pool, from, to).await?))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct LibraryQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    /// `json` (default) or `csv`.
    pub format: Option<String>,
}

/// Available reports grouped by category.
#[utoipa::path(get, path = "/api/v1/admin/reports/library", responses((status = 200), (status = 403)))]
pub async fn library(_p: RequirePermission<perms::ReportsRead>) -> Json<serde_json::Value> {
    let items: Vec<_> = pelty_database::report_library::REPORTS.iter()
        .map(|d| serde_json::json!({ "key": d.key, "category": d.category, "columns": d.columns, "chart": d.chart }))
        .collect();
    Json(serde_json::json!({ "items": items }))
}

/// Run one report over whole months `[from, to]` as JSON or CSV.
#[utoipa::path(get, path = "/api/v1/admin/reports/library/{key}", params(LibraryQuery), responses((status = 200), (status = 403), (status = 404), (status = 422)))]
pub async fn run_report(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ReportsRead>,
    axum::extract::Path(key): axum::extract::Path<String>,
    Query(q): Query<LibraryQuery>,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let (from, to) = range(
        &ReportQuery {
            from: q.from.clone(),
            to: q.to.clone(),
        },
        OffsetDateTime::now_utc().date(),
    )?;
    let end = from_index(index(to) + 1).ok_or(AppError::Validation("error.invalid_range"))?;
    let t = pelty_database::report_library::run(
        &s.pool,
        &key,
        from.midnight().assume_utc(),
        end.midnight().assume_utc(),
    )
    .await?;
    match q.format.as_deref() {
        None | Some("json") => Ok(Json(t).into_response()),
        Some("csv") => Ok((
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "text/csv; charset=utf-8".to_string(),
                ),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{key}-{from}-{to}.csv\""),
                ),
                (axum::http::header::CACHE_CONTROL, "no-store".to_string()),
            ],
            format!("\u{feff}{}", pelty_database::report_library::to_csv(&t)),
        )
            .into_response()),
        Some(_) => Err(AppError::Validation("error.validation").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;
    fn q(f: Option<&str>, t: Option<&str>) -> ReportQuery {
        ReportQuery {
            from: f.map(Into::into),
            to: t.map(Into::into),
        }
    }
    #[test]
    fn ranges() {
        let today = date!(2026 - 10 - 29);
        assert_eq!(
            range(&q(None, None), today).unwrap(),
            (date!(2025 - 11 - 01), date!(2026 - 10 - 01))
        );
        assert_eq!(
            range(&q(Some("2026-01"), Some("2026-01")), today)
                .unwrap()
                .0,
            date!(2026 - 01 - 01)
        );
        assert!(range(&q(Some("2026-02"), Some("2026-01")), today).is_err());
        assert!(range(&q(Some("2020-01"), Some("2026-01")), today).is_err());
        assert!(range(&q(Some("2026-13"), None), today).is_err());
        assert!(range(&q(Some("26-01"), None), today).is_err());
    }
}
