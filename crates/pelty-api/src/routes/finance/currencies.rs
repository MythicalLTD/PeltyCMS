//! Currency administration (`settings.manage`): exchange rates, default
//! currency, and price recalculation from the default currency.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_database::audit;
use pelty_database::currencies::{self as cur, CurrencyInput};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/currencies", get(list).post(create))
        .route("/admin/currencies/{code}", put(update))
        .route("/admin/currencies/{code}/default", post(make_default))
        .route("/admin/currencies/sync-prices", post(sync_prices))
        .route(
            "/admin/currencies/rates/settings",
            get(rate_settings).put(save_rate_settings),
        )
        .route("/admin/currencies/rates/update", post(update_rates))
        .route("/admin/currencies/rates/history", get(rate_history))
}

type Sm = RequirePermission<perms::SettingsManage>;

#[utoipa::path(get, path = "/api/v1/admin/currencies", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: Sm) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": cur::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/currencies", request_body = CurrencyInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): Sm,
    Json(b): Json<CurrencyInput>,
) -> ApiResult<Json<Value>> {
    let code = cur::create(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "currency.created",
        Some(&code),
        u.ip.as_deref(),
        json!({ "rate": b.rate.to_string() }),
    )
    .await?;
    Ok(Json(json!({ "code": code })))
}

#[utoipa::path(put, path = "/api/v1/admin/currencies/{code}", request_body = CurrencyInput, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): Sm,
    Path(code): Path<String>,
    Json(b): Json<CurrencyInput>,
) -> ApiResult<Json<Value>> {
    cur::update(&s.pool, &code, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "currency.updated",
        Some(&code.to_ascii_uppercase()),
        u.ip.as_deref(),
        json!({ "rate": b.rate.to_string(), "enabled": b.enabled }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/currencies/{code}/default", responses((status = 200), (status = 404), (status = 422)))]
pub async fn make_default(
    State(s): State<AppState>,
    RequirePermission(u, _): Sm,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    cur::set_default(&s.pool, &code).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "currency.default_set",
        Some(&code.to_ascii_uppercase()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SyncBody {
    /// Keep prices that already exist in the target currency.
    #[serde(default = "yes")]
    pub only_missing: bool,
}
fn yes() -> bool {
    true
}

#[utoipa::path(post, path = "/api/v1/admin/currencies/sync-prices", request_body = SyncBody, responses((status = 200)))]
pub async fn sync_prices(
    State(s): State<AppState>,
    RequirePermission(u, _): Sm,
    Json(b): Json<SyncBody>,
) -> ApiResult<Json<Value>> {
    let n = cur::sync_prices(&s.pool, b.only_missing).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "currency.prices_synced",
        None,
        u.ip.as_deref(),
        json!({ "rows": n, "only_missing": b.only_missing }),
    )
    .await?;
    Ok(Json(json!({ "updated": n })))
}

#[utoipa::path(get, path = "/api/v1/admin/currencies/rates/settings", responses((status = 200), (status = 403)))]
pub async fn rate_settings(State(s): State<AppState>, _p: Sm) -> ApiResult<Json<Value>> {
    Ok(Json(json!(
        pelty_database::exchange_rates::settings(&s.pool).await?
    )))
}

#[utoipa::path(put, path = "/api/v1/admin/currencies/rates/settings", request_body = pelty_database::exchange_rates::RateSettingsInput, responses((status = 200), (status = 422)))]
pub async fn save_rate_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): Sm,
    Json(b): Json<pelty_database::exchange_rates::RateSettingsInput>,
) -> ApiResult<Json<Value>> {
    pelty_database::exchange_rates::save_settings(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "currency.rate_settings_updated",
        None,
        u.ip.as_deref(),
        json!({ "enabled": b.enabled, "source": b.source, "markup": b.markup_percent.to_string() }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/currencies/rates/update", responses((status = 200), (status = 409), (status = 422)))]
pub async fn update_rates(
    State(s): State<AppState>,
    RequirePermission(u, _): Sm,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        crate::exchange_rates::update_now(&s, Some(u.user_id)).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/currencies/rates/history", responses((status = 200)))]
pub async fn rate_history(State(s): State<AppState>, _p: Sm) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": pelty_database::exchange_rates::history(&s.pool, 200).await? }),
    ))
}
