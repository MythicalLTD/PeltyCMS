//! Headless commerce API: the whole purchase flow (catalog → cart →
//! checkout → payment → status) for custom storefronts, mobile apps and
//! integrations. Authenticated with a customer API key
//! (`Authorization: Bearer pelty_…`); interactive sessions are refused so
//! the flow is never CSRF-exposed and the checkout captcha (meant for
//! browsers) does not apply. Prices are always computed server-side.
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::orders::CartItem;
use pelty_database::storefront::{self as sf, CartInput};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/headless/catalog", get(catalog))
        .route("/headless/carts", post(create_cart))
        .route("/headless/carts/{id}", get(get_cart).put(update_cart))
        .route("/headless/carts/{id}/checkout", post(checkout))
        .route("/headless/invoices/{id}/pay", post(pay))
        .route("/headless/orders/{id}", get(order_status))
}

fn key_only(u: &CurrentUser) -> ApiResult<()> {
    if u.api_key {
        Ok(())
    } else {
        Err(AppError::Forbidden.into())
    }
}

/// Catalog, enabled currencies and payment gateways in one call.
#[utoipa::path(get, path = "/api/v1/headless/catalog", tag = "headless", security(("api_key" = [])), responses((status = 200), (status = 401), (status = 403)))]
pub async fn catalog(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    let Json(cat) = crate::routes::catalog::public_catalog(State(s.clone())).await?;
    let Json(cur) = crate::routes::catalog::list_currencies(State(s.clone())).await?;
    let Json(gw) = crate::routes::store::store_gateways(State(s)).await?;
    Ok(Json(
        json!({ "groups": cat["groups"], "products": cat["products"], "currencies": cur["items"], "gateways": gw }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessCart {
    /// Defaults to the customer's currency.
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub country: String,
    pub items: Vec<CartItem>,
    #[serde(default)]
    pub coupon: Option<String>,
}

async fn save(s: &AppState, u: &CurrentUser, id: Uuid, b: HeadlessCart) -> ApiResult<Value> {
    let cid = customer_of(s, u).await?;
    if let Some(c) = sf::get_cart(&s.pool, id).await? {
        if c.customer_id != Some(cid) {
            return Err(AppError::NotFound.into());
        }
        if c.order_id.is_some() {
            return Err(AppError::Conflict("error.cart_closed").into());
        }
    }
    let currency = match b.currency {
        Some(c) => c,
        None => {
            s.clients()
                .get(cid)
                .await?
                .ok_or(AppError::NotFound)?
                .currency
        }
    };
    let cart = sf::save_cart(
        &s.pool,
        id,
        Some(cid),
        &CartInput {
            currency,
            country: b.country,
            items: b.items,
            coupon: b.coupon,
            email: None,
            locale: String::new(),
        },
    )
    .await?;
    crate::routes::store::cart_view(s, cart, Some(cid)).await
}

/// Create a server-side cart owned by the key's customer; the response holds
/// the live price/tax preview (or the validation error key).
#[utoipa::path(post, path = "/api/v1/headless/carts", tag = "headless", security(("api_key" = [])), request_body = HeadlessCart, responses((status = 200), (status = 403), (status = 422)))]
pub async fn create_cart(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<HeadlessCart>,
) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    Ok(Json(save(&s, &u, Uuid::new_v4(), b).await?))
}

#[utoipa::path(put, path = "/api/v1/headless/carts/{id}", tag = "headless", security(("api_key" = [])), request_body = HeadlessCart, responses((status = 200), (status = 404), (status = 409)))]
pub async fn update_cart(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<HeadlessCart>,
) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    if sf::get_cart(&s.pool, id).await?.is_none() {
        return Err(AppError::NotFound.into());
    }
    Ok(Json(save(&s, &u, id, b).await?))
}

#[utoipa::path(get, path = "/api/v1/headless/carts/{id}", tag = "headless", security(("api_key" = [])), responses((status = 200), (status = 404)))]
pub async fn get_cart(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    let cid = customer_of(&s, &u).await?;
    let cart = sf::get_cart(&s.pool, id)
        .await?
        .filter(|c| c.customer_id == Some(cid))
        .ok_or(AppError::NotFound)?;
    Ok(Json(
        crate::routes::store::cart_view(&s, cart, Some(cid)).await?,
    ))
}

/// Turn the cart into an order and a finalized invoice (fraud screening,
/// coupons and taxes as in the web checkout).
#[utoipa::path(post, path = "/api/v1/headless/carts/{id}/checkout", tag = "headless", security(("api_key" = [])), responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn checkout(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    let cid = customer_of(&s, &u).await?;
    let cart = sf::get_cart(&s.pool, id)
        .await?
        .filter(|c| c.customer_id == Some(cid))
        .ok_or(AppError::NotFound)?;
    if cart.order_id.is_some() {
        return Err(AppError::Conflict("error.cart_closed").into());
    }
    let items = sf::parse_items(&cart.items);
    if items.is_empty() {
        return Err(AppError::Validation("error.cart_invalid").into());
    }
    let (order, invoice) = crate::routes::billing::place_order(
        &s,
        &u,
        cid,
        &cart.currency,
        &items,
        cart.coupon.as_deref(),
        None,
    )
    .await?;
    sf::mark_converted(&s.pool, cart.id, cid, order.id).await?;
    Ok(Json(json!({ "order": order, "invoice": invoice })))
}

/// Start a payment for an invoice; returns the gateway redirect/action.
#[utoipa::path(post, path = "/api/v1/headless/invoices/{id}/pay", tag = "headless", security(("api_key" = [])), request_body = crate::routes::billing::PayBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn pay(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<crate::routes::billing::PayBody>,
) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    crate::routes::billing::client_pay(State(s), u, Path(id), Json(b)).await
}

/// Order status with its invoice and provisioned services.
#[utoipa::path(get, path = "/api/v1/headless/orders/{id}", tag = "headless", security(("api_key" = [])), responses((status = 200), (status = 404)))]
pub async fn order_status(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    key_only(&u)?;
    crate::routes::store::client_order(State(s), u, Path(id)).await
}
