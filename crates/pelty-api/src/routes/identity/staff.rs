//! Staff roles/permissions editor, staff list with role assignment and the
//! staff 2FA policy (`users.manage`).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::staff::{self, RoleInput};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/roles", get(list_roles).post(create_role))
        .route("/admin/roles/{id}", put(update_role).delete(delete_role))
        .route("/admin/staff", get(list_staff).post(add_staff))
        .route("/admin/users/{id}/roles", put(set_roles))
        .route("/admin/security/staff-2fa", get(get_2fa).put(put_2fa))
}

type Um = RequirePermission<perms::UsersManage>;

#[utoipa::path(get, path = "/api/v1/admin/roles", responses((status = 200), (status = 403)))]
pub async fn list_roles(State(s): State<AppState>, _p: Um) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": staff::roles(&s.pool).await?, "permissions": staff::permissions(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/roles", request_body = RoleInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create_role(State(s): State<AppState>, RequirePermission(u, _): Um, Json(b): Json<RoleInput>) -> ApiResult<Json<Value>> {
    let id = staff::create_role(&s.pool, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "role.created", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name.trim(), "permissions": b.permissions })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("role.created", json!({ "role_id": id, "name": b.name.trim(), "permissions": b.permissions })));
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(put, path = "/api/v1/admin/roles/{id}", request_body = RoleInput, responses((status = 200), (status = 404), (status = 409)))]
pub async fn update_role(State(s): State<AppState>, RequirePermission(u, _): Um, Path(id): Path<Uuid>, Json(b): Json<RoleInput>) -> ApiResult<Json<Value>> {
    staff::update_role(&s.pool, id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "role.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name.trim(), "permissions": b.permissions })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("role.updated", json!({ "role_id": id, "name": b.name.trim(), "permissions": b.permissions })));
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/roles/{id}", responses((status = 200), (status = 404), (status = 409)))]
pub async fn delete_role(State(s): State<AppState>, RequirePermission(u, _): Um, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    staff::delete_role(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "role.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("role.deleted", json!({ "role_id": id })));
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/staff", responses((status = 200), (status = 403)))]
pub async fn list_staff(State(s): State<AppState>, _p: Um) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": staff::staff(&s.pool).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AddStaff {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub roles: Vec<String>,
    /// `invite` (default: an email link to choose a password) or `password`.
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

const STAFF_INVITE_DAYS: i64 = 7;

/// Add a staff member: a brand-new account (invited by email or with a set
/// password), or staff roles added to an existing account with that email.
#[utoipa::path(post, path = "/api/v1/admin/staff", request_body = AddStaff, responses((status = 200), (status = 409), (status = 422)))]
pub async fn add_staff(State(s): State<AppState>, RequirePermission(u, _): Um, Json(b): Json<AddStaff>) -> ApiResult<Json<Value>> {
    let email = b.email.trim().to_lowercase();
    if email.len() > 254 || !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    let login = b.login.as_deref().unwrap_or("invite");
    if !["invite", "password"].contains(&login) {
        return Err(AppError::Validation("error.invalid_login_mode").into());
    }
    if !b.roles.iter().any(|r| r != "customer") {
        return Err(AppError::Validation("error.roles_required").into());
    }
    let (id, invited) = match pelty_database::users::find_by_email(&s.pool, &email).await? {
        Some(existing) => {
            let mut have = pelty_database::users::roles(&s.pool, existing.id).await?;
            if have.iter().any(|r| r != "customer") {
                return Err(AppError::Conflict("error.already_staff").into());
            }
            have.extend(b.roles.iter().cloned());
            staff::set_user_roles(&s.pool, u.user_id, existing.id, &have).await?;
            (existing.id, false)
        }
        None => {
            let pw = match (login, b.password.as_deref()) {
                ("password", Some(p)) if !p.is_empty() => p.to_string(),
                ("password", _) => return Err(AppError::Validation("error.password_required").into()),
                _ => format!("{}aA1!", pelty_auth::token::generate()),
            };
            let name = b.display_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&email).to_string();
            let input = pelty_auth::RegisterInput { email: &email, password: &pw, display_name: &name };
            let usr = s.auth.register(input, "customer", u.ip.as_deref()).await?;
            if let Err(e) = staff::set_user_roles(&s.pool, u.user_id, usr.id, &b.roles).await {
                // Don't leave a half-made account behind when a role name was bad.
                let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(usr.id).execute(&s.pool).await;
                return Err(e.into());
            }
            if login == "invite" {
                let tok = pelty_auth::token::generate();
                pelty_database::user_tokens::issue(&s.pool, usr.id, pelty_database::user_tokens::PASSWORD_RESET, &pelty_auth::token::hash(&tok), time::Duration::days(STAFF_INVITE_DAYS)).await?;
                let link = format!("{}/reset-password?token={tok}", s.cfg.public_url);
                let locale = crate::mail::user_locale(&s, &usr);
                let days = STAFF_INVITE_DAYS.to_string();
                crate::mail::enqueue(&s, "staff_invite", &locale, &email, crate::mail::vars(&[("name", &name), ("link", &link), ("expires_days", &days)])).await?;
            }
            (usr.id, login == "invite")
        }
    };
    audit::record(&s.pool, Some(u.user_id), "staff.added", Some(&id.to_string()), u.ip.as_deref(), json!({ "roles": b.roles, "invited": invited })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("user.roles_changed", json!({ "user_id": id, "roles": b.roles, "by": u.user_id })));
    Ok(Json(json!({ "id": id, "invited": invited })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RolesBody {
    pub roles: Vec<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/users/{id}/roles", request_body = RolesBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn set_roles(State(s): State<AppState>, RequirePermission(u, _): Um, Path(id): Path<Uuid>, Json(b): Json<RolesBody>) -> ApiResult<Json<Value>> {
    staff::set_user_roles(&s.pool, u.user_id, id, &b.roles).await?;
    audit::record(&s.pool, Some(u.user_id), "user.roles_changed", Some(&id.to_string()), u.ip.as_deref(), json!({ "roles": b.roles })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("user.roles_changed", json!({ "user_id": id, "roles": b.roles, "by": u.user_id })));
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/security/staff-2fa", responses((status = 200)))]
pub async fn get_2fa(State(s): State<AppState>, _p: Um) -> ApiResult<Json<Value>> {
    let missing = staff::staff(&s.pool).await?.into_iter().filter(|m| !m.mfa_enabled).count();
    Ok(Json(json!({ "required": staff::require_2fa(&s.pool).await?, "staff_without_2fa": missing })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TwoFaBody {
    pub required: bool,
}

/// Turning the policy on requires the actor to have 2FA (no self lock-out).
#[utoipa::path(put, path = "/api/v1/admin/security/staff-2fa", request_body = TwoFaBody, responses((status = 200), (status = 409)))]
pub async fn put_2fa(State(s): State<AppState>, RequirePermission(u, _): Um, Json(b): Json<TwoFaBody>) -> ApiResult<Json<Value>> {
    if b.required && !pelty_database::mfa::is_enabled(&s.pool, u.user_id).await? && !pelty_database::mfa::passkey_2fa(&s.pool, u.user_id).await? {
        return Err(AppError::Conflict("error.mfa_required_self").into());
    }
    pelty_database::settings::set(&s.pool, staff::REQUIRE_2FA_KEY, json!(b.required)).await?;
    audit::record(&s.pool, Some(u.user_id), "security.staff_2fa", None, u.ip.as_deref(), json!({ "required": b.required })).await?;
    Ok(Json(json!({ "ok": true })))
}
