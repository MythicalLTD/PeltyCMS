//! Admin client groups (`client_groups.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::client_groups::{self, GroupInput, COLORS};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/client-groups", get(list).post(create))
        .route("/admin/client-groups/{id}", put(update).delete(remove))
        .route(
            "/admin/client-groups/{id}/members",
            get(members).post(add_member),
        )
        .route(
            "/admin/client-groups/{id}/members/{customer_id}",
            delete(remove_member),
        )
        .route("/admin/customers/{id}/group", post(set_customer_group))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GroupBody {
    pub name: String,
    #[serde(default = "slate")]
    pub color: String,
    /// Decimal string, 0-100.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>)]
    pub discount_percent: Option<Decimal>,
    #[serde(default)]
    pub late_fee_exempt: bool,
    #[serde(default)]
    pub description: String,
}
fn slate() -> String {
    "slate".into()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MemberBody {
    pub email: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CustomerGroupBody {
    pub group_id: Option<Uuid>,
}

fn validate(b: &GroupBody) -> Result<(String, Decimal), AppError> {
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Validation("error.invalid_name"));
    }
    if !COLORS.contains(&b.color.as_str()) {
        return Err(AppError::Validation("error.invalid_color"));
    }
    let d = b.discount_percent.unwrap_or(Decimal::ZERO);
    if d < Decimal::ZERO || d > Decimal::ONE_HUNDRED || d.scale() > 2 {
        return Err(AppError::Validation("error.invalid_discount"));
    }
    if b.description.chars().count() > 500 {
        return Err(AppError::Validation("error.validation"));
    }
    Ok((name.to_string(), d))
}

#[utoipa::path(get, path = "/api/v1/admin/client-groups", responses((status = 200), (status = 403)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ClientGroupsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": client_groups::list(&s.pool).await?, "colors": COLORS }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/client-groups", request_body = GroupBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ClientGroupsManage>,
    Json(b): Json<GroupBody>,
) -> ApiResult<Json<Value>> {
    let (name, d) = validate(&b)?;
    let id = client_groups::create(
        &s.pool,
        &GroupInput {
            name: &name,
            color: &b.color,
            discount_percent: d,
            late_fee_exempt: b.late_fee_exempt,
            description: b.description.trim(),
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "client_group.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": name }),
    )
    .await?;
    Ok(Json(
        json!({ "group": client_groups::get(&s.pool, id).await? }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/client-groups/{id}", request_body = GroupBody, responses((status = 200), (status = 404)))]
pub async fn update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ClientGroupsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<GroupBody>,
) -> ApiResult<Json<Value>> {
    let (name, d) = validate(&b)?;
    if !client_groups::update(
        &s.pool,
        id,
        &GroupInput {
            name: &name,
            color: &b.color,
            discount_percent: d,
            late_fee_exempt: b.late_fee_exempt,
            description: b.description.trim(),
        },
    )
    .await?
    {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "client_group.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "discount_percent": d.to_string() }),
    )
    .await?;
    Ok(Json(
        json!({ "group": client_groups::get(&s.pool, id).await? }),
    ))
}

#[utoipa::path(delete, path = "/api/v1/admin/client-groups/{id}", responses((status = 204), (status = 404)))]
pub async fn remove(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ClientGroupsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !client_groups::delete(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "client_group.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/admin/client-groups/{id}/members", responses((status = 200), (status = 404)))]
pub async fn members(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ClientGroupsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    client_groups::get(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(
        json!({ "items": client_groups::members(&s.pool, id).await? }),
    ))
}

async fn assign(
    s: &AppState,
    u: &crate::extract::CurrentUser,
    customer: Uuid,
    group: Option<Uuid>,
) -> ApiResult<()> {
    if let Some(g) = group {
        client_groups::get(&s.pool, g)
            .await?
            .ok_or(AppError::Validation("error.invalid_group"))?;
    }
    if !client_groups::assign(&s.pool, customer, group).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.group_changed",
        Some(&customer.to_string()),
        u.ip.as_deref(),
        json!({ "group_id": group }),
    )
    .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/admin/client-groups/{id}/members", request_body = MemberBody, responses((status = 204), (status = 404), (status = 422)))]
pub async fn add_member(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ClientGroupsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<MemberBody>,
) -> ApiResult<StatusCode> {
    client_groups::get(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let c = client_groups::customer_by_email(&s.pool, b.email.trim())
        .await?
        .ok_or(AppError::Validation("error.unknown_customer"))?;
    assign(&s, &u, c, Some(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/admin/client-groups/{id}/members/{customer_id}", responses((status = 204), (status = 404)))]
pub async fn remove_member(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ClientGroupsManage>,
    Path((id, customer_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let current = client_groups::of_customer(&s.pool, customer_id).await?;
    if current.map(|g| g.id) != Some(id) {
        return Err(AppError::NotFound.into());
    }
    assign(&s, &u, customer_id, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/group", request_body = CustomerGroupBody, responses((status = 204), (status = 404), (status = 422)))]
pub async fn set_customer_group(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ClientGroupsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CustomerGroupBody>,
) -> ApiResult<StatusCode> {
    assign(&s, &u, id, b.group_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
