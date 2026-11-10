//! Creator codes (module `creator_codes`): affiliates pick a vanity code,
//! buyers enter it at checkout, the order is credited to the affiliate and
//! may get an admin-set first-invoice discount.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::creator_codes::{self as cc, CreatorCode, CreatorCodeUpdate};
use pelty_database::orders::{AddonDiscount, CartItem};
use pelty_database::{affiliates, audit, feature_modules};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/creator-codes/{code}", get(public_check))
        .route("/client/creator-code", get(client_get).put(client_set))
        .route("/admin/creator-codes", get(admin_list))
        .route("/admin/creator-codes/{id}", put(admin_update))
}

/// The code a checkout should credit: `None` when none was entered or the
/// module is off; a bad or own code is a 422 so the buyer can fix it.
pub async fn resolve(
    s: &AppState,
    customer: Uuid,
    code: Option<&str>,
) -> ApiResult<Option<CreatorCode>> {
    let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    if !feature_modules::is_enabled(&s.pool, cc::MODULE).await? {
        return Ok(None);
    }
    Ok(Some(cc::check(&s.pool, code, Some(customer)).await?))
}

/// First-invoice discounts for a code with a discount, or none.
/// `place_full` re-quotes and clamps them after every earlier discount.
pub async fn checkout_discounts(
    s: &AppState,
    currency: &str,
    items: &[CartItem],
    code: &CreatorCode,
) -> ApiResult<Vec<AddonDiscount>> {
    if code.discount_percent <= Decimal::ZERO || items.is_empty() {
        return Ok(vec![]);
    }
    let Ok(quotes) = s.orders().line_amounts(currency, items).await else {
        return Ok(vec![]);
    };
    let pct = code.discount_percent;
    Ok(quotes
        .iter()
        .enumerate()
        .map(|(item, q)| AddonDiscount {
            item,
            amount: q.amount * pct / Decimal::from(100),
            label: format!("Creator code: {} (-{}%)", code.code, pct.normalize()),
        })
        .filter(|d| d.amount > Decimal::ZERO)
        .collect())
}

/// Check a code before checkout (shows the creator's discount).
#[utoipa::path(get, path = "/api/v1/public/creator-codes/{code}", params(("code" = String, Path)), responses((status = 200), (status = 404)))]
pub async fn public_check(
    State(s): State<AppState>,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    // Unknown/disabled codes are 404; a database failure stays a 500.
    let c = match cc::check(&s.pool, &code, None).await {
        Ok(c) => c,
        Err(AppError::Validation(_)) => return Err(AppError::NotFound.into()),
        Err(e) => return Err(e.into()),
    };
    Ok(Json(json!({
        "code": c.code,
        "discount_percent": c.discount_percent.normalize().to_string(),
    })))
}

async fn my_affiliate(s: &AppState, u: &CurrentUser) -> ApiResult<affiliates::Affiliate> {
    affiliates::for_user(&s.pool, u.user_id)
        .await?
        .ok_or_else(|| AppError::Conflict("error.creator_code_not_affiliate").into())
}

/// The signed-in affiliate's code and how many orders used it.
#[utoipa::path(get, path = "/api/v1/client/creator-code", responses((status = 200), (status = 401), (status = 409)))]
pub async fn client_get(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let a = my_affiliate(&s, &u).await?;
    let code = cc::for_affiliate(&s.pool, a.id).await?;
    let uses = match &code {
        Some(c) => cc::uses(&s.pool, c.id).await?,
        None => 0,
    };
    Ok(Json(json!({ "code": code, "orders": uses })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatorCodeBody {
    pub code: String,
}

/// Pick or rename the code (3–24 of a-z, 0-9, `_`, `-`; stored lower-case).
#[utoipa::path(put, path = "/api/v1/client/creator-code", request_body = CreatorCodeBody, responses((status = 200, body = CreatorCode), (status = 409), (status = 422)))]
pub async fn client_set(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<CreatorCodeBody>,
) -> ApiResult<Json<CreatorCode>> {
    let a = my_affiliate(&s, &u).await?;
    let c = cc::set_code(&s.pool, a.id, &b.code).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator_code.set",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "code": c.code }),
    )
    .await?;
    Ok(Json(c))
}

#[utoipa::path(get, path = "/api/v1/admin/creator-codes", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AffiliatesManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": cc::list(&s.pool).await? })))
}

/// Set a code's buyer discount (0–50 %) or switch it off.
#[utoipa::path(put, path = "/api/v1/admin/creator-codes/{id}", request_body = CreatorCodeUpdate, params(("id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CreatorCodeUpdate>,
) -> ApiResult<Json<Value>> {
    cc::update(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator_code.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "discount_percent": b.discount_percent.to_string(), "enabled": b.enabled }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
