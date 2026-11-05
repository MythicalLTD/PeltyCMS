//! Admin fraud screening: settings, results and the manual review queue.
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{
    audit,
    fraud::{self, FraudSettings},
};
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::ApiResult,
    extract::{perms, RequirePermission},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/settings/fraud", get(get_settings).put(put_settings))
        .route("/admin/fraud/checks", get(list_checks))
        .route("/admin/fraud/checks/{id}", get(get_check))
        .route("/admin/fraud/checks/{id}/approve", post(approve_check))
        .route("/admin/fraud/checks/{id}/reject", post(reject_check))
        .route("/admin/fraud/summary", get(summary))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/fraud", responses((status = 200, body = FraudSettings), (status = 403)))]
pub async fn get_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<FraudSettings>> {
    Ok(Json(fraud::get_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/fraud", request_body = FraudSettings, responses((status = 200, body = FraudSettings), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<FraudSettings>,
) -> ApiResult<Json<FraudSettings>> {
    let mut b = b.normalized().map_err(AppError::Validation)?;
    let mut ranges = Vec::with_capacity(b.vpn_ranges.len());
    for r in &b.vpn_ranges {
        let n = crate::security::parse_cidr(r).ok_or(AppError::Validation("error.invalid_cidr"))?;
        let n = n.to_string();
        if !ranges.contains(&n) {
            ranges.push(n);
        }
    }
    b.vpn_ranges = ranges;
    fraud::put_settings(&s.pool, &b).await?;
    crate::fraud::sync_http_hook(&s, &b);
    // The domain/range lists can be long; keep the audit entry readable.
    let mut meta = json!(b);
    meta["disposable_domains"] = json!(b.disposable_domains.len());
    meta["vpn_ranges"] = json!(b.vpn_ranges.len());
    meta["http_hook_key"] = json!(if b.http_hook_key.is_empty() {
        ""
    } else {
        "***"
    });
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.fraud",
        None,
        u.ip.as_deref(),
        meta,
    )
    .await?;
    Ok(Json(b))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ChecksQuery {
    pub verdict: Option<String>,
    /// Only reviews still awaiting a decision (the review queue).
    pub open: Option<bool>,
    pub customer_id: Option<Uuid>,
    pub order_id: Option<Uuid>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/fraud/checks", params(ChecksQuery), responses((status = 200), (status = 403)))]
pub async fn list_checks(
    State(s): State<AppState>,
    _p: RequirePermission<perms::OrdersManage>,
    Query(q): Query<ChecksQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = crate::routes::Page {
        page: q.page,
        per_page: q.per_page,
    }
    .bounds();
    let f = fraud::ListFilter {
        verdict: q
            .verdict
            .as_deref()
            .filter(|x| ["accept", "review", "reject"].contains(x)),
        open: q.open.unwrap_or(false),
        customer_id: q.customer_id,
        order_id: q.order_id,
    };
    let (items, total) = fraud::list(&s.pool, &f, limit, offset).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/fraud/checks/{id}", responses((status = 200, body = fraud::Check), (status = 404)))]
pub async fn get_check(
    State(s): State<AppState>,
    _p: RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<fraud::Check>> {
    Ok(Json(fraud::get(&s.pool, id).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/fraud/summary", responses((status = 200), (status = 403)))]
pub async fn summary(
    State(s): State<AppState>,
    _p: RequirePermission<perms::OrdersManage>,
) -> ApiResult<Json<Value>> {
    let cfg = fraud::get_settings(&s.pool).await?;
    Ok(Json(
        json!({ "enabled": cfg.enabled, "open_reviews": fraud::open_reviews(&s.pool).await? }),
    ))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewBody {
    /// Internal note stored with the decision.
    pub note: String,
    /// Approve: also approve the order now (otherwise the remaining approval
    /// rules, e.g. "awaiting payment", still apply).
    pub approve_order: bool,
}

/// Loads an open review whose order can still be acted on.
async fn open_check(s: &AppState, id: Uuid, note: &str) -> Result<(fraud::Check, Uuid), AppError> {
    if note.chars().count() > 2000 {
        return Err(AppError::Validation("error.validation"));
    }
    let c = fraud::get(&s.pool, id).await?;
    if c.verdict != "review" || c.decision.is_some() {
        return Err(AppError::Conflict("error.fraud_already_reviewed"));
    }
    let order = c
        .order_id
        .ok_or(AppError::Conflict("error.order_not_pending"))?;
    if c.order_status.as_deref() != Some("pending") {
        return Err(AppError::Conflict("error.order_not_pending"));
    }
    Ok((c, order))
}

/// Clear a fraud hold: the order continues through the approval rules (or is
/// approved right away with `approve_order`).
#[utoipa::path(post, path = "/api/v1/admin/fraud/checks/{id}/approve", request_body = ReviewBody, responses((status = 200), (status = 404), (status = 409)))]
pub async fn approve_check(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
    body: Option<Json<ReviewBody>>,
) -> ApiResult<Json<Value>> {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let note = b.note.trim();
    let (_, order) = open_check(&s, id, note).await?;
    if b.approve_order {
        // Addons may veto the approval; do that before recording a decision.
        crate::addon_ext::pre_event(
            &s,
            "order.before_approve",
            json!({ "order_id": order, "by": u.user_id }),
        )
        .await?;
    }
    let c = fraud::decide(&s.pool, id, "approved", u.user_id, note).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "fraud.review_approved",
        Some(&order.to_string()),
        u.ip.as_deref(),
        json!({ "check_id": id, "score": c.score, "note": note }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "order.fraud_approved",
        json!({ "order_id": order, "customer_id": c.customer_id, "check_id": id }),
    ));
    let (approved, services) = if b.approve_order {
        let ids =
            crate::approval::activate(&s, order, Some(u.user_id), u.ip.as_deref(), false).await?;
        (true, ids)
    } else {
        (crate::approval::try_auto(&s, order).await?, vec![])
    };
    Ok(Json(
        json!({ "check": c, "order_approved": approved, "services": services }),
    ))
}

/// Confirm the fraud: the order is cancelled with status `fraud` and its
/// unpaid invoice voided.
#[utoipa::path(post, path = "/api/v1/admin/fraud/checks/{id}/reject", request_body = ReviewBody, responses((status = 200), (status = 404), (status = 409)))]
pub async fn reject_check(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
    body: Option<Json<ReviewBody>>,
) -> ApiResult<Json<Value>> {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let note = b.note.trim();
    let (_, order) = open_check(&s, id, note).await?;
    s.orders().cancel(order, true).await?;
    let c = fraud::decide(&s.pool, id, "rejected", u.user_id, note).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "fraud.review_rejected",
        Some(&order.to_string()),
        u.ip.as_deref(),
        json!({ "check_id": id, "score": c.score, "note": note }),
    )
    .await?;
    s.emit(DomainEvent::new("order.fraud_rejected", json!({ "order_id": order, "customer_id": c.customer_id, "score": c.score, "check_id": id, "by_staff": true })));
    Ok(Json(json!({ "check": c })))
}
