//! Registrar TLD price sync + margin rules, premium-name settings and
//! per-TLD additional order fields.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::domain_pricing::{self as dp, Cost, MarginRule, PremiumSettings, SyncLine, TldField};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/domains/margins", get(margins).put(set_margins))
        .route("/admin/domains/sync-prices", post(sync_prices))
        .route("/admin/domains/costs", get(costs))
        .route("/admin/domains/premium-settings", get(premium).put(set_premium))
        .route("/admin/domains/tld-fields", get(all_fields))
        .route("/admin/domains/tld-fields/{tld}", put(set_fields))
        .route("/client/domains/fields", get(client_fields))
}

#[utoipa::path(get, path = "/api/v1/admin/domains/margins", responses((status = 200, body = Vec<MarginRule>)))]
pub async fn margins(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Vec<MarginRule>>> {
    Ok(Json(dp::margin_rules(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/domains/margins", request_body = Vec<MarginRule>, responses((status = 200, body = Vec<MarginRule>), (status = 422)))]
pub async fn set_margins(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<Vec<MarginRule>>) -> ApiResult<Json<Vec<MarginRule>>> {
    dp::set_margin_rules(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "domains.margins_updated", None, u.ip.as_deref(), json!({ "rules": b.len() })).await?;
    Ok(Json(dp::margin_rules(&s.pool).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SyncBody {
    pub registrar: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/domains/sync-prices", request_body = SyncBody, responses((status = 200), (status = 422)))]
pub async fn sync_prices(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<SyncBody>) -> ApiResult<Json<Value>> {
    let reg = crate::addons::registrar(&s, &b.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
    let list = reg.tld_prices().await.map_err(|_| AppError::Validation("error.registrar_unavailable"))?.ok_or(AppError::Validation("error.registrar_no_price_sync"))?;
    let mut parsed = Vec::new();
    let mut invalid = 0usize;
    for c in &list {
        match Cost::parse(&c.tld, &c.currency, &c.register, &c.renew, &c.transfer) {
            Some(x) => parsed.push(x),
            None => invalid += 1,
        }
    }
    let lines: Vec<SyncLine> = dp::apply_costs(&s.pool, &b.registrar, &parsed, b.dry_run).await?;
    let updated = lines.iter().filter(|l| l.action == "updated").count();
    if !b.dry_run {
        audit::record(&s.pool, Some(u.user_id), "domains.prices_synced", Some(&b.registrar), u.ip.as_deref(), json!({ "updated": updated, "lines": lines.len(), "invalid": invalid })).await?;
    }
    Ok(Json(json!({ "dry_run": b.dry_run, "updated": updated, "invalid": invalid, "lines": lines })))
}

#[utoipa::path(get, path = "/api/v1/admin/domains/costs", responses((status = 200)))]
pub async fn costs(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": dp::cost_rows(&s.pool).await? })))
}

#[utoipa::path(get, path = "/api/v1/admin/domains/premium-settings", responses((status = 200, body = PremiumSettings)))]
pub async fn premium(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<PremiumSettings>> {
    Ok(Json(dp::premium_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/domains/premium-settings", request_body = PremiumSettings, responses((status = 200, body = PremiumSettings), (status = 422)))]
pub async fn set_premium(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<PremiumSettings>) -> ApiResult<Json<PremiumSettings>> {
    dp::set_premium_settings(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "domains.premium_settings", None, u.ip.as_deref(), json!(b)).await?;
    Ok(Json(b))
}

#[utoipa::path(get, path = "/api/v1/admin/domains/tld-fields", responses((status = 200, body = Vec<TldField>)))]
pub async fn all_fields(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Vec<TldField>>> {
    Ok(Json(dp::all_tld_fields(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/domains/tld-fields/{tld}", request_body = Vec<TldField>, responses((status = 200, body = Vec<TldField>), (status = 422)))]
pub async fn set_fields(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Path(tld): Path<String>, Json(b): Json<Vec<TldField>>) -> ApiResult<Json<Vec<TldField>>> {
    let tld = tld.trim_start_matches('.').to_ascii_lowercase();
    let out = dp::set_tld_fields(&s.pool, &tld, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "domains.tld_fields_updated", Some(&tld), u.ip.as_deref(), json!({ "fields": out.len() })).await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct FieldsQuery {
    pub tld: String,
}

#[utoipa::path(get, path = "/api/v1/client/domains/fields", params(FieldsQuery), responses((status = 200, body = Vec<TldField>)))]
pub async fn client_fields(State(s): State<AppState>, _u: CurrentUser, Query(q): Query<FieldsQuery>) -> ApiResult<Json<Vec<TldField>>> {
    Ok(Json(dp::tld_fields(&s.pool, &q.tld.trim_start_matches('.').to_ascii_lowercase()).await?))
}
