//! Public storefront (no login): carts with a live price/tax preview,
//! storefront checkout for a signed-in customer, the client order status
//! page, the client home widgets, contacts / sub-accounts, email
//! preferences, login history and the cart-recovery email job.
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::routes::customers::{customer_for, customer_of};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::orders::{self, CartItem};
use pelty_database::storefront::{self as sf, CartInput, EmailPrefs};
use pelty_database::{audit, sessions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const RECOVERY_JOB: &str = "store.cart_recovery";
/// A cart counts as abandoned after this many idle minutes.
pub const IDLE_MINUTES: i64 = 120;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/store/gateways", get(store_gateways))
        .route("/store/carts/{id}", get(get_cart).put(put_cart))
        .route("/client/checkout", post(checkout))
        .route("/client/orders/{id}", get(client_order))
        .route("/client/home", get(client_home))
        .route("/client/subaccounts", get(list_contacts).post(add_contact))
        .route("/client/subaccounts/invite", post(invite_contact))
        .route("/admin/customers/{id}/subaccounts", get(admin_contacts))
        .route(
            "/client/subaccounts/{id}",
            put(update_contact).delete(remove_contact),
        )
        .route("/client/email-preferences", get(get_prefs).put(put_prefs))
        .route(
            "/client/dashboard/layout",
            get(get_layout).put(put_layout).delete(reset_layout),
        )
        .route("/client/login-history", get(login_history))
        .route("/client/access", get(client_access))
}

/// Optional session: the storefront works signed out, but a signed-in
/// customer gets their own tax country and client-group discount.
pub(crate) async fn maybe_customer(s: &AppState, parts: &axum::http::HeaderMap) -> Option<Uuid> {
    let tok = crate::extract::cookie(parts, crate::extract::SESSION_COOKIE)?;
    let ctx = pelty_database::sessions::resolve(
        &s.pool,
        &pelty_auth::token::hash(&tok),
        s.auth.idle,
        time::Duration::seconds(60),
    )
    .await
    .ok()??;
    if sf::contact_for_user(&s.pool, ctx.user_id)
        .await
        .ok()?
        .is_some()
    {
        return None;
    }
    s.clients().id_for_user(ctx.user_id).await.ok()
}

#[utoipa::path(get, path = "/api/v1/store/gateways", responses((status = 200)))]
pub async fn store_gateways(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let ids: Vec<String> = pelty_database::extensions::gateways(&s.pool)
        .await?
        .into_iter()
        .filter(|g| g.enabled)
        .map(|g| g.id)
        .collect();
    Ok(Json(json!({ "items": ids })))
}

pub(crate) async fn cart_view(
    s: &AppState,
    cart: sf::Cart,
    viewer: Option<Uuid>,
) -> ApiResult<Value> {
    let ctx = json!({ "cart_id": cart.id, "customer_id": viewer.or(cart.customer_id) });
    Ok(
        crate::addon_functions::run(s, "cart.view", ctx, (), |()| async move {
            cart_view_core(s, cart, viewer).await.map_err(|e| e.0)
        })
        .await?,
    )
}

async fn cart_view_core(s: &AppState, cart: sf::Cart, viewer: Option<Uuid>) -> ApiResult<Value> {
    let items = sf::parse_items(&cart.items);
    let who = viewer.or(cart.customer_id);
    let preview = sf::preview(
        &s.pool,
        who,
        &cart.country,
        &cart.currency,
        &items,
        cart.coupon.as_deref(),
        cart.creator_code.as_deref(),
    )
    .await;
    let (preview, error) = match preview {
        Ok(p) => (json!(p), Value::Null),
        Err(AppError::Validation(k)) | Err(AppError::Conflict(k)) => (Value::Null, json!(k)),
        Err(AppError::NotFound) => (Value::Null, json!("error.product_unavailable")),
        Err(e) => return Err(e.into()),
    };
    let mut c = json!(cart);
    if let Some(o) = c.as_object_mut() {
        o.remove("customer_id");
    }
    Ok(json!({ "cart": c, "preview": preview, "error": error }))
}

#[utoipa::path(get, path = "/api/v1/store/carts/{id}", responses((status = 200), (status = 404)))]
pub async fn get_cart(
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cart = sf::get_cart(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let viewer = maybe_customer(&s, &headers).await;
    Ok(Json(cart_view(&s, cart, viewer).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CartBody {
    pub currency: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub items: Vec<CartItem>,
    #[serde(default)]
    pub coupon: Option<String>,
    /// Creator code (module `creator_codes`; ignored while it is off).
    #[serde(default)]
    pub creator_code: Option<String>,
    /// Optional email for cart recovery reminders.
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub locale: String,
}

/// Create or replace a cart. The id is an unguessable client-generated
/// UUID v4 that acts as the cart token.
#[utoipa::path(put, path = "/api/v1/store/carts/{id}", request_body = CartBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn put_cart(
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<CartBody>,
) -> ApiResult<Json<Value>> {
    if id.get_version_num() != 4 {
        return Err(AppError::Validation("error.cart_invalid").into());
    }
    let viewer = maybe_customer(&s, &headers).await;
    let cart = sf::save_cart(
        &s.pool,
        id,
        viewer,
        &CartInput {
            currency: b.currency,
            country: b.country,
            items: b.items,
            coupon: b.coupon,
            creator_code: b.creator_code,
            email: b.email,
            locale: b.locale,
        },
    )
    .await?;
    Ok(Json(cart_view(&s, cart, viewer).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckoutBody {
    pub cart_id: Uuid,
    /// Custom order field values per cart line (index → field_id → value).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub fields: std::collections::HashMap<usize, std::collections::HashMap<Uuid, String>>,
    /// Billing country to store on the profile when it is still empty.
    #[serde(default)]
    pub country: Option<String>,
}

/// Turn a cart into an order + invoice for the signed-in customer. The
/// client then pays the invoice with the chosen gateway.
#[utoipa::path(post, path = "/api/v1/client/checkout", request_body = CheckoutBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn checkout(
    State(s): State<AppState>,
    u: CurrentUser,
    headers: axum::http::HeaderMap,
    Json(b): Json<CheckoutBody>,
) -> ApiResult<Json<Value>> {
    crate::captcha::require(&s, &headers, "checkout", u.ip.as_deref()).await?;
    let cid = customer_for(&s, &u, "orders").await?;
    let cart = sf::get_cart(&s.pool, b.cart_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if cart.order_id.is_some() {
        return Err(AppError::Conflict("error.cart_closed").into());
    }
    if cart.customer_id.is_some_and(|c| c != cid) {
        return Err(AppError::NotFound.into());
    }
    let items = sf::parse_items(&cart.items);
    if items.is_empty() {
        return Err(AppError::Validation("error.cart_invalid").into());
    }
    let input = CheckoutInput {
        fields: b.fields,
        country: b.country,
    };
    Ok(Json(place_cart(&s, &u, cid, &cart, items, input).await?))
}

/// Place an order + invoice for `cid` from an open cart (signed-in or guest
/// checkout): order fields, profile country, the bundle the cart holds, and
/// the addon hook `checkout.place`.
pub(crate) async fn place_cart(
    s: &AppState,
    u: &CurrentUser,
    cid: Uuid,
    cart: &sf::Cart,
    mut items: Vec<CartItem>,
    input: CheckoutInput,
) -> ApiResult<Value> {
    let ctx = json!({ "customer_id": cid, "cart_id": cart.id });
    let out = crate::addon_functions::run(
        s,
        "checkout.place",
        ctx,
        input,
        |b: CheckoutInput| async move {
            for (i, it) in items.iter_mut().enumerate() {
                if let Some(f) = b.fields.get(&i) {
                    it.fields = f.clone();
                }
            }
            if let Some(c) = b
                .country
                .as_deref()
                .map(|c| c.trim().to_ascii_uppercase())
                .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()))
            {
                sf::set_country_if_empty(&s.pool, cid, &c).await?;
            }
            // A bundle the cart fully holds applies on its own (no code).
            let bundle = sf::best_bundle(&s.pool, &items, Some(cid), &cart.currency).await?;
            let (order, invoice) = crate::routes::billing::place_order(
                s,
                u,
                cid,
                &cart.currency,
                &items,
                cart.coupon.as_deref(),
                bundle.as_ref(),
                cart.creator_code.as_deref(),
            )
            .await
            .map_err(|e| e.0)?;
            sf::mark_converted(&s.pool, cart.id, cid, order.id).await?;
            Ok(json!({ "order": order, "invoice": invoice }))
        },
    )
    .await?;
    Ok(out)
}

/// The rewritable part of a checkout (`fn checkout.place`).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CheckoutInput {
    pub(crate) fields: std::collections::HashMap<usize, std::collections::HashMap<Uuid, String>>,
    pub(crate) country: Option<String>,
}

/// Order status page for the customer.
#[utoipa::path(get, path = "/api/v1/client/orders/{id}", responses((status = 200), (status = 404)))]
pub async fn client_order(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "orders").await?;
    let o = s
        .orders()
        .get(id)
        .await?
        .filter(|o| o.customer_id == cid)
        .ok_or(AppError::NotFound)?;
    let invoice = match o.invoice_id {
        Some(i) => s.invoices().get_owned(i, cid).await?,
        None => None,
    };
    let services = sf::order_services(&s.pool, id).await?;
    let v = json!({ "order": o, "items": s.orders().items(id).await?, "invoice": invoice, "services": services });
    if !crate::addon_filters::has(&s, "order.view") {
        return Ok(Json(v));
    }
    Ok(Json(
        crate::addon_filters::apply_with(
            &s,
            "order.view",
            v,
            json!({ "customer_id": cid, "order_id": id }),
            false,
        )
        .await,
    ))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct LangQuery {
    pub locale: Option<String>,
}

/// What the signed-in user may use: owner (everything) or a contact's
/// granted areas.
async fn access(s: &AppState, u: &CurrentUser) -> ApiResult<(Uuid, sf::HomeScope, bool)> {
    Ok(match sf::contact_for_user(&s.pool, u.user_id).await? {
        Some(c) => {
            let has = |p: &str| c.permissions.iter().any(|x| x == p);
            (
                c.customer_id,
                sf::HomeScope {
                    services: has("services"),
                    invoices: has("invoices"),
                    tickets: has("tickets"),
                },
                false,
            )
        }
        None => (
            customer_of(s, u).await?,
            sf::HomeScope {
                services: true,
                invoices: true,
                tickets: true,
            },
            true,
        ),
    })
}

#[utoipa::path(get, path = "/api/v1/client/access", responses((status = 200), (status = 401)))]
pub async fn client_access(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let (_, sc, owner) = access(&s, &u).await?;
    let extra = match sf::contact_for_user(&s.pool, u.user_id).await? {
        Some(c) => (
            c.permissions.iter().any(|p| p == "orders"),
            c.permissions.iter().any(|p| p == "domains"),
        ),
        None => (true, true),
    };
    Ok(Json(
        json!({ "owner": owner, "services": sc.services, "invoices": sc.invoices, "tickets": sc.tickets, "orders": extra.0, "domains": extra.1 }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/home", params(LangQuery), responses((status = 200), (status = 401)))]
pub async fn client_home(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<LangQuery>,
) -> ApiResult<Json<Value>> {
    let (cid, scope, owner) = access(&s, &u).await?;
    let locale = q
        .locale
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let mut v = sf::home(&s.pool, cid, scope, &locale).await?;
    v["owner"] = json!(owner);
    Ok(Json(v))
}

// contacts

#[utoipa::path(get, path = "/api/v1/client/subaccounts", responses((status = 200), (status = 403)))]
pub async fn list_contacts(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    Ok(Json(json!({ "items": sf::contacts(&s.pool, cid).await? })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewContact {
    pub email: String,
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Create a sub-account that signs in with its own email and password and
/// sees only the granted areas of this account.
#[utoipa::path(post, path = "/api/v1/client/subaccounts", request_body = NewContact, responses((status = 200), (status = 409), (status = 422)))]
pub async fn add_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<NewContact>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let perms = sf::check_perms(&b.permissions)?;
    if b.name.trim().is_empty() {
        return Err(AppError::Validation("error.name_required").into());
    }
    let n = sf::contact_count(&s.pool, cid).await?;
    if n >= 25 {
        return Err(AppError::Validation("error.too_many_contacts").into());
    }
    let user = s
        .auth
        .register(
            pelty_auth::RegisterInput {
                email: &b.email,
                password: &b.password,
                display_name: b.name.trim(),
            },
            "customer",
            u.ip.as_deref(),
        )
        .await?;
    let c = sf::add_contact(&s.pool, cid, user.id, &b.name, &perms).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_added",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "permissions": perms }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.contact_added",
        json!({ "customer_id": cid, "contact_id": c.id, "user_id": user.id, "permissions": perms }),
    ));
    Ok(Json(json!(c)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteContact {
    pub email: String,
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

/// How long an invitation link stays valid.
pub const INVITE_TTL_DAYS: i64 = 7;

/// Invite a contact by email: the account is created with an unusable random
/// password and the invitee picks their own through a one-time link.
#[utoipa::path(post, path = "/api/v1/client/subaccounts/invite", request_body = InviteContact, responses((status = 200), (status = 409), (status = 422)))]
pub async fn invite_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<InviteContact>,
) -> ApiResult<Json<Value>> {
    use pelty_auth::token;
    let cid = customer_of(&s, &u).await?;
    let perms = sf::check_perms(&b.permissions)?;
    if b.name.trim().is_empty() {
        return Err(AppError::Validation("error.name_required").into());
    }
    if sf::contact_count(&s.pool, cid).await? >= 25 {
        return Err(AppError::Validation("error.too_many_contacts").into());
    }
    // Never revealed or mailed; replaced when the invite is accepted.
    let throwaway = format!("Aa1!{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let user = s
        .auth
        .register(
            pelty_auth::RegisterInput {
                email: &b.email,
                password: &throwaway,
                display_name: b.name.trim(),
            },
            "customer",
            u.ip.as_deref(),
        )
        .await?;
    let c = sf::add_contact(&s.pool, cid, user.id, &b.name, &perms).await?;
    sf::mark_invited(&s.pool, c.id).await?;
    let tok = token::generate();
    pelty_database::user_tokens::issue(
        &s.pool,
        user.id,
        pelty_database::user_tokens::PASSWORD_RESET,
        &token::hash(&tok),
        time::Duration::days(INVITE_TTL_DAYS),
    )
    .await?;
    let link = format!("{}/reset-password?token={tok}", s.cfg.public_url);
    let locale = b
        .locale
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let owner = s
        .clients()
        .get(cid)
        .await?
        .map(|c| {
            format!("{} {}", c.first_name, c.last_name)
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    let days = INVITE_TTL_DAYS.to_string();
    crate::mail::enqueue(
        &s,
        "contact_invite",
        &locale,
        &user.email,
        crate::mail::vars(&[
            ("name", b.name.trim()),
            ("inviter", &owner),
            ("link", &link),
            ("expires_days", &days),
        ]),
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_invited",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "permissions": perms }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.contact_invited",
        json!({ "customer_id": cid, "contact_id": c.id, "permissions": perms }),
    ));
    Ok(Json(json!(c)))
}

/// Staff view of an account's sub-accounts (WHMCS "Users" tab).
#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/subaccounts", responses((status = 200), (status = 403)))]
pub async fn admin_contacts(
    State(s): State<AppState>,
    _p: crate::extract::RequirePermission<crate::extract::perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": sf::contacts(&s.pool, id).await?, "permissions": sf::CONTACT_PERMS }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ContactPatch {
    pub name: String,
    pub permissions: Vec<String>,
}

#[utoipa::path(put, path = "/api/v1/client/subaccounts/{id}", request_body = ContactPatch, responses((status = 200), (status = 404)))]
pub async fn update_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<ContactPatch>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let perms = sf::check_perms(&b.permissions)?;
    if !sf::update_contact(&s.pool, cid, id, &b.name, &perms).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "permissions": perms }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.contact_updated",
        json!({ "customer_id": cid, "contact_id": id, "permissions": perms }),
    ));
    Ok(Json(json!({ "ok": true })))
}

/// Remove a contact: its access ends immediately (sessions revoked) and
/// the login is disabled.
#[utoipa::path(delete, path = "/api/v1/client/subaccounts/{id}", responses((status = 200), (status = 404)))]
pub async fn remove_contact(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let user = sf::remove_contact(&s.pool, cid, id)
        .await?
        .ok_or(AppError::NotFound)?;
    sessions::revoke_all(&s.pool, user).await?;
    sf::disable_user(&s.pool, user).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "customer.contact_removed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "customer.contact_removed",
        json!({ "customer_id": cid, "contact_id": id }),
    ));
    Ok(Json(json!({ "ok": true })))
}

// prefs and history

/// The caller's client dashboard layout (order, hidden widgets, sizes).
#[utoipa::path(get, path = "/api/v1/client/dashboard/layout", responses((status = 200, body = pelty_database::admin_ops::ClientLayout)))]
pub async fn get_layout(
    State(s): State<AppState>,
    u: CurrentUser,
) -> ApiResult<Json<pelty_database::admin_ops::ClientLayout>> {
    Ok(Json(
        pelty_database::admin_ops::client_layout(&s.pool, u.user_id).await?,
    ))
}

#[utoipa::path(put, path = "/api/v1/client/dashboard/layout", request_body = pelty_database::admin_ops::ClientLayout, responses((status = 200), (status = 422)))]
pub async fn put_layout(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<pelty_database::admin_ops::ClientLayout>,
) -> ApiResult<Json<pelty_database::admin_ops::ClientLayout>> {
    Ok(Json(
        pelty_database::admin_ops::save_client_layout(&s.pool, u.user_id, &b).await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/client/dashboard/layout", responses((status = 200)))]
pub async fn reset_layout(
    State(s): State<AppState>,
    u: CurrentUser,
) -> ApiResult<Json<pelty_database::admin_ops::ClientLayout>> {
    pelty_database::admin_ops::reset_client_layout(&s.pool, u.user_id).await?;
    Ok(Json(pelty_database::admin_ops::default_client_layout()))
}

#[utoipa::path(get, path = "/api/v1/client/email-preferences", responses((status = 200)))]
pub async fn get_prefs(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<EmailPrefs>> {
    let cid = customer_of(&s, &u).await?;
    Ok(Json(sf::email_prefs(&s.pool, cid).await?))
}

#[utoipa::path(put, path = "/api/v1/client/email-preferences", request_body = EmailPrefs, responses((status = 200)))]
pub async fn put_prefs(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<EmailPrefs>,
) -> ApiResult<Json<EmailPrefs>> {
    let cid = customer_of(&s, &u).await?;
    sf::put_email_prefs(&s.pool, cid, &b).await?;
    Ok(Json(b))
}

/// The caller's own recent sign-ins, failures and lockouts.
#[utoipa::path(get, path = "/api/v1/client/login-history", responses((status = 200)))]
pub async fn login_history(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": sf::login_history(&s.pool, u.user_id, 50).await? }),
    ))
}

// cart recovery job

/// Enqueue one recovery pass per hour (deduplicated).
pub async fn schedule_recovery(s: &AppState) -> pelty_core::AppResult<Option<i64>> {
    let key = format!(
        "{RECOVERY_JOB}:{}",
        time::OffsetDateTime::now_utc().unix_timestamp() / 3600
    );
    pelty_database::jobs::enqueue(&s.pool, RECOVERY_JOB, json!({}), None, Some(&key), 3).await
}

/// Email each abandoned cart once, with a link that restores it. Respects
/// the customer's marketing email preference.
pub async fn run_recovery(s: &AppState, idle_minutes: i64) -> pelty_core::AppResult<usize> {
    let mut sent = 0;
    for c in sf::abandoned(&s.pool, idle_minutes, 200).await? {
        if !sf::mark_reminded(&s.pool, c.id).await? {
            continue;
        }
        let locale = Some(c.locale.clone())
            .filter(|l| s.i18n.has(l))
            .unwrap_or_else(|| s.cfg.default_locale.clone());
        let link = format!("{}/cart?c={}", s.cfg.public_url, c.id);
        crate::mail::enqueue(
            s,
            "cart_abandoned",
            &locale,
            &c.email,
            crate::mail::vars(&[("items", &c.items.to_string()), ("link", &link)]),
        )
        .await?;
        sent += 1;
    }
    Ok(sent)
}

// admin orders

/// Full order detail for the admin order page: customer, lines, invoice,
/// latest fraud screening, notes and created services.
pub async fn admin_order_detail(s: &AppState, o: orders::Order) -> ApiResult<Value> {
    let ctx = json!({ "order_id": o.id, "customer_id": o.customer_id });
    Ok(
        crate::addon_functions::run(s, "order.admin_detail", ctx, (), |()| async move {
            admin_order_detail_core(s, o).await.map_err(|e| e.0)
        })
        .await?,
    )
}

async fn admin_order_detail_core(s: &AppState, o: orders::Order) -> ApiResult<Value> {
    let customer = s.clients().get(o.customer_id).await?;
    let invoice = match o.invoice_id {
        Some(i) => s.invoices().get(i).await?,
        None => None,
    };
    let services = sf::order_services(&s.pool, o.id).await?;
    Ok(json!({
        "order": o,
        "items": s.orders().items(o.id).await?,
        "customer": customer.map(|c| json!({ "id": c.id, "email": c.email, "first_name": c.first_name, "last_name": c.last_name, "organization": c.organization, "country": c.country })),
        "invoice": invoice,
        "fraud": sf::order_fraud(&s.pool, o.id).await?,
        "notes": sf::order_notes(&s.pool, o.id).await?,
        "services": services,
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdminOrderBody {
    pub customer_id: Uuid,
    pub currency: Option<String>,
    pub items: Vec<CartItem>,
    #[serde(default)]
    pub coupon: Option<String>,
    /// Approve (activate services) right away.
    #[serde(default)]
    pub approve: bool,
}

/// Manual order for a customer, priced from the catalog like a client order.
#[utoipa::path(post, path = "/api/v1/admin/orders", request_body = AdminOrderBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn admin_create_order(
    State(s): State<AppState>,
    crate::extract::RequirePermission(u, _): crate::extract::RequirePermission<
        crate::extract::perms::OrdersManage,
    >,
    Json(b): Json<AdminOrderBody>,
) -> ApiResult<Json<Value>> {
    let c = s
        .clients()
        .get(b.customer_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let currency = b.currency.unwrap_or(c.currency);
    let (order, invoice) = crate::routes::billing::place_order(
        &s,
        &u,
        c.id,
        &currency,
        &b.items,
        b.coupon.as_deref(),
        None,
        None,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.created_by_admin",
        Some(&order.id.to_string()),
        u.ip.as_deref(),
        json!({ "customer_id": c.id }),
    )
    .await?;
    let mut services = vec![];
    if b.approve {
        services = s.orders().approve(order.id, u.user_id).await?;
        s.emit(pelty_extensions::events::DomainEvent::new(
            "order.approved",
            json!({ "order_id": order.id, "services": services }),
        ));
        for sid in &services {
            s.emit(pelty_extensions::events::DomainEvent::new(
                "service.created",
                json!({ "service_id": sid, "order_id": order.id }),
            ));
            crate::provision::on_transition(&s, *sid, "pending", "active").await;
        }
    }
    Ok(Json(
        json!({ "order": order, "invoice": invoice, "services": services }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteBody {
    pub body: String,
}

#[utoipa::path(post, path = "/api/v1/admin/orders/{id}/notes", request_body = NoteBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_add_note(
    State(s): State<AppState>,
    crate::extract::RequirePermission(u, _): crate::extract::RequirePermission<
        crate::extract::perms::OrdersManage,
    >,
    Path(id): Path<Uuid>,
    Json(b): Json<NoteBody>,
) -> ApiResult<Json<Value>> {
    let nid = sf::add_order_note(&s.pool, id, u.user_id, &b.body).await?;
    Ok(Json(json!({ "id": nid })))
}

/// Refund an order: credit the paid invoice in full (credit note) and close
/// the order. Unpaid orders must be cancelled instead.
#[utoipa::path(post, path = "/api/v1/admin/orders/{id}/refund", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_refund_order(
    State(s): State<AppState>,
    crate::extract::RequirePermission(u, _): crate::extract::RequirePermission<
        crate::extract::perms::OrdersManage,
    >,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    if !u.has("invoices.manage") {
        return Err(AppError::Forbidden.into());
    }
    let o = s.orders().get(id).await?.ok_or(AppError::NotFound)?;
    let inv = match o.invoice_id {
        Some(i) => s.invoices().get(i).await?.ok_or(AppError::NotFound)?,
        None => return Err(AppError::Conflict("error.order_not_paid").into()),
    };
    if inv.status != "paid" {
        return Err(AppError::Conflict("error.order_not_paid").into());
    }
    let cn = s.invoices().credit_note(inv.id).await?;
    sf::mark_order_refunded(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.refunded",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "credit_note": cn.id }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "invoice.refunded",
        json!({ "invoice_id": inv.id, "credit_note_id": cn.id }),
    ));
    s.emit(pelty_extensions::events::DomainEvent::new(
        "order.refunded",
        json!({ "order_id": id, "credit_note_id": cn.id }),
    ));
    Ok(Json(json!({ "credit_note": cn })))
}
