//! Admin-editable system settings (non-secret).
use axum::{extract::State, routing::get, Json, Router};
use pelty_core::AppError;
use pelty_database::{
    audit,
    settings::{self, BankTransfer},
};

use crate::{
    error::ApiResult,
    extract::{perms, RequirePermission},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/settings/bank-transfer", get(get_bank).put(put_bank))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/bank-transfer", responses((status = 200, body = BankTransfer), (status = 403)))]
pub async fn get_bank(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<BankTransfer>> {
    Ok(Json(settings::bank_transfer(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/bank-transfer", request_body = BankTransfer, responses((status = 200, body = BankTransfer), (status = 422)))]
pub async fn put_bank(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<BankTransfer>,
) -> ApiResult<Json<BankTransfer>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    settings::set(
        &s.pool,
        settings::BANK_TRANSFER_KEY,
        serde_json::to_value(&b).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.bank_transfer",
        None,
        u.ip.as_deref(),
        serde_json::json!({}),
    )
    .await?;
    Ok(Json(b))
}
