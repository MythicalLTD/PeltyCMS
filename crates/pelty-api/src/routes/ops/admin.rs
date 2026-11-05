//! Admin dashboard routes: overview stats and admin landing data.
use crate::routes::Page;
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use pelty_core::AppError;
use pelty_database::audit::AuditFilter;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list_users))
        .route("/admin/audit-logs", get(list_audit))
        .route("/admin/audit-logs/actions", get(audit_actions))
        .route("/admin/audit-logs/export.csv", get(export_audit))
        .route("/admin/users/{id}/verify-email", post(verify_email))
}

#[utoipa::path(get, path = "/api/v1/admin/users", params(Page), responses((status = 200), (status = 401), (status = 403)))]
pub async fn list_users(
    State(s): State<AppState>,
    _p: RequirePermission<perms::UsersRead>,
    Query(q): Query<Page>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = pelty_database::users::list(&s.pool, limit, offset).await?;
    Ok(Json(
        json!({ "items": items, "page": page, "per_page": per, "total": total }),
    ))
}

#[derive(serde::Deserialize, utoipa::IntoParams, Default)]
pub struct AuditQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    /// Action prefix, e.g. `invoice.`.
    pub action: Option<String>,
    /// Actor e-mail substring.
    pub actor: Option<String>,
    /// Actor user id (exact).
    pub actor_id: Option<uuid::Uuid>,
    /// Target, exact (usually an id).
    pub target: Option<String>,
    /// IP address or prefix.
    pub ip: Option<String>,
    /// Free text over action, target and metadata.
    pub q: Option<String>,
    /// From (inclusive): `YYYY-MM-DD` or RFC 3339.
    pub from: Option<String>,
    /// To: `YYYY-MM-DD` (whole day, inclusive) or RFC 3339 (exclusive).
    pub to: Option<String>,
    /// `true` = only entries without an actor (system / background jobs).
    pub system: Option<bool>,
}

/// `YYYY-MM-DD` or RFC 3339. A bare date as `to` means the end of that day.
pub fn parse_bound(v: &str, end: bool) -> Option<time::OffsetDateTime> {
    use time::format_description::well_known::Rfc3339;
    if let Ok(t) = time::OffsetDateTime::parse(v, &Rfc3339) {
        return Some(t);
    }
    let d = time::Date::parse(v, time::macros::format_description!("[year]-[month]-[day]")).ok()?;
    let d = if end { d.next_day()? } else { d };
    Some(d.midnight().assume_utc())
}

fn audit_filter(q: &AuditQuery) -> ApiResult<AuditFilter> {
    let f = |v: &Option<String>| v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.chars().take(200).collect::<String>());
    let bound = |v: &Option<String>, end| -> ApiResult<Option<time::OffsetDateTime>> {
        match f(v) {
            None => Ok(None),
            Some(v) => parse_bound(&v, end).map(Some).ok_or_else(|| AppError::Validation("error.invalid_range").into()),
        }
    };
    let (from, to) = (bound(&q.from, false)?, bound(&q.to, true)?);
    if let (Some(a), Some(b)) = (from, to) {
        if a >= b {
            return Err(AppError::Validation("error.invalid_range").into());
        }
    }
    Ok(AuditFilter { action: f(&q.action), actor: f(&q.actor), actor_id: q.actor_id, target: f(&q.target), ip: f(&q.ip), q: f(&q.q), from, to, system: q.system.unwrap_or(false) })
}

#[utoipa::path(get, path = "/api/v1/admin/audit-logs", params(AuditQuery), responses((status = 200), (status = 401), (status = 403), (status = 422)))]
pub async fn list_audit(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AuditRead>,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = Page { page: q.page, per_page: q.per_page }.bounds();
    let (items, total) = pelty_database::audit::filter(&s.pool, &audit_filter(&q)?, limit, offset).await?;
    Ok(Json(
        json!({ "items": items, "page": page, "per_page": per, "total": total }),
    ))
}

/// Distinct action names with counts, for the filter picker.
#[utoipa::path(get, path = "/api/v1/admin/audit-logs/actions", responses((status = 200), (status = 401), (status = 403)))]
pub async fn audit_actions(State(s): State<AppState>, _p: RequirePermission<perms::AuditRead>) -> ApiResult<Json<Value>> {
    let rows = pelty_database::audit::action_counts(&s.pool, 1000).await?;
    Ok(Json(json!({ "items": rows.into_iter().map(|(action, count)| json!({ "action": action, "count": count })).collect::<Vec<_>>() })))
}

/// Most rows one export returns (newest first).
pub const AUDIT_EXPORT_MAX: i64 = 50_000;

/// CSV of the filtered audit log (same filters as the list, newest first, capped).
/// The export itself is audited.
#[utoipa::path(get, path = "/api/v1/admin/audit-logs/export.csv", params(AuditQuery), responses((status = 200, content_type = "text/csv"), (status = 401), (status = 403), (status = 422)))]
pub async fn export_audit(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AuditRead>,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Response> {
    use crate::csv::{Cell, Csv};
    use time::format_description::well_known::Rfc3339;
    let filter = audit_filter(&q)?;
    let (rows, total) = pelty_database::audit::filter(&s.pool, &filter, AUDIT_EXPORT_MAX, 0).await?;
    let mut c = Csv::new(&["id", "created_at", "action", "actor_id", "actor_email", "target", "ip", "metadata"]);
    for r in &rows {
        let (id, at, aid, meta) = (r.id.to_string(), r.created_at.format(&Rfc3339).unwrap_or_default(), r.actor_id.map(|a| a.to_string()).unwrap_or_default(), r.metadata.to_string());
        c.row([Cell::Text(&id), Cell::Text(&at), Cell::Text(&r.action), Cell::Text(&aid), Cell::Text(r.actor_email.as_deref().unwrap_or("")), Cell::Text(r.target.as_deref().unwrap_or("")), Cell::Text(r.ip.as_deref().unwrap_or("")), Cell::Text(&meta)]);
    }
    pelty_database::audit::record(&s.pool, Some(u.user_id), "audit.exported", None, u.ip.as_deref(), json!({ "rows": rows.len(), "total": total, "query": {
        "action": filter.action, "actor": filter.actor, "actor_id": filter.actor_id, "target": filter.target, "ip": filter.ip, "q": filter.q, "system": filter.system,
        "from": q.from, "to": q.to,
    } })).await?;
    let stamp = time::OffsetDateTime::now_utc().date();
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"audit-log-{stamp}.csv\"")),
            (header::CACHE_CONTROL, "no-store".to_string()),
            (header::HeaderName::from_static("x-total-count"), total.to_string()),
        ],
        format!("\u{feff}{}", c.finish()),
    )
        .into_response())
}

/// Mark a user's email address as confirmed (staff override). Idempotent.
#[utoipa::path(post, path = "/api/v1/admin/users/{id}/verify-email", responses((status = 200), (status = 401), (status = 403), (status = 404)))]
pub async fn verify_email(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::UsersManage>,
    Path(id): Path<uuid::Uuid>,
) -> ApiResult<Json<Value>> {
    pelty_database::users::find(&s.pool, id).await?.ok_or(pelty_core::AppError::NotFound)?;
    pelty_database::users::mark_verified(&s.pool, id).await?;
    pelty_database::audit::record(&s.pool, Some(u.user_id), "user.email_verified_by_staff", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
