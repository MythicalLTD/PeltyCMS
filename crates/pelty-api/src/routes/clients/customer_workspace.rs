//! Unified customer workspace reads; section permissions use scoped CurrentUser permissions.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::Method;
use axum::routing::get;
use axum::{Json, Router};
use pelty_database::customer_workspace::{self as db, Access};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/customers/{id}/workspace", get(workspace))
        .route("/admin/customers/{id}/activity", get(activity))
}

pub(crate) fn access(u: &CurrentUser) -> Access {
    // CurrentUser already intersects API-key scopes with owner permissions.
    // Never use roles or re-expand those permissions after that intersection.
    let services = u.permits("services.manage", &Method::GET);
    Access {
        finance: u.permits("invoices.manage", &Method::GET),
        services,
        support: u.permits("tickets.manage", &Method::GET),
        operations: services && u.has("jobs.manage"),
        audit: u.has("audit.read"),
        manage_customer: u.has("customers.manage"),
    }
}

/// Snapshot-consistent overview. Lists are bounded to 50; forbidden totals/counts are null.
/// Money is serialized as decimal strings and balances are never combined across currencies.
#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/workspace", params(("id" = Uuid, Path)), responses((status = 200, body = Value), (status = 403), (status = 404)))]
pub async fn workspace(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(db::workspace(&s.pool, id, access(&u)).await?))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ActivityQuery {
    /// all/invoice/payment/refund/credit/service/domain/ticket/chat/email/job/note/audit/customer
    pub kind: Option<String>,
    /// Opaque next_cursor from the preceding page, URL-encoded as a query value.
    pub before: Option<String>,
    /// Page size, 1 through 100 (default 50).
    pub limit: Option<i64>,
}

/// Stable descending (occurred_at, event key) pagination, authorized before LIMIT.
#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/activity", params(("id" = Uuid, Path), ActivityQuery), responses((status = 200, body = Value), (status = 403), (status = 404), (status = 422)))]
pub async fn activity(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
    Query(q): Query<ActivityQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        db::activity(
            &s.pool,
            id,
            access(&u),
            q.kind.as_deref().unwrap_or("all"),
            q.before.as_deref(),
            q.limit.unwrap_or(50),
        )
        .await?,
    ))
}

/// Keep the existing summary arrays, but use the safe projection and customer's
/// configured currency for legacy scalar money fields (never a cross-currency sum).
pub(crate) async fn legacy_summary(s: &AppState, u: &CurrentUser, id: Uuid) -> ApiResult<Value> {
    let mut v = db::workspace(&s.pool, id, access(u)).await?;
    let balance = v["balances"]
        .as_array()
        .and_then(|bs| {
            bs.iter()
                .find(|b| b["currency"] == v["customer"]["currency"])
        })
        .cloned();
    let mut stats = v["counts"].clone();
    for key in ["invoices_unpaid", "amount_due", "amount_paid", "credit"] {
        stats[key] = balance
            .as_ref()
            .map(|b| b[key].clone())
            .unwrap_or(Value::Null);
    }
    stats["currency"] = v["customer"]["currency"].clone();
    v["stats"] = stats;
    v["legacy_currency"] = json!(v["customer"]["currency"]);
    Ok(v)
}
