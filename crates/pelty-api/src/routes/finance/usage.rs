//! Metered billing: metric catalog (`catalog.manage`), usage ingestion and
//! period history (`services.manage`, API keys work too), client view.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::usage::{self, MetricInput};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub const BILL_JOB: &str = "usage.bill";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/usage-metrics", get(metrics).post(metric_create))
        .route(
            "/admin/usage-metrics/{id}",
            put(metric_update).delete(metric_delete),
        )
        .route("/admin/services/{id}/usage", get(admin_usage).post(record))
        .route("/admin/usage/bill", post(bill_now))
        .route("/admin/usage/sample", post(sample_now))
        .route("/client/services/{id}/usage", get(client_usage))
}

fn validate(m: &MetricInput) -> ApiResult<()> {
    if !m.source_key.is_empty()
        && (m.source_key.len() > 80
            || !m
                .source_key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
    {
        return Err(AppError::Validation("error.validation").into());
    }
    if m.source_divisor <= Decimal::ZERO || !["sum", "max"].contains(&m.aggregate.as_str()) {
        return Err(AppError::Validation("error.validation").into());
    }
    if m.unit_price.is_sign_negative()
        || m.included_quantity.is_sign_negative()
        || m.unit_price.scale() > 8
        || m.included_quantity.scale() > 6
    {
        return Err(AppError::Validation("error.invalid_amount").into());
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/admin/usage-metrics", responses((status = 200)))]
pub async fn metrics(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": usage::metrics(&s.pool, None).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/usage-metrics", request_body = MetricInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn metric_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<MetricInput>,
) -> ApiResult<Json<usage::Metric>> {
    validate(&b)?;
    let m = usage::create_metric(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "usage_metric.created",
        Some(&m.id.to_string()),
        u.ip.as_deref(),
        json!({ "key": m.key }),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(put, path = "/api/v1/admin/usage-metrics/{id}", request_body = MetricInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn metric_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<MetricInput>,
) -> ApiResult<Json<usage::Metric>> {
    validate(&b)?;
    let m = usage::update_metric(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "usage_metric.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(m))
}

#[utoipa::path(delete, path = "/api/v1/admin/usage-metrics/{id}", responses((status = 200), (status = 409)))]
pub async fn metric_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    usage::delete_metric(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "usage_metric.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RecordBody {
    pub metric: String,
    #[schema(value_type = String)]
    pub quantity: Decimal,
    /// Defaults to now; RFC 3339.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub recorded_at: Option<OffsetDateTime>,
    pub idempotency_key: String,
}

#[utoipa::path(post, path = "/api/v1/admin/services/{id}/usage", request_body = RecordBody, responses((status = 200), (status = 422)))]
pub async fn record(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RecordBody>,
) -> ApiResult<Json<Value>> {
    let now = OffsetDateTime::now_utc();
    let at = b.recorded_at.unwrap_or(now);
    if at > now + time::Duration::minutes(5) {
        return Err(AppError::Validation("error.usage_in_future").into());
    }
    let r = usage::record(
        &s.pool,
        id,
        b.metric.trim(),
        b.quantity,
        at,
        &b.idempotency_key,
    )
    .await?;
    Ok(Json(
        json!({ "id": r.id, "duplicate": r.duplicate, "period": r.period }),
    ))
}

async fn history(s: &AppState, service: Uuid) -> ApiResult<Json<Value>> {
    let mut out = Vec::new();
    for p in usage::periods(&s.pool, service).await? {
        let lines: Vec<Value> = usage::period_usage(&s.pool, p.id)
            .await?
            .into_iter()
            .map(|u| json!({ "key": u.key, "name": u.name, "unit": u.unit, "quantity": u.quantity, "included": u.included_quantity, "billable": u.billable(), "unit_price": u.unit_price, "charge": u.charge() }))
            .collect();
        out.push(json!({ "period": p, "metrics": lines }));
    }
    Ok(Json(json!({ "items": out })))
}

#[utoipa::path(get, path = "/api/v1/admin/services/{id}/usage", responses((status = 200)))]
pub async fn admin_usage(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    history(&s, id).await
}

#[utoipa::path(get, path = "/api/v1/client/services/{id}/usage", responses((status = 200), (status = 404)))]
pub async fn client_usage(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let svc = s
        .hosting()
        .get(id)
        .await?
        .filter(|x| x.customer_id == c)
        .ok_or(AppError::NotFound)?;
    history(&s, svc.id).await
}

#[utoipa::path(post, path = "/api/v1/admin/usage/bill", responses((status = 200)))]
pub async fn bill_now(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
) -> ApiResult<Json<Value>> {
    let n = usage::bill_due(&s.pool, OffsetDateTime::now_utc().date()).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "usage.billed",
        None,
        u.ip.as_deref(),
        json!({ "invoices": n }),
    )
    .await?;
    Ok(Json(json!({ "invoices": n })))
}

/// Poll the server module's usage for every service with module-sourced
/// metrics and record hourly samples (runs before each billing pass).
pub async fn sample_all(s: &AppState) -> ApiResult<Value> {
    let now = OffsetDateTime::now_utc();
    let (mut services_n, mut recorded, mut failed) = (0u32, 0u32, 0u32);
    for sid in usage::services_with_module_metrics(&s.pool, 5000).await? {
        services_n += 1;
        match crate::routes::modules_ext::usage_of(s, sid).await {
            Some(u) => recorded += usage::record_samples(&s.pool, sid, &u, now).await?.len() as u32,
            None => failed += 1,
        }
    }
    Ok(json!({ "services": services_n, "recorded": recorded, "unavailable": failed }))
}

#[utoipa::path(post, path = "/api/v1/admin/usage/sample", responses((status = 200)))]
pub async fn sample_now(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
) -> ApiResult<Json<Value>> {
    let r = sample_all(&s).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "usage.sampled",
        None,
        u.ip.as_deref(),
        r.clone(),
    )
    .await?;
    Ok(Json(r))
}
