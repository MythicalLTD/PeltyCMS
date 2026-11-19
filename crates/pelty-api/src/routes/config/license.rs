//! Admin install notice: GNU AGPL license status and one-time acceptance.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::license::{self as lic, Status};
use serde::Deserialize;
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/license", get(status))
        .route("/admin/license/accept", post(accept))
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
    /// Must be true; confirms the license notice (LICENSE + EULA.md) for this deployment.
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
