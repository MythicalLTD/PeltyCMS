//! Affiliate program: public referral link (sets a tracking cookie), client
//! dashboard + payout requests, admin management (`affiliates.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::ListQuery;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::affiliates::{self, AffiliateUpdate};
use pelty_database::audit;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const REF_COOKIE: &str = "pelty_ref";
const REF_MAX_AGE: i64 = 60 * 60 * 24 * 30;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/affiliates/r/{code}", get(track))
        .route("/affiliates/track", post(track_json))
        .route("/client/affiliate", get(client_get).post(client_enroll))
        .route("/client/affiliate/payouts", post(client_payout))
        .route("/admin/affiliates", get(admin_list))
        .route("/admin/affiliates/{id}", put(admin_update))
        .route("/admin/affiliates/payouts", get(admin_payouts))
        .route("/admin/affiliates/payouts/{id}", post(admin_process))
}

/// Called from registration: attribute the new user if a tracking cookie is set.
pub(crate) async fn attribute_signup(s: &AppState, headers: &HeaderMap, user_id: Uuid) {
    if let Some(code) = crate::extract::cookie(headers, REF_COOKIE) {
        if let Err(e) = affiliates::attribute(&s.pool, user_id, &code).await {
            tracing::warn!(error = %e, "affiliate attribution failed");
        }
    }
}

/// Referral link: remembers the code for 30 days, then sends the visitor to signup.
#[utoipa::path(get, path = "/api/v1/affiliates/r/{code}", responses((status = 303)))]
pub async fn track(State(s): State<AppState>, Path(code): Path<String>) -> ApiResult<Response> {
    let mut res = (StatusCode::SEE_OTHER, [(header::LOCATION, "/register")]).into_response();
    if affiliates::code_exists(&s.pool, &code).await? {
        res.headers_mut().append(
            header::SET_COOKIE,
            crate::routes::auth::cookie_header(&s, REF_COOKIE, &code, REF_MAX_AGE, true),
        );
    }
    Ok(res)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackBody {
    pub code: String,
}

/// SPA landing (`/r/:code`): validates the code and sets the tracking cookie.
#[utoipa::path(post, path = "/api/v1/affiliates/track", request_body = TrackBody, responses((status = 200)))]
pub async fn track_json(
    State(s): State<AppState>,
    Json(b): Json<TrackBody>,
) -> ApiResult<Response> {
    let valid = affiliates::code_exists(&s.pool, &b.code).await?;
    let mut res = Json(json!({ "valid": valid })).into_response();
    if valid {
        res.headers_mut().append(
            header::SET_COOKIE,
            crate::routes::auth::cookie_header(&s, REF_COOKIE, &b.code, REF_MAX_AGE, true),
        );
    }
    Ok(res)
}

/// Mail every active user holding `affiliates.manage` about a payout request.
async fn notify_staff(s: &AppState, email: &str, amount: &str) -> ApiResult<()> {
    let link = format!("{}/admin/affiliates", s.cfg.public_url);
    for (to, locale) in affiliates::staff_recipients(&s.pool, "affiliates.manage").await? {
        let locale = locale
            .filter(|l| s.i18n.has(l))
            .unwrap_or_else(|| s.cfg.default_locale.clone());
        crate::mail::enqueue(
            s,
            "affiliate_payout_requested",
            &locale,
            &to,
            crate::mail::vars(&[("affiliate", email), ("amount", amount), ("link", &link)]),
        )
        .await?;
    }
    Ok(())
}

async fn dashboard(s: &AppState, a: affiliates::Affiliate) -> ApiResult<Json<Value>> {
    let balances = affiliates::balances(&s.pool, a.id).await?;
    let commissions = affiliates::commissions(&s.pool, a.id).await?;
    let payouts = affiliates::payouts(&s.pool, Some(a.id)).await?;
    let link = format!("/r/{}", a.code);
    Ok(Json(json!({
        "enrolled": true, "affiliate": a, "link": link,
        "balances": balances, "commissions": commissions, "payouts": payouts
    })))
}

#[utoipa::path(get, path = "/api/v1/client/affiliate", responses((status = 200), (status = 401)))]
pub async fn client_get(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    match affiliates::for_user(&s.pool, u.user_id).await? {
        Some(a) => dashboard(&s, a).await,
        None => Ok(Json(json!({ "enrolled": false }))),
    }
}

#[utoipa::path(post, path = "/api/v1/client/affiliate", responses((status = 200), (status = 401)))]
pub async fn client_enroll(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let a = affiliates::enroll(&s.pool, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.enrolled",
        Some(&a.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    dashboard(&s, a).await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PayoutBody {
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
}

#[utoipa::path(post, path = "/api/v1/client/affiliate/payouts", request_body = PayoutBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn client_payout(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<PayoutBody>,
) -> ApiResult<Json<Value>> {
    let a = affiliates::for_user(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let id =
        affiliates::request_payout(&s.pool, a.id, &b.currency.to_uppercase(), b.amount).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.payout_requested",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "amount": b.amount, "currency": b.currency }),
    )
    .await?;
    let cur = b.currency.to_uppercase();
    s.emit(pelty_extensions::events::DomainEvent::new(
        "affiliate.payout_requested",
        json!({ "payout_id": id, "affiliate_id": a.id, "amount": b.amount, "currency": cur }),
    ));
    let email = affiliates::user_email(&s.pool, u.user_id).await?;
    notify_staff(&s, &email, &format!("{} {}", b.amount, cur)).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(get, path = "/api/v1/admin/affiliates", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AffiliatesManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (lim, off, page, per) = q.bounds();
    let (items, total) = affiliates::list(&s.pool, lim, off).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(put, path = "/api/v1/admin/affiliates/{id}", request_body = AffiliateUpdate, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<AffiliateUpdate>,
) -> ApiResult<Json<Value>> {
    affiliates::update(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "commission_percent": b.commission_percent, "status": b.status }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/affiliates/payouts", responses((status = 200), (status = 403)))]
pub async fn admin_payouts(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AffiliatesManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": affiliates::payouts(&s.pool, None).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessBody {
    pub status: String,
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/admin/affiliates/payouts/{id}", request_body = ProcessBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_process(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ProcessBody>,
) -> ApiResult<Json<Value>> {
    affiliates::process_payout(&s.pool, id, &b.status, &b.note).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "affiliate.payout_processed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
