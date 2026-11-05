//! CSV import (clients, services, invoices): analyze → map columns →
//! dry-run → import.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{DefaultBodyLimit, State};
use axum::routing::post;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::csv_import as ci;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/import/csv/analyze", post(analyze))
        .route("/admin/import/csv/run", post(run))
        .layer(DefaultBodyLimit::max(
            pelty_core::config::upload_limits().import + 64 * 1024,
        ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CsvAnalyze {
    /// clients | services | invoices
    pub kind: String,
    pub csv: String,
}

/// (column spec: name + required, data rows)
type Parsed = (&'static [(&'static str, bool)], Vec<Vec<String>>);

fn rows(kind: &str, csv: &str) -> ApiResult<Parsed> {
    let spec = ci::fields(kind).ok_or(AppError::Validation("error.import_kind"))?;
    if csv.len() > pelty_core::config::upload_limits().import {
        return Err(AppError::Validation("error.import_too_large").into());
    }
    let rows = ci::parse(csv)?;
    if rows.len() < 2 {
        return Err(AppError::Validation("error.import_empty").into());
    }
    Ok((spec, rows))
}

/// Headers, a 5-row preview, the target fields and a suggested mapping.
#[utoipa::path(post, path = "/api/v1/admin/import/csv/analyze", request_body = CsvAnalyze, responses((status = 200), (status = 422)))]
pub async fn analyze(
    _p: RequirePermission<perms::SettingsManage>,
    Json(b): Json<CsvAnalyze>,
) -> ApiResult<Json<Value>> {
    let (spec, rows) = rows(&b.kind, &b.csv)?;
    let fields: Vec<Value> = spec
        .iter()
        .map(|(f, r)| json!({ "key": f, "required": r }))
        .collect();
    Ok(Json(json!({
        "headers": rows[0],
        "preview": rows.iter().skip(1).take(5).collect::<Vec<_>>(),
        "rows": rows.len() - 1,
        "fields": fields,
        "mapping": ci::suggest(&b.kind, &rows[0]),
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CsvRun {
    pub kind: String,
    pub csv: String,
    /// Target field → CSV header.
    pub mapping: HashMap<String, String>,
    #[serde(default = "yes")]
    pub dry_run: bool,
}

fn yes() -> bool {
    true
}

/// Validate every row; unless `dry_run`, import them all-or-nothing.
#[utoipa::path(post, path = "/api/v1/admin/import/csv/run", request_body = CsvRun, responses((status = 200), (status = 422)))]
pub async fn run(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<CsvRun>,
) -> ApiResult<Json<Value>> {
    let (_, rows) = rows(&b.kind, &b.csv)?;
    let rep = ci::run(&s.pool, &b.kind, &rows, &b.mapping, b.dry_run).await?;
    if rep.committed {
        audit::record(
            &s.pool,
            Some(u.user_id),
            "import.csv",
            Some(&b.kind),
            u.ip.as_deref(),
            json!({ "created": rep.created, "skipped": rep.skipped }),
        )
        .await?;
    }
    Ok(Json(serde_json::to_value(&rep).unwrap_or_default()))
}
