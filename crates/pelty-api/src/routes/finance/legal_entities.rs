//! Legal entities (selling companies): numbering, terms, bank details.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::legal_entities::{self as le, EntityInput, LegalEntity};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/legal-entities", get(list).post(create))
        .route("/admin/legal-entities/{id}", put(update).delete(remove))
        .route("/admin/legal-entities/{id}/default", post(make_default))
        .route("/admin/legal-entities/default", delete(clear_default))
        .route(
            "/admin/customers/{id}/legal-entity",
            get(customer_entity).put(assign),
        )
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EntityView {
    #[serde(flatten)]
    pub entity: LegalEntity,
    /// Next invoice number this year (not consumed).
    pub next_number: String,
}

async fn view(s: &AppState, e: LegalEntity) -> ApiResult<EntityView> {
    let year = OffsetDateTime::now_utc().year();
    let next_number = le::preview(&s.pool, &e, year).await?;
    Ok(EntityView {
        entity: e,
        next_number,
    })
}

#[utoipa::path(get, path = "/api/v1/admin/legal-entities", responses((status = 200, body = [EntityView]), (status = 403)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Vec<EntityView>>> {
    let mut out = vec![];
    for e in le::list(&s.pool).await? {
        out.push(view(&s, e).await?);
    }
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/admin/legal-entities", request_body = EntityInput, responses((status = 200, body = EntityView), (status = 409), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<EntityInput>,
) -> ApiResult<Json<EntityView>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    let e = le::create(&s.pool, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "legal_entity.create",
        Some(&e.id.to_string()),
        u.ip.as_deref(),
        json!({ "name": e.name }),
    )
    .await?;
    Ok(Json(view(&s, e).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/legal-entities/{id}", request_body = EntityInput, responses((status = 200, body = EntityView), (status = 404), (status = 409), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<EntityInput>,
) -> ApiResult<Json<EntityView>> {
    let b = b.normalized().map_err(AppError::Validation)?;
    let e = le::update(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "legal_entity.update",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": e.name }),
    )
    .await?;
    Ok(Json(view(&s, e).await?))
}

#[utoipa::path(delete, path = "/api/v1/admin/legal-entities/{id}", responses((status = 204), (status = 404), (status = 409)))]
pub async fn remove(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    le::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "legal_entity.delete",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/admin/legal-entities/{id}/default", responses((status = 204), (status = 404)))]
pub async fn make_default(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    le::set_default(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "legal_entity.default",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignBody {
    /// `null` = use the default entity.
    pub legal_entity_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/legal-entity", responses((status = 200, body = AssignBody), (status = 404)))]
pub async fn customer_entity(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AssignBody>> {
    if s.clients().get(id).await?.is_none() {
        return Err(AppError::NotFound.into());
    }
    Ok(Json(AssignBody {
        legal_entity_id: le::of_customer(&s.pool, id).await?,
    }))
}

#[utoipa::path(put, path = "/api/v1/admin/customers/{id}/legal-entity", request_body = AssignBody, responses((status = 200, body = AssignBody), (status = 404), (status = 422)))]
pub async fn assign(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<AssignBody>,
) -> ApiResult<Json<AssignBody>> {
    le::assign_customer(&s.pool, id, b.legal_entity_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.legal_entity",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!(b),
    )
    .await?;
    Ok(Json(b))
}

#[utoipa::path(delete, path = "/api/v1/admin/legal-entities/default", responses((status = 204), (status = 403)))]
pub async fn clear_default(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
) -> ApiResult<axum::http::StatusCode> {
    le::clear_default(&s.pool).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "legal_entity.default",
        None,
        u.ip.as_deref(),
        json!({ "cleared": true }),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
