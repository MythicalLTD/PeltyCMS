//! Admin product license: Free Tier status, Soft overage banner, agreement acceptance.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::license::{self as lic, Status};
use serde::Deserialize;
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/license", get(status))
        .route("/admin/license/accept", post(accept))
        .route("/admin/license/overage-notice", put(overage_notice))
}

#[utoipa::path(get, path = "/api/v1/admin/license", responses((status = 200, body = Status), (status = 403)))]
pub async fn status(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AdminAccess>,
) -> ApiResult<Json<Status>> {
    Ok(Json(lic::status(&s.pool).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AcceptBody {
    /// Must be true; confirms LICENSE + EULA for this deployment.
    pub accept: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/license/accept", request_body = AcceptBody, responses((status = 200, body = Status), (status = 422), (status = 403)))]
pub async fn accept(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AdminAccess>,
    Json(b): Json<AcceptBody>,
) -> ApiResult<Json<Status>> {
    if !b.accept {
        return Err(pelty_core::AppError::Validation("error.validation").into());
    }
    let st = lic::accept(&s.pool, Some(u.user_id), "admin").await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "license.accepted",
        None,
        u.ip.as_deref(),
        json!({ "version": st.version }),
    )
    .await?;
    Ok(Json(st))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OverageBody {
    pub dismissed: bool,
}

#[utoipa::path(put, path = "/api/v1/admin/license/overage-notice", request_body = OverageBody, responses((status = 200, body = Status), (status = 403)))]
pub async fn overage_notice(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<OverageBody>,
) -> ApiResult<Json<Status>> {
    let st = lic::set_overage_dismissed(&s.pool, b.dismissed).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "license.overage_notice",
        None,
        u.ip.as_deref(),
        json!({ "dismissed": b.dismissed }),
    )
    .await?;
    Ok(Json(st))
}
