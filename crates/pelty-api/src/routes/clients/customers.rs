//! Customer profiles. `/client/*` is always scoped to the caller's own
//! customer record; `/admin/customers*` requires `customers.manage`.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::{paged, ListQuery};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::customers::{self, Profile};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/profile", get(client_profile).put(client_update))
        .route(
            "/client/contacts",
            get(client_contacts).post(client_add_contact),
        )
        .route(
            "/client/contacts/{id}",
            put(client_update_contact).delete(client_delete_contact),
        )
        .route(
            "/admin/customers/{id}/contacts/{cid}",
            put(admin_update_contact),
        )
        .route("/admin/customers", get(admin_list).post(admin_create))
        .route("/admin/customers/{id}", get(admin_get).patch(admin_update))
}

/// The caller's customer id (created on first use).
pub async fn customer_of(s: &AppState, u: &CurrentUser) -> ApiResult<Uuid> {
    // Sub-account contacts only reach their parent account through
    // `customer_for` with a granted permission; everything else is denied.
    if pelty_database::storefront::contact_for_user(&s.pool, u.user_id)
        .await?
        .is_some()
    {
        return Err(AppError::Forbidden.into());
    }
    Ok(s.clients().id_for_user(u.user_id).await?)
}

/// The customer whose `area` ("invoices", "tickets", "services") the caller
/// may use: their own account, or the parent account of a contact that was
/// granted that permission.
pub async fn customer_for(s: &AppState, u: &CurrentUser, area: &str) -> ApiResult<Uuid> {
    match pelty_database::storefront::contact_for_user(&s.pool, u.user_id).await? {
        Some(c) if c.permissions.iter().any(|p| p == area) => Ok(c.customer_id),
        Some(_) => Err(AppError::Forbidden.into()),
        None => Ok(s.clients().id_for_user(u.user_id).await?),
    }
}

#[utoipa::path(get, path = "/api/v1/client/profile", responses((status = 200), (status = 401)))]
pub async fn client_profile(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let id = customer_of(&s, &u).await?;
    let c = s.clients().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(c.for_client()))
}

/// `customer.before_update`: addons may cancel the change or rewrite any
/// profile field. A rewritten profile that no longer validates is ignored.
async fn pre_profile(
    s: &AppState,
    id: Option<Uuid>,
    p: Profile,
    by_staff: bool,
) -> ApiResult<Profile> {
    let mut v = serde_json::to_value(&p).unwrap_or_else(|_| json!({}));
    v["customer_id"] = json!(id);
    v["by_staff"] = json!(by_staff);
    let out = crate::addon_ext::pre_event(s, "customer.before_update", v).await?;
    let mut o = out.as_object().cloned().unwrap_or_default();
    o.remove("customer_id");
    o.remove("by_staff");
    Ok(match serde_json::from_value::<Profile>(Value::Object(o)) {
        Ok(mut np) => {
            if np.validate().is_ok() {
                np
            } else {
                p
            }
        }
        _ => p,
    })
}

#[utoipa::path(put, path = "/api/v1/client/profile", request_body = Profile, responses((status = 200), (status = 401), (status = 422)))]
pub async fn client_update(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(mut p): Json<Profile>,
) -> ApiResult<Json<Value>> {
    p.validate()?;
    let id = customer_of(&s, &u).await?;
    let p = pre_profile(&s, Some(id), p, false).await?;
    let c = s
        .clients()
        .update_profile(id, &p)
        .await?
        .ok_or(AppError::NotFound)?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.profile_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.updated",
        json!({ "customer_id": id, "by_staff": false }),
    ));
    Ok(Json(c.for_client()))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ContactBody {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub is_billing: bool,
}

#[utoipa::path(get, path = "/api/v1/client/contacts", responses((status = 200), (status = 401)))]
pub async fn client_contacts(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let id = customer_of(&s, &u).await?;
    Ok(Json(json!({ "items": s.clients().contacts(id).await? })))
}

#[utoipa::path(post, path = "/api/v1/client/contacts", request_body = ContactBody, responses((status = 200), (status = 422)))]
pub async fn client_add_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<ContactBody>,
) -> ApiResult<Json<Value>> {
    let id = customer_of(&s, &u).await?;
    let c = s
        .clients()
        .add_contact(id, &b.name, &b.email, &b.phone, b.is_billing)
        .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_added",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(c)))
}

#[utoipa::path(put, path = "/api/v1/client/contacts/{id}", request_body = ContactBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn client_update_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(cid): Path<Uuid>,
    Json(b): Json<ContactBody>,
) -> ApiResult<Json<Value>> {
    let id = customer_of(&s, &u).await?;
    let c = s
        .clients()
        .update_contact(id, cid, &b.name, &b.email, &b.phone, b.is_billing)
        .await?
        .ok_or(AppError::NotFound)?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_updated",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!(c)))
}

#[utoipa::path(delete, path = "/api/v1/client/contacts/{id}", responses((status = 200), (status = 404)))]
pub async fn client_delete_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(cid): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let id = customer_of(&s, &u).await?;
    if !s.clients().delete_contact(id, cid).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_deleted",
        Some(&cid.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(put, path = "/api/v1/admin/customers/{id}/contacts/{cid}", request_body = ContactBody, responses((status = 200), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_update_contact(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path((id, cid)): Path<(Uuid, Uuid)>,
    Json(b): Json<ContactBody>,
) -> ApiResult<Json<Value>> {
    let c = s
        .clients()
        .update_contact(id, cid, &b.name, &b.email, &b.phone, b.is_billing)
        .await?
        .ok_or(AppError::NotFound)?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_updated",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "by": "admin" }),
    )
    .await?;
    Ok(Json(json!(c)))
}

#[utoipa::path(get, path = "/api/v1/admin/customers", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CustomersManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = s.clients().list(q.q(), limit, offset).await?;
    Ok(Json(paged(items, page, per, total)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCustomer {
    pub email: String,
    /// `none` (record only, default), `password` (staff sets it) or
    /// `invite` (the client gets an email link to choose one).
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    /// Preferred language of the login (`en`, `ro`, ...).
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Send the welcome email (always sent for `invite`, which needs the link).
    #[serde(default)]
    pub send_welcome: bool,
    #[serde(flatten)]
    pub profile: Profile,
}

/// Link lifetime of the "set your password" welcome email.
const INVITE_TTL_DAYS: i64 = 7;

#[utoipa::path(post, path = "/api/v1/admin/customers", request_body = CreateCustomer, responses((status = 200), (status = 403), (status = 409), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Json(mut b): Json<CreateCustomer>,
) -> ApiResult<Json<Value>> {
    let email = b.email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    let login = b.login.as_deref().unwrap_or("none");
    if !["none", "password", "invite"].contains(&login) {
        return Err(AppError::Validation("error.invalid_login_mode").into());
    }
    if let Some(st) = b.status.as_deref() {
        if !["active", "inactive", "closed"].contains(&st) {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    if b.notes.as_ref().is_some_and(|n| n.chars().count() > 10_000) {
        return Err(AppError::Validation("error.field_too_long").into());
    }
    if let Some(l) = b.locale.as_deref() {
        if !s.i18n.has(l) {
            return Err(AppError::Validation("error.invalid_locale").into());
        }
    }
    b.profile.validate()?;
    let dup: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM customers WHERE lower(email::text) = $1)")
            .bind(&email)
            .fetch_one(&s.pool)
            .await
            .map_err(pelty_database::db_err)?;
    if dup {
        return Err(AppError::Conflict("error.customer_exists").into());
    }
    if login != "none"
        && pelty_database::users::find_by_email(&s.pool, &email)
            .await?
            .is_some()
    {
        return Err(AppError::Conflict("error.email_taken").into());
    }
    if let Some(g) = b.group_id {
        let ok: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM client_groups WHERE id = $1)")
                .bind(g)
                .fetch_one(&s.pool)
                .await
                .map_err(pelty_database::db_err)?;
        if !ok {
            return Err(AppError::Validation("error.invalid_group").into());
        }
    }
    // Cancel-only: refuse the account (duplicate CRM record, blocked domain …).
    crate::addon_ext::pre_event(
        &s,
        "customer.before_create",
        json!({ "email": email, "by_staff": true }),
    )
    .await?;
    let name = [
        b.profile.first_name.as_deref(),
        b.profile.last_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let name = if name.trim().is_empty() {
        b.profile
            .organization
            .clone()
            .unwrap_or_else(|| email.clone())
    } else {
        name
    };
    let user = match login {
        "none" => None,
        _ => {
            // Invite: a random password nobody knows until the link sets a real one.
            let pw = match (login, b.password.as_deref()) {
                ("password", Some(p)) if !p.is_empty() => p.to_string(),
                ("password", _) => {
                    return Err(AppError::Validation("error.password_required").into())
                }
                _ => format!("{}aA1!", pelty_auth::token::generate()),
            };
            let input = pelty_auth::RegisterInput {
                email: &email,
                password: &pw,
                display_name: &name,
            };
            Some(s.auth.register(input, "customer", u.ip.as_deref()).await?)
        }
    };
    let c = match &user {
        Some(usr) => {
            let id = s.clients().id_for_user(usr.id).await?;
            if let Some(l) = b.locale.as_deref() {
                pelty_database::users::set_locale(&s.pool, usr.id, Some(l)).await?;
            }
            s.clients()
                .update_profile(id, &b.profile)
                .await?
                .ok_or(AppError::NotFound)?
        }
        None => s.clients().create(&email, &b.profile).await?,
    };
    if b.status.is_some() || b.notes.is_some() {
        s.clients()
            .update_admin(c.id, b.status.as_deref(), b.notes.as_deref(), None)
            .await?;
    }
    if let Some(g) = b.group_id {
        pelty_database::client_groups::assign(&s.pool, c.id, Some(g)).await?;
    }
    if let Some(usr) = user
        .as_ref()
        .filter(|_| login == "invite" || b.send_welcome)
    {
        let mut link = format!("{}/login", s.cfg.public_url);
        if login == "invite" {
            let tok = pelty_auth::token::generate();
            pelty_database::user_tokens::issue(
                &s.pool,
                usr.id,
                pelty_database::user_tokens::PASSWORD_RESET,
                &pelty_auth::token::hash(&tok),
                time::Duration::days(INVITE_TTL_DAYS),
            )
            .await?;
            link = format!("{}/reset-password?token={tok}", s.cfg.public_url);
        }
        let locale = crate::mail::user_locale(&s, usr);
        let days = INVITE_TTL_DAYS.to_string();
        let tmpl = if login == "invite" {
            "client_invite"
        } else {
            "client_welcome"
        };
        crate::mail::enqueue(
            &s,
            tmpl,
            &locale,
            &email,
            crate::mail::vars(&[("name", &name), ("link", &link), ("expires_days", &days)]),
        )
        .await?;
    }
    let c = s.clients().get(c.id).await?.ok_or(AppError::NotFound)?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.created",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "login": login }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.created",
        json!({ "customer_id": c.id, "email": email, "by": u.user_id }),
    ));
    Ok(Json(json!(c)))
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = s.clients().get(id).await?.ok_or(AppError::NotFound)?;
    let contacts = s.clients().contacts(id).await?;
    Ok(Json(json!({ "customer": c, "contacts": contacts })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AdminUpdate {
    pub status: Option<String>,
    pub notes: Option<String>,
    pub tax_exempt: Option<bool>,
    #[serde(flatten)]
    pub profile: Profile,
}

#[utoipa::path(patch, path = "/api/v1/admin/customers/{id}", request_body = AdminUpdate, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
    Json(mut b): Json<AdminUpdate>,
) -> ApiResult<Json<Value>> {
    b.profile.validate()?;
    let profile = pre_profile(&s, Some(id), std::mem::take(&mut b.profile), true).await?;
    s.clients()
        .update_profile(id, &profile)
        .await?
        .ok_or(AppError::NotFound)?;
    let c = customers::update_admin(
        &s.pool,
        id,
        b.status.as_deref(),
        b.notes.as_deref(),
        b.tax_exempt,
    )
    .await?
    .ok_or(AppError::NotFound)?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.updated",
        json!({ "customer_id": id, "by_staff": true, "status": b.status }),
    ));
    Ok(Json(json!(c)))
}
