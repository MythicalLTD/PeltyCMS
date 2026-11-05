//! Admin CRUD for banned e-mail domains (checked at registration).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use pelty_database::{audit, banned_emails as be};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/banned-emails", get(list).post(add))
        .route("/admin/banned-emails/{domain}", delete(remove))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddBody {
    pub domain: String,
    #[serde(default)]
    pub reason: String,
}

#[utoipa::path(get, path = "/api/v1/admin/banned-emails", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": be::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/banned-emails", request_body = AddBody, responses((status = 200), (status = 422)))]
pub async fn add(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<AddBody>) -> ApiResult<Json<Value>> {
    let d = be::normalize(&b.domain)?;
    let reason: String = b.reason.trim().chars().take(200).collect();
    be::add(&s.pool, &d, &reason).await?;
    audit::record(&s.pool, Some(u.user_id), "security.email_domain_banned", Some(&d), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "domain": d })))
}

#[utoipa::path(delete, path = "/api/v1/admin/banned-emails/{domain}", responses((status = 200), (status = 404)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Path(domain): Path<String>) -> ApiResult<Json<Value>> {
    let d = be::normalize(&domain)?;
    be::remove(&s.pool, &d).await?;
    audit::record(&s.pool, Some(u.user_id), "security.email_domain_unbanned", Some(&d), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
