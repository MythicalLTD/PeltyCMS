//! On-demand renewals (WHMCS Billing > On-Demand Renewals): a client or
//! staff member invoices the next period of a service ahead of the cron.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::services::EarlyRenewal;
use pelty_extensions::events::DomainEvent;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

pub const KEY: &str = "billing.on_demand_renewals";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/settings/on-demand-renewals",
            get(get_cfg).put(put_cfg),
        )
        .route("/admin/services/{id}/renew-now", post(admin_renew))
        .route("/client/services/{id}/renew-now", post(client_renew))
        .route("/client/renewals/config", get(client_cfg))
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RenewalSettings {
    /// Clients may renew early from their service page.
    #[serde(default)]
    pub enabled: bool,
    /// How many days before the due date (0 = any time).
    #[serde(default)]
    pub max_days_ahead: i64,
}

async fn cfg(s: &AppState) -> ApiResult<RenewalSettings> {
    Ok(pelty_database::settings::get(&s.pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

#[utoipa::path(get, path = "/api/v1/admin/settings/on-demand-renewals", responses((status = 200, body = RenewalSettings)))]
pub async fn get_cfg(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<RenewalSettings>> {
    Ok(Json(cfg(&s).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/on-demand-renewals", request_body = RenewalSettings, responses((status = 200, body = RenewalSettings), (status = 422)))]
pub async fn put_cfg(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<RenewalSettings>,
) -> ApiResult<Json<RenewalSettings>> {
    if !(0..=365).contains(&b.max_days_ahead) {
        return Err(AppError::Validation("error.validation").into());
    }
    pelty_database::settings::set(&s.pool, KEY, json!(b)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.on_demand_renewals",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(b))
}

#[utoipa::path(get, path = "/api/v1/client/renewals/config", responses((status = 200, body = RenewalSettings)))]
pub async fn client_cfg(
    State(s): State<AppState>,
    _u: CurrentUser,
) -> ApiResult<Json<RenewalSettings>> {
    Ok(Json(cfg(&s).await?))
}

fn emitted(s: &AppState, sid: Uuid, r: &EarlyRenewal) {
    s.emit(DomainEvent::new("invoice.created", json!({ "invoice_id": r.invoice_id, "service_id": sid, "renewal": true, "on_demand": true })));
}

#[utoipa::path(post, path = "/api/v1/admin/services/{id}/renew-now", responses((status = 200, body = EarlyRenewal), (status = 404), (status = 409)))]
pub async fn admin_renew(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<EarlyRenewal>> {
    let r = s
        .hosting()
        .renew_now(id, None, 0, OffsetDateTime::now_utc().date())
        .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.renewed_early",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "invoice_id": r.invoice_id }),
    )
    .await?;
    emitted(&s, id, &r);
    Ok(Json(r))
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/renew-now", responses((status = 200, body = EarlyRenewal), (status = 403), (status = 404), (status = 409)))]
pub async fn client_renew(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<EarlyRenewal>> {
    let c = cfg(&s).await?;
    if !c.enabled {
        return Err(AppError::Forbidden.into());
    }
    let cust = customer_of(&s, &u).await?;
    let r = s
        .hosting()
        .renew_now(
            id,
            Some(cust),
            c.max_days_ahead,
            OffsetDateTime::now_utc().date(),
        )
        .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.renewed_early",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "invoice_id": r.invoice_id }),
    )
    .await?;
    emitted(&s, id, &r);
    Ok(Json(r))
}
