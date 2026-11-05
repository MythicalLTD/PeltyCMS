//! Admin overdue automation settings and a manual run.
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, overdue::{self, OverdueSettings}};
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::{error::ApiResult, extract::{perms, RequirePermission}, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/settings/overdue", get(get_settings).put(put_settings))
        .route("/admin/overdue/run", post(run_now))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/overdue", responses((status = 200, body = OverdueSettings), (status = 403)))]
pub async fn get_settings(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<OverdueSettings>> {
    Ok(Json(overdue::get_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/overdue", request_body = OverdueSettings, responses((status = 200, body = OverdueSettings), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<OverdueSettings>,
) -> ApiResult<Json<OverdueSettings>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    overdue::put_settings(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.overdue", None, u.ip.as_deref(), json!(b)).await?;
    Ok(Json(b))
}

/// Run today's overdue automation now (idempotent with the daily job).
#[utoipa::path(post, path = "/api/v1/admin/overdue/run", responses((status = 200), (status = 403)))]
pub async fn run_now(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    let r = crate::overdue::run(&s, OffsetDateTime::now_utc().date()).await?;
    audit::record(&s.pool, Some(u.user_id), "overdue.run", None, u.ip.as_deref(), r.clone()).await?;
    Ok(Json(r))
}
