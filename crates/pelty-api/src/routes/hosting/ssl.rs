//! SSL certificate status for client domains: admin overview, on-demand
//! recheck, and the client's own view.
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, ssl_checks};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::ApiResult,
    extract::{perms, CurrentUser, RequirePermission},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/ssl", get(admin_list))
        .route("/admin/domains/{id}/ssl-check", post(admin_check))
        .route("/client/ssl", get(client_list))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SslQuery {
    pub status: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

const STATUSES: [&str; 5] = ["valid", "expiring", "expired", "invalid", "unreachable"];

#[utoipa::path(get, path = "/api/v1/admin/ssl", params(SslQuery), responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Query(q): Query<SslQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = crate::routes::Page {
        page: q.page,
        per_page: q.per_page,
    }
    .bounds();
    let status = q.status.as_deref().filter(|x| STATUSES.contains(x));
    let (items, total) = ssl_checks::list(&s.pool, status, limit, offset).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/admin/domains/{id}/ssl-check", responses((status = 200), (status = 404)))]
pub async fn admin_check(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let name = ssl_checks::domain_name(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let p = crate::ssl_probe::probe(&name, 443, false).await;
    ssl_checks::save(&s.pool, id, &p).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "domain.ssl_checked",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": p.status }),
    )
    .await?;
    Ok(Json(json!(ssl_checks::get(&s.pool, id).await?)))
}

#[utoipa::path(get, path = "/api/v1/client/ssl", responses((status = 200), (status = 401)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = crate::routes::customers::customer_of(&s, &u).await?;
    Ok(Json(
        json!({ "items": ssl_checks::for_customer(&s.pool, cid).await? }),
    ))
}
