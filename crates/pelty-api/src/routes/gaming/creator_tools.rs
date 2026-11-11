//! Creator tools (module `creator_marketplace`): discount codes and bundles
//! made by creators for their own listings, customer wishlists, buyer <->
//! creator conversations (mail on the first unread message) and per-listing
//! analytics (views, sales, revenue, conversion).
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::routes::customers::customer_of;
use crate::routes::marketplace::{approved, me};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::creator_tools::{self as ct, CreatorBundleInput, CreatorCouponInput};
use pelty_database::marketplace as mk;
use pelty_extensions::events::DomainEvent;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/client/creator/coupons",
            get(list_coupons).post(create_coupon),
        )
        .route(
            "/client/creator/coupons/{id}",
            put(update_coupon).delete(delete_coupon),
        )
        .route(
            "/client/creator/bundles",
            get(list_bundles).post(create_bundle),
        )
        .route(
            "/client/creator/bundles/{id}",
            put(update_bundle).delete(delete_bundle),
        )
        .route(
            "/client/creator/listings/{id}/analytics",
            get(listing_analytics),
        )
        .route("/client/wishlist", get(wishlist))
        .route("/client/wishlist/{product_id}", put(wish).delete(unwish))
        .route(
            "/client/market-messages",
            get(inbox).post(start_conversation),
        )
        .route("/client/market-messages/{id}", get(thread))
        .route("/client/market-messages/{id}/reply", post(reply))
        .route("/client/market-messages/{id}/status", post(set_status))
}

// ------------------------------------------------------------- discount codes

#[utoipa::path(get, path = "/api/v1/client/creator/coupons", responses((status = 200), (status = 404)))]
pub async fn list_coupons(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    let items = ct::coupons(&s.pool, c.id).await?;
    Ok(Json(
        json!({ "items": items, "prefix": format!("{}-", c.slug.to_ascii_uppercase()) }),
    ))
}

#[utoipa::path(post, path = "/api/v1/client/creator/coupons", request_body = CreatorCouponInput, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn create_coupon(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<CreatorCouponInput>,
) -> ApiResult<Json<Value>> {
    let (c, _) = approved(&s, &u).await?;
    let coupon = ct::save_coupon(&s.pool, c.id, &c.slug, None, &b).await?;
    audit(
        &s,
        &u,
        "creator.coupon_saved",
        coupon.id,
        json!({ "code": coupon.code }),
    )
    .await?;
    Ok(Json(json!(coupon)))
}

#[utoipa::path(put, path = "/api/v1/client/creator/coupons/{id}", request_body = CreatorCouponInput, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn update_coupon(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<CreatorCouponInput>,
) -> ApiResult<Json<Value>> {
    let (c, _) = approved(&s, &u).await?;
    let coupon = ct::save_coupon(&s.pool, c.id, &c.slug, Some(id), &b).await?;
    audit(
        &s,
        &u,
        "creator.coupon_saved",
        coupon.id,
        json!({ "code": coupon.code }),
    )
    .await?;
    Ok(Json(json!(coupon)))
}

/// Deletes an unused code; a used one is switched off (`deleted: false`).
#[utoipa::path(delete, path = "/api/v1/client/creator/coupons/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_coupon(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    let deleted = ct::delete_coupon(&s.pool, c.id, id).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

async fn audit(
    s: &AppState,
    u: &CurrentUser,
    action: &str,
    id: Uuid,
    meta: Value,
) -> ApiResult<()> {
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        action,
        Some(&id.to_string()),
        u.ip.as_deref(),
        meta,
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------------- bundles

#[utoipa::path(get, path = "/api/v1/client/creator/bundles", responses((status = 200), (status = 404)))]
pub async fn list_bundles(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    Ok(Json(json!({ "items": ct::bundles(&s.pool, c.id).await? })))
}

#[utoipa::path(post, path = "/api/v1/client/creator/bundles", request_body = CreatorBundleInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn create_bundle(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<CreatorBundleInput>,
) -> ApiResult<Json<Value>> {
    let (c, _) = approved(&s, &u).await?;
    let bundle = ct::save_bundle(&s.pool, c.id, None, &b).await?;
    audit(
        &s,
        &u,
        "creator.bundle_saved",
        bundle.id,
        json!({ "name": bundle.name }),
    )
    .await?;
    Ok(Json(json!(bundle)))
}

#[utoipa::path(put, path = "/api/v1/client/creator/bundles/{id}", request_body = CreatorBundleInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update_bundle(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<CreatorBundleInput>,
) -> ApiResult<Json<Value>> {
    let (c, _) = approved(&s, &u).await?;
    let bundle = ct::save_bundle(&s.pool, c.id, Some(id), &b).await?;
    audit(
        &s,
        &u,
        "creator.bundle_saved",
        bundle.id,
        json!({ "name": bundle.name }),
    )
    .await?;
    Ok(Json(json!(bundle)))
}

#[utoipa::path(delete, path = "/api/v1/client/creator/bundles/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_bundle(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    ct::delete_bundle(&s.pool, c.id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}

// ----------------------------------------------------------------- analytics

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct StatsQuery {
    /// 7 to 365 (default 30).
    pub days: Option<i64>,
    /// Currency for revenue (default: the listing's first price currency).
    pub currency: Option<String>,
}

/// Per-day views, sales, revenue and earnings of one of the creator's listings.
#[utoipa::path(get, path = "/api/v1/client/creator/listings/{id}/analytics", params(StatsQuery), responses((status = 200), (status = 404)))]
pub async fn listing_analytics(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    let l = mk::listing(&s.pool, id).await?;
    if l.creator_id != c.id {
        return Err(AppError::NotFound.into());
    }
    let days = q.days.unwrap_or(30).clamp(7, 365);
    let currency = match q.currency.filter(|c| c.len() == 3) {
        Some(c) => c.to_ascii_uppercase(),
        None => sqlx::query_scalar::<_, String>(
            "SELECT currency::text FROM product_prices WHERE product_id = $1 ORDER BY currency LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&s.pool)
        .await
        .map_err(pelty_database::db_err)?
        .unwrap_or_else(|| "EUR".into()),
    };
    let series = ct::listing_stats(&s.pool, id, &currency, days).await?;
    let views: i64 = series.iter().map(|d| d.views).sum();
    let sales: i64 = series.iter().map(|d| d.sales).sum();
    let revenue: Decimal = series.iter().map(|d| d.revenue).sum();
    let earnings: Decimal = series.iter().map(|d| d.earnings).sum();
    let wishlists = ct::wish_counts(&s.pool, &[id])
        .await?
        .first()
        .map_or(0, |x| x.1);
    Ok(Json(json!({
        "product_id": id,
        "name": l.name,
        "currency": currency,
        "days": days,
        "series": series,
        "totals": {
            "views": views,
            "sales": sales,
            "revenue": revenue,
            "earnings": earnings,
            "conversion": ct::conversion(views, sales),
            "wishlists": wishlists,
        },
    })))
}

// ------------------------------------------------------------------ wishlist

#[utoipa::path(get, path = "/api/v1/client/wishlist", responses((status = 200)))]
pub async fn wishlist(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    Ok(Json(json!({ "items": ct::wishlist(&s.pool, cid).await? })))
}

#[utoipa::path(put, path = "/api/v1/client/wishlist/{product_id}", responses((status = 200), (status = 404), (status = 422)))]
pub async fn wish(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(product): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    ct::wish(&s.pool, cid, product).await?;
    Ok(Json(json!({ "saved": true })))
}

#[utoipa::path(delete, path = "/api/v1/client/wishlist/{product_id}", responses((status = 200)))]
pub async fn unwish(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(product): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    ct::unwish(&s.pool, cid, product).await?;
    Ok(Json(json!({ "saved": false })))
}

// ------------------------------------------------------------- conversations

/// The caller's buyer id and creator id (either may be missing).
async fn sides(s: &AppState, u: &CurrentUser) -> ApiResult<(Option<Uuid>, Option<Uuid>)> {
    let cid = customer_of(s, u).await.ok();
    let creator = mk::by_user(&s.pool, u.user_id).await?.map(|c| c.id);
    if cid.is_none() && creator.is_none() {
        return Err(AppError::NotFound.into());
    }
    Ok((cid, creator))
}

/// Load a conversation the caller takes part in; `true` = as the creator.
async fn mine(s: &AppState, u: &CurrentUser, id: Uuid) -> ApiResult<(ct::Conversation, bool)> {
    let (cid, creator) = sides(s, u).await?;
    let c = ct::conversation(&s.pool, id).await?;
    if creator == Some(c.creator_id) {
        Ok((c, true))
    } else if cid == Some(c.customer_id) {
        Ok((c, false))
    } else {
        Err(AppError::NotFound.into())
    }
}

#[utoipa::path(get, path = "/api/v1/client/market-messages", responses((status = 200)))]
pub async fn inbox(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let (cid, creator) = sides(&s, &u).await?;
    Ok(Json(json!({
        "items": ct::conversations(&s.pool, cid, creator).await?,
        "unread": ct::unread(&s.pool, cid, creator).await?,
        "creator_id": creator,
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartBody {
    pub product_id: Uuid,
    pub subject: String,
    pub body: String,
}

/// A customer asks the creator of a listing a question (pre-sale or support).
#[utoipa::path(post, path = "/api/v1/client/market-messages", request_body = StartBody, responses((status = 200), (status = 404), (status = 422), (status = 429)))]
pub async fn start_conversation(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<StartBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let id = ct::start(&s.pool, cid, u.user_id, b.product_id, &b.subject, &b.body).await?;
    notify(&s, id, true, &b.body).await;
    Ok(Json(json!(ct::conversation(&s.pool, id).await?)))
}

#[utoipa::path(get, path = "/api/v1/client/market-messages/{id}", responses((status = 200), (status = 404)))]
pub async fn thread(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let (c, as_creator) = mine(&s, &u, id).await?;
    ct::mark_read(&s.pool, id, as_creator).await?;
    Ok(Json(json!({
        "conversation": c,
        "messages": ct::messages(&s.pool, id).await?,
        "as_creator": as_creator,
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyBody {
    pub body: String,
}

#[utoipa::path(post, path = "/api/v1/client/market-messages/{id}/reply", request_body = ReplyBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn reply(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<ReplyBody>,
) -> ApiResult<Json<Value>> {
    let (_, as_creator) = mine(&s, &u, id).await?;
    let before = ct::reply(&s.pool, id, u.user_id, as_creator, &b.body).await?;
    if before == 0 {
        notify(&s, id, !as_creator, &b.body).await;
    }
    Ok(Json(
        json!({ "messages": ct::messages(&s.pool, id).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusBody {
    pub open: bool,
}

/// Either side may close a conversation or open it again.
#[utoipa::path(post, path = "/api/v1/client/market-messages/{id}/status", request_body = StatusBody, responses((status = 200), (status = 404)))]
pub async fn set_status(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    mine(&s, &u, id).await?;
    ct::set_status(&s.pool, id, b.open).await?;
    Ok(Json(json!(ct::conversation(&s.pool, id).await?)))
}

/// Mail the other side (only when they had nothing unread) and emit
/// `market.message_received` for webhooks and notification rules.
async fn notify(s: &AppState, id: Uuid, to_creator: bool, body: &str) {
    let r: pelty_core::AppResult<()> = async {
        let c = ct::conversation(&s.pool, id).await?;
        s.emit(DomainEvent::new(
            "market.message_received",
            json!({ "conversation_id": id, "creator_id": c.creator_id, "customer_id": c.customer_id, "to": if to_creator { "creator" } else { "buyer" } }),
        ));
        let Some((email, name, locale)) = ct::recipient(&s.pool, id, to_creator).await? else {
            return Ok(());
        };
        let locale = locale
            .filter(|l| s.i18n.has(l))
            .unwrap_or_else(|| s.cfg.default_locale.clone());
        let from = if to_creator { c.buyer.clone() } else { c.creator.clone() };
        let preview: String = body.chars().take(300).collect();
        let link = format!(
            "{}/messages?c={id}",
            s.cfg.public_url.trim_end_matches('/')
        );
        crate::mail::enqueue(
            s,
            "market_message",
            &locale,
            &email,
            crate::mail::vars(&[
                ("name", &name),
                ("from", &from),
                ("subject", &c.subject),
                ("product", c.product.as_deref().unwrap_or("")),
                ("message", &preview),
                ("link", &link),
            ]),
        )
        .await
    }
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "market message notification failed");
    }
}
