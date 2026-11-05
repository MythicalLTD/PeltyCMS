//! Security questions: staff manage the list, users pick one and store a
//! hashed answer, staff verify a caller against it (answer never shown).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_auth::password;
use pelty_core::AppError;
use pelty_database::{audit, security_questions as sq, users};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/security-questions", get(admin_list).post(admin_create))
        .route("/admin/security-questions/{id}", delete(admin_delete))
        .route("/admin/customers/{id}/security-question", get(admin_status))
        .route("/admin/customers/{id}/security-question/verify", post(admin_verify))
        .route("/account/security-question", get(my_question).put(set_mine).delete(clear_mine))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewQuestion {
    pub question: String,
}

#[utoipa::path(get, path = "/api/v1/admin/security-questions", responses((status = 200), (status = 403)))]
pub async fn admin_list(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": sq::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/security-questions", request_body = NewQuestion, responses((status = 200), (status = 422)))]
pub async fn admin_create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<NewQuestion>) -> ApiResult<Json<Value>> {
    let id = sq::create(&s.pool, &b.question).await?;
    audit::record(&s.pool, Some(u.user_id), "security_question.created", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(delete, path = "/api/v1/admin/security-questions/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_delete(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    sq::delete(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "security_question.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn owner(s: &AppState, customer: Uuid) -> ApiResult<Uuid> {
    let o: Option<Option<Uuid>> = sqlx::query_scalar("SELECT owner_user_id FROM customers WHERE id = $1")
        .bind(customer)
        .fetch_optional(&s.pool)
        .await
        .map_err(pelty_database::db_err)?;
    Ok(o.flatten().ok_or(AppError::NotFound)?)
}

/// Which question the client chose (not the answer).
#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/security-question", responses((status = 200), (status = 404)))]
pub async fn admin_status(State(s): State<AppState>, _p: RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let uid = owner(&s, id).await?;
    Ok(Json(json!({ "current": sq::current(&s.pool, uid).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyBody {
    pub answer: String,
}

/// Check a caller's answer. Rate limited per customer and audited.
#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/security-question/verify", request_body = VerifyBody, responses((status = 200), (status = 404), (status = 429)))]
pub async fn admin_verify(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>, Json(b): Json<VerifyBody>) -> ApiResult<Json<Value>> {
    let uid = owner(&s, id).await?;
    let key = format!("secq:{uid}");
    if s.auth_limiter.exceeded(&key) {
        return Err(AppError::RateLimited.into());
    }
    let hash = sq::answer_hash(&s.pool, uid).await?.ok_or(AppError::Conflict("error.security_question_unset"))?;
    let ok = password::verify(&sq::normalize(&b.answer), &hash);
    if !ok {
        s.auth_limiter.check(&key);
    }
    audit::record(&s.pool, Some(u.user_id), "security_question.verified", Some(&id.to_string()), u.ip.as_deref(), json!({ "match": ok })).await?;
    Ok(Json(json!({ "match": ok })))
}

#[utoipa::path(get, path = "/api/v1/account/security-question", responses((status = 200), (status = 401)))]
pub async fn my_question(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "questions": sq::list(&s.pool).await?, "current": sq::current(&s.pool, u.user_id).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAnswer {
    pub question_id: Uuid,
    pub answer: String,
    /// Current account password.
    pub password: String,
}

#[utoipa::path(put, path = "/api/v1/account/security-question", request_body = SetAnswer, responses((status = 200), (status = 422), (status = 429)))]
pub async fn set_mine(State(s): State<AppState>, u: CurrentUser, Json(b): Json<SetAnswer>) -> ApiResult<Json<Value>> {
    let key = format!("reauth:{}", u.user_id);
    if s.auth_limiter.exceeded(&key) {
        return Err(AppError::RateLimited.into());
    }
    let user = users::find(&s.pool, u.user_id).await?.ok_or(AppError::Unauthenticated)?;
    if !password::verify(&b.password, &user.password_hash) {
        s.auth_limiter.check(&key);
        return Err(AppError::Validation("error.invalid_password").into());
    }
    let a = sq::prepare(&b.answer)?;
    let hash = password::hash(&a)?;
    sq::set_answer(&s.pool, u.user_id, b.question_id, &hash).await?;
    audit::record(&s.pool, Some(u.user_id), "account.security_question_set", None, u.ip.as_deref(), json!({ "question_id": b.question_id })).await?;
    Ok(Json(json!({ "current": sq::current(&s.pool, u.user_id).await? })))
}

#[utoipa::path(delete, path = "/api/v1/account/security-question", responses((status = 200), (status = 401)))]
pub async fn clear_mine(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    sq::clear(&s.pool, u.user_id).await?;
    audit::record(&s.pool, Some(u.user_id), "account.security_question_cleared", None, u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
