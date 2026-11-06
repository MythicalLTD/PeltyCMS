//! Upgrade/downgrade and cancellation requests (client + admin).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::billing::DUE_DAYS;
use crate::routes::customers::customer_of;
use crate::routes::{paged, ListQuery};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::service_changes as db;
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/services/{id}/lifecycle", get(client_lifecycle))
        .route(
            "/client/services/{id}/change",
            post(client_change).delete(client_cancel_change),
        )
        .route(
            "/client/services/{id}/options/quote",
            post(client_quote_options),
        )
        .route(
            "/client/services/{id}/options/change",
            post(client_change_options).delete(client_cancel_option_change),
        )
        .route(
            "/client/services/{id}/cancellation",
            post(client_request_cancel).delete(client_withdraw_cancel),
        )
        .route("/admin/services/{id}/lifecycle", get(admin_lifecycle))
        .route(
            "/admin/services/{id}/change",
            post(admin_change).delete(admin_cancel_change),
        )
        .route("/admin/cancellations", get(admin_cancellations))
        .route("/admin/cancellations/{id}/decide", post(admin_decide))
}

fn today() -> time::Date {
    OffsetDateTime::now_utc().date()
}

async fn owned(s: &AppState, u: &CurrentUser, id: Uuid) -> ApiResult<()> {
    let cid = customer_of(s, u).await?;
    s.hosting()
        .get_owned(id, cid)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(())
}

async fn lifecycle(s: &AppState, id: Uuid) -> ApiResult<Value> {
    // Options are only offered for active recurring services.
    let options = match db::options(&s.pool, id, today()).await {
        Ok(o) => o,
        Err(AppError::Conflict(_)) => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let changes = db::changes_for(&s.pool, id).await?;
    let option_changes = db::option_changes_for(&s.pool, id).await?;
    let cancellation = db::open_cancel(&s.pool, id).await?;
    let config: Value = sqlx::query_scalar("SELECT config FROM services WHERE id = $1")
        .bind(id)
        .fetch_optional(&s.pool)
        .await
        .map_err(pelty_database::db_err)?
        .unwrap_or_else(|| json!({}));
    // Product configurable options for the UI.
    let product_id: Option<Uuid> =
        sqlx::query_scalar("SELECT product_id FROM services WHERE id = $1")
            .bind(id)
            .fetch_optional(&s.pool)
            .await
            .map_err(pelty_database::db_err)?;
    let product_options = match product_id {
        Some(pid) => {
            let full = pelty_database::catalog::products(&s.pool, false, Some(pid)).await?;
            full.into_iter()
                .next()
                .map(|p| p.options)
                .unwrap_or_default()
        }
        None => vec![],
    };
    Ok(json!({
        "options": options,
        "changes": changes,
        "option_changes": option_changes,
        "cancellation": cancellation,
        "config": config,
        "product_options": product_options,
    }))
}

/// Cancel immediately: lifecycle transition plus provisioning hook.
pub(crate) async fn cancel_service(s: &AppState, id: Uuid, actor: Option<Uuid>) -> ApiResult<()> {
    let (from, svc) = match s.hosting().transition(id, "cancel", "").await {
        Ok(v) => v,
        // Already terminal: nothing to do.
        Err(AppError::Conflict(_)) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    audit::record(
        &s.pool,
        actor,
        "service.transition",
        Some(&id.to_string()),
        None,
        json!({ "from": from, "to": svc.status, "reason": "cancellation" }),
    )
    .await?;
    crate::provision::on_transition(s, id, &from, &svc.status).await;
    s.emit(DomainEvent::new(
        "service.cancelled",
        json!({ "service_id": id }),
    ));
    Ok(())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeBody {
    pub product_id: Uuid,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelBody {
    /// `immediate` or `end_of_period`.
    pub kind: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DecideBody {
    pub approve: bool,
    #[serde(default)]
    pub note: String,
}

async fn do_change(
    s: &AppState,
    id: Uuid,
    to: Uuid,
    actor: Uuid,
    ip: Option<&str>,
) -> ApiResult<Json<Value>> {
    // Cancel-only pre-event: refuse an upgrade/downgrade path.
    crate::addon_ext::pre_event(
        s,
        "service.before_change",
        json!({ "service_id": id, "product_id": to, "by": actor }),
    )
    .await?;
    let (change, invoice) =
        db::request_change(&s.pool, id, to, Some(actor), today(), DUE_DAYS).await?;
    audit::record(&s.pool, Some(actor), "service.change_requested", Some(&id.to_string()), ip,
        json!({ "change_id": change.id, "kind": change.kind, "to": to, "invoice_id": change.invoice_id })).await?;
    if change.status == "applied" {
        crate::provision::on_plan_change(s, id).await;
    }
    s.emit(DomainEvent::new("service.change_requested", json!({ "service_id": id, "change_id": change.id, "kind": change.kind, "product_id": to, "status": change.status, "invoice_id": change.invoice_id })));
    if let Some(inv) = &invoice {
        s.emit(DomainEvent::new(
            "invoice.created",
            json!({ "invoice_id": inv.id, "customer_id": inv.customer_id, "service_id": id }),
        ));
    }
    Ok(Json(json!({ "change": change, "invoice": invoice })))
}

#[utoipa::path(get, path = "/api/v1/client/services/{id}/lifecycle", responses((status = 200), (status = 404)))]
pub async fn client_lifecycle(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    Ok(Json(lifecycle(&s, id).await?))
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/change", request_body = ChangeBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn client_change(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeBody>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    do_change(&s, id, b.product_id, u.user_id, u.ip.as_deref()).await
}

#[utoipa::path(delete, path = "/api/v1/client/services/{id}/change", responses((status = 200), (status = 404)))]
pub async fn client_cancel_change(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    db::cancel_change(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.change_cancelled",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/cancellation", request_body = CancelBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn client_request_cancel(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<CancelBody>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    // Pre-event: retention addons may refuse, or rewrite kind and reason.
    let pre = crate::addon_ext::pre_event(
        &s,
        "service.before_cancellation_request",
        json!({ "service_id": id, "kind": b.kind, "reason": b.reason, "user_id": u.user_id }),
    )
    .await?;
    let kind = crate::addon_ext::pre_choice(&pre, "kind", &["immediate", "end_of_period"], &b.kind);
    let reason = pre
        .get("reason")
        .and_then(Value::as_str)
        .filter(|r| r.len() <= 2000)
        .map(str::to_string)
        .unwrap_or(b.reason);
    let r = db::request_cancel(&s.pool, id, &kind, &reason, Some(u.user_id)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.cancellation_requested",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "request_id": r.id, "kind": r.kind }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "service.cancellation_requested",
        json!({ "service_id": id, "request_id": r.id, "kind": r.kind, "reason": reason }),
    ));
    Ok(Json(json!({ "cancellation": r })))
}

#[utoipa::path(delete, path = "/api/v1/client/services/{id}/cancellation", responses((status = 200), (status = 404)))]
pub async fn client_withdraw_cancel(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    db::withdraw_cancel(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.cancellation_withdrawn",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(DomainEvent::new(
        "service.cancellation_withdrawn",
        json!({ "service_id": id }),
    ));
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/services/{id}/lifecycle", responses((status = 200), (status = 403)))]
pub async fn admin_lifecycle(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.hosting().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(lifecycle(&s, id).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/services/{id}/change", request_body = ChangeBody, responses((status = 200), (status = 403), (status = 409), (status = 422)))]
pub async fn admin_change(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeBody>,
) -> ApiResult<Json<Value>> {
    do_change(&s, id, b.product_id, u.user_id, u.ip.as_deref()).await
}

#[utoipa::path(delete, path = "/api/v1/admin/services/{id}/change", responses((status = 200), (status = 403), (status = 404)))]
pub async fn admin_cancel_change(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    db::cancel_change(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.change_cancelled",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/cancellations", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_cancellations(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let status = match q.status() {
        Some(x) if !db::CANCEL_STATUSES.contains(&x) => {
            return Err(AppError::Validation("error.invalid_status").into())
        }
        x => x,
    };
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = db::list_cancels(&s.pool, status, limit, offset).await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/admin/cancellations/{id}/decide", request_body = DecideBody, responses((status = 200), (status = 403), (status = 404), (status = 409)))]
pub async fn admin_decide(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<DecideBody>,
) -> ApiResult<Json<Value>> {
    let d = db::decide(&s.pool, id, b.approve, &b.note, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.cancellation_decided",
        Some(&d.request.service_id.to_string()),
        u.ip.as_deref(),
        json!({ "request_id": id, "status": d.request.status }),
    )
    .await?;
    s.emit(DomainEvent::new("service.cancellation_decided", json!({ "service_id": d.request.service_id, "request_id": id, "status": d.request.status, "approved": b.approve })));
    if d.cancel_now {
        cancel_service(&s, d.request.service_id, Some(u.user_id)).await?;
    }
    Ok(Json(json!({ "cancellation": d.request })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionChangeBody {
    pub config: Value,
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/options/quote", request_body = OptionChangeBody, responses((status = 200), (status = 404)))]
pub async fn client_quote_options(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<OptionChangeBody>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    let q = db::quote_options(&s.pool, id, &b.config, today()).await?;
    Ok(Json(json!(q)))
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/options/change", request_body = OptionChangeBody, responses((status = 200), (status = 404)))]
pub async fn client_change_options(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<OptionChangeBody>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    let (change, invoice) =
        db::request_option_change(&s.pool, id, b.config, Some(u.user_id), today(), DUE_DAYS)
            .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.option_change",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "change_id": change.id, "status": change.status }),
    )
    .await?;
    Ok(Json(json!({ "change": change, "invoice": invoice })))
}

#[utoipa::path(delete, path = "/api/v1/client/services/{id}/options/change", responses((status = 200), (status = 404)))]
pub async fn client_cancel_option_change(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    owned(&s, &u, id).await?;
    db::cancel_option_change(&s.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}
