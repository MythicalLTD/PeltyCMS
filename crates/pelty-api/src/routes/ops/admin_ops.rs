//! Admin ops: dashboard prefs + quick links, login history, staff online,
//! calendar, per-client email log with resend, DB backups.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::admin_ops::{self as ops, Prefs};
use pelty_database::audit;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/prefs", get(get_prefs).put(put_prefs))
        .route(
            "/admin/settings/housekeeping",
            get(get_housekeeping).put(put_housekeeping),
        )
        .route(
            "/admin/settings/inactive-clients",
            get(get_inactive).put(put_inactive),
        )
        .route("/admin/inactive-clients/preview", get(preview_inactive))
        .route("/admin/inactive-clients/run", post(run_inactive))
        .route(
            "/admin/settings/proforma",
            get(get_proforma).put(put_proforma),
        )
        .route("/admin/login-history", get(login_history))
        .route("/admin/staff-online", get(staff_online))
        .route("/admin/calendar", get(calendar))
        .route("/admin/customers/{id}/emails", get(customer_emails))
        .route("/admin/emails/{job}/resend", post(resend))
        .route("/admin/backups", get(list_backups).post(create_backup))
        .route("/admin/backups/{id}", axum::routing::delete(delete_backup))
        .route("/admin/backups/{id}/download", get(download_backup))
}

#[utoipa::path(get, path = "/api/v1/admin/prefs", responses((status = 200, body = Prefs), (status = 403)))]
pub async fn get_prefs(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TodosManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "prefs": ops::prefs(&s.pool, u.user_id).await?, "widgets": ops::WIDGETS }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/prefs", request_body = Prefs, responses((status = 200), (status = 422)))]
pub async fn put_prefs(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TodosManage>,
    Json(p): Json<Prefs>,
) -> ApiResult<Json<Value>> {
    ops::save_prefs(&s.pool, u.user_id, &p).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct HistoryQ {
    pub user_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/login-history", params(HistoryQ), responses((status = 200), (status = 403)))]
pub async fn login_history(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::SettingsManage>,
    Query(q): Query<HistoryQ>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": ops::login_history(&s.pool, q.user_id, q.limit.unwrap_or(100)).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/staff-online", responses((status = 200), (status = 403)))]
pub async fn staff_online(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::TodosManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": ops::staff_online(&s.pool, 15).await?, "window_minutes": 15 }),
    ))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct MonthQ {
    pub month: String,
}

#[utoipa::path(get, path = "/api/v1/admin/calendar", params(MonthQ), responses((status = 200), (status = 422)))]
pub async fn calendar(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::TodosManage>,
    Query(q): Query<MonthQ>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "month": q.month, "items": ops::calendar(&s.pool, &q.month).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/emails", responses((status = 200), (status = 404)))]
pub async fn customer_emails(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": ops::customer_emails(&s.pool, id).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/emails/{job}/resend", responses((status = 200), (status = 404), (status = 409)))]
pub async fn resend(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(job): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (to, id) = ops::resend_email(&s.pool, job, crate::mail::SENSITIVE).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "email.resent",
        Some(&job.to_string()),
        u.ip.as_deref(),
        json!({ "to": pelty_database::campaigns::mask(&to), "job": id }),
    )
    .await?;
    Ok(Json(json!({ "job_id": id })))
}

#[utoipa::path(get, path = "/api/v1/admin/backups", responses((status = 200), (status = 403)))]
pub async fn list_backups(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::BackupsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": ops::backups(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/backups", responses((status = 200), (status = 403)))]
pub async fn create_backup(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::BackupsManage>,
) -> ApiResult<Json<Value>> {
    let b = ops::create_backup(&s.pool, Some(u.user_id)).await?;
    pelty_database::jobs::enqueue(
        &s.pool,
        crate::backup::JOB_KIND,
        json!({ "backup_id": b.id }),
        None,
        Some(&format!("backup:{}", b.id)),
        1,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "backup.created",
        Some(&b.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(b)))
}

#[utoipa::path(delete, path = "/api/v1/admin/backups/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_backup(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::BackupsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let name = ops::delete_backup(&s.pool, id).await?;
    if let Some(p) = crate::backup::path_for(&name) {
        let _ = std::fs::remove_file(p);
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "backup.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/backups/{id}/download", responses((status = 200), (status = 404)))]
pub async fn download_backup(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::BackupsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let b = ops::backup(&s.pool, id).await?;
    let path = crate::backup::path_for(&b.file_name)
        .filter(|_| b.status == "done")
        .ok_or(AppError::NotFound)?;
    let bytes = std::fs::read(path).map_err(|_| AppError::NotFound)?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "backup.downloaded",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", b.file_name),
            ),
        ],
        bytes,
    ))
}

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Proforma {
    /// Unpaid invoices carry no legal number; the next sequential number is assigned on payment.
    pub enabled: bool,
}

#[utoipa::path(get, path = "/api/v1/admin/settings/proforma", responses((status = 200), (status = 403)))]
pub async fn get_proforma(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    let v = pelty_database::settings::get(&s.pool, pelty_database::invoices::PROFORMA_KEY).await?;
    Ok(Json(
        json!({ "enabled": v.and_then(|v| v.as_bool()).unwrap_or(false) }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/proforma", request_body = Proforma, responses((status = 200), (status = 422)))]
pub async fn put_proforma(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<Proforma>,
) -> ApiResult<Json<Value>> {
    pelty_database::settings::set(
        &s.pool,
        pelty_database::invoices::PROFORMA_KEY,
        json!(b.enabled),
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.proforma_updated",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(json!(b)))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/housekeeping", responses((status = 200), (status = 403)))]
pub async fn get_housekeeping(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(
        pelty_database::housekeeping::get(&s.pool).await?
    )))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/housekeeping", responses((status = 200), (status = 422)))]
pub async fn put_housekeeping(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<pelty_database::housekeeping::HousekeepingSettings>,
) -> ApiResult<Json<Value>> {
    pelty_database::housekeeping::put(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.housekeeping_updated",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(json!(b)))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/inactive-clients", responses((status = 200, body = pelty_database::inactive_clients::Settings)))]
pub async fn get_inactive(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<pelty_database::inactive_clients::Settings>> {
    Ok(Json(pelty_database::inactive_clients::get(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/inactive-clients", request_body = pelty_database::inactive_clients::Settings, responses((status = 200), (status = 422)))]
pub async fn put_inactive(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<pelty_database::inactive_clients::Settings>,
) -> ApiResult<Json<pelty_database::inactive_clients::Settings>> {
    pelty_database::inactive_clients::put(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.inactive_clients_updated",
        None,
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(b))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct PreviewQuery {
    pub days: i32,
}

/// Dry run: who would be anonymised with this many days of inactivity.
#[utoipa::path(get, path = "/api/v1/admin/inactive-clients/preview", params(PreviewQuery), responses((status = 200), (status = 422)))]
pub async fn preview_inactive(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
    Query(q): Query<PreviewQuery>,
) -> ApiResult<Json<Value>> {
    let items = pelty_database::inactive_clients::candidates(&s.pool, q.days, 500).await?;
    Ok(Json(json!({ "count": items.len(), "items": items })))
}

/// Run the erasure now with the saved settings (no-op when disabled).
#[utoipa::path(post, path = "/api/v1/admin/inactive-clients/run", responses((status = 200)))]
pub async fn run_inactive(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    let n = pelty_database::inactive_clients::run(&s.pool).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "inactive_clients.erased",
        None,
        u.ip.as_deref(),
        json!({ "count": n }),
    )
    .await?;
    Ok(Json(json!({ "erased": n })))
}
