//! Saved payment methods, the auto-pay toggle and auto-pay settings.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::autopay::{self, AutopaySettings, NewMethod, PaymentMethod};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/client/billing/payment-methods",
            get(client_list).post(client_add),
        )
        .route(
            "/client/billing/payment-methods/{id}",
            delete(client_remove),
        )
        .route(
            "/client/billing/payment-methods/{id}/default",
            post(client_default),
        )
        .route("/client/billing/auto-pay", put(client_auto_pay))
        .route("/admin/customers/{id}/payment-methods", get(admin_list))
        .route("/admin/customers/{id}/auto-pay", put(admin_auto_pay))
        .route(
            "/admin/settings/autopay",
            get(get_settings).put(put_settings),
        )
        .route("/admin/autopay/run", post(run_now))
}

async fn overview(s: &AppState, cid: Uuid) -> ApiResult<Value> {
    let methods: Vec<PaymentMethod> = autopay::methods(&s.pool, cid).await?;
    let mut gateways = vec![];
    for d in s
        .gateways
        .descriptors()
        .into_iter()
        .filter(|d| d.capabilities.recurring)
    {
        if pelty_database::extensions::gateway(&s.pool, d.id)
            .await?
            .is_some_and(|r| r.enabled)
            && crate::addons::gateway_enabled(s, d.id).await?
        {
            gateways.push(d.id);
        }
    }
    gateways.sort_unstable();
    Ok(json!({
        "methods": methods,
        "auto_pay": autopay::auto_pay(&s.pool, cid).await?,
        "gateways": gateways,
        "attempts": autopay::attempts(&s.pool, cid, 20).await?,
    }))
}

#[utoipa::path(get, path = "/api/v1/client/billing/payment-methods", responses((status = 200), (status = 401)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    Ok(Json(overview(&s, cid).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddBody {
    pub gateway: String,
    /// Token created client-side by the provider SDK (e.g. Stripe `pm_…`).
    pub token: String,
}

#[utoipa::path(post, path = "/api/v1/client/billing/payment-methods", request_body = AddBody, responses((status = 200, body = PaymentMethod), (status = 400), (status = 422)))]
pub async fn client_add(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<AddBody>,
) -> ApiResult<Json<PaymentMethod>> {
    let cid = customer_of(&s, &u).await?;
    if b.token.is_empty() || b.token.len() > 255 {
        return Err(AppError::Validation("error.invalid_token").into());
    }
    let gw = crate::autopay::recurring_gateway(&s, &b.gateway).await?;
    let email = s
        .clients()
        .get(cid)
        .await?
        .map(|c| c.email)
        .unwrap_or_default();
    let saved = gw
        .save_method(&b.token, &email, &format!("pm:{cid}:{}", b.token))
        .await
        .map_err(|e| {
            tracing::warn!(gateway = %b.gateway, error = %e, "save_method failed");
            AppError::BadRequest("error.payment_method_rejected")
        })?;
    let m = autopay::add_method(
        &s.pool,
        cid,
        &NewMethod {
            gateway: &b.gateway,
            token: &saved.token,
            brand: &saved.brand,
            last4: &saved.last4,
            exp_month: i16::from(saved.exp_month),
            exp_year: i16::try_from(saved.exp_year).unwrap_or(0),
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "payment_method.added",
        Some(&m.id.to_string()),
        u.ip.as_deref(),
        json!({ "gateway": m.gateway, "last4": m.last4 }),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(delete, path = "/api/v1/client/billing/payment-methods/{id}", responses((status = 200), (status = 404)))]
pub async fn client_remove(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    autopay::remove(&s.pool, cid, id).await?;
    // Auto-pay without any method left would silently do nothing: turn it off.
    if autopay::methods(&s.pool, cid).await?.is_empty() {
        autopay::set_auto_pay(&s.pool, cid, false).await?;
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "payment_method.removed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(overview(&s, cid).await?))
}

#[utoipa::path(post, path = "/api/v1/client/billing/payment-methods/{id}/default", responses((status = 200), (status = 404)))]
pub async fn client_default(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    autopay::set_default(&s.pool, cid, id).await?;
    Ok(Json(overview(&s, cid).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoPayBody {
    pub enabled: bool,
}

#[utoipa::path(put, path = "/api/v1/client/billing/auto-pay", request_body = AutoPayBody, responses((status = 200), (status = 409)))]
pub async fn client_auto_pay(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<AutoPayBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    autopay::set_auto_pay(&s.pool, cid, b.enabled).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.auto_pay",
        Some(&cid.to_string()),
        u.ip.as_deref(),
        json!({ "enabled": b.enabled }),
    )
    .await?;
    Ok(Json(overview(&s, cid).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/payment-methods", responses((status = 200), (status = 403), (status = 404)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(overview(&s, id).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/customers/{id}/auto-pay", request_body = AutoPayBody, responses((status = 200), (status = 409)))]
pub async fn admin_auto_pay(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<AutoPayBody>,
) -> ApiResult<Json<Value>> {
    autopay::set_auto_pay(&s.pool, id, b.enabled).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.auto_pay",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "enabled": b.enabled }),
    )
    .await?;
    Ok(Json(overview(&s, id).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/autopay", responses((status = 200, body = AutopaySettings), (status = 403)))]
pub async fn get_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<AutopaySettings>> {
    Ok(Json(autopay::get_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/autopay", request_body = AutopaySettings, responses((status = 200, body = AutopaySettings), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<AutopaySettings>,
) -> ApiResult<Json<AutopaySettings>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    autopay::put_settings(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.autopay",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(b))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    /// Run as of this date (YYYY-MM-DD); defaults to today.
    #[schema(value_type = Option<String>)]
    pub date: Option<time::Date>,
}

/// Run auto-pay now (idempotent with the daily job).
#[utoipa::path(post, path = "/api/v1/admin/autopay/run", request_body = RunBody, responses((status = 200), (status = 403)))]
pub async fn run_now(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    b: Option<Json<RunBody>>,
) -> ApiResult<Json<Value>> {
    let date = b
        .and_then(|b| b.0.date)
        .unwrap_or_else(|| OffsetDateTime::now_utc().date());
    let r = crate::autopay::run(&s, date).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "autopay.run",
        None,
        u.ip.as_deref(),
        r.clone(),
    )
    .await?;
    Ok(Json(r))
}
