//! First-run setup wizard / getting-started checklist.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::setup_wizard::{self as sw, Wizard};
use serde::Deserialize;
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/setup-wizard", get(status)).route("/admin/setup-wizard/dismissed", put(dismiss))
}

#[utoipa::path(get, path = "/api/v1/admin/setup-wizard", responses((status = 200, body = Wizard), (status = 403)))]
pub async fn status(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Wizard>> {
    Ok(Json(sw::status(&s.pool).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DismissBody {
    pub dismissed: bool,
}

#[utoipa::path(put, path = "/api/v1/admin/setup-wizard/dismissed", request_body = DismissBody, responses((status = 200, body = Wizard)))]
pub async fn dismiss(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<DismissBody>) -> ApiResult<Json<Wizard>> {
    sw::set_dismissed(&s.pool, b.dismissed).await?;
    audit::record(&s.pool, Some(u.user_id), "setup_wizard.dismissed", None, u.ip.as_deref(), json!({ "dismissed": b.dismissed })).await?;
    Ok(Json(sw::status(&s.pool).await?))
}
