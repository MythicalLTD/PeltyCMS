//! Creator marketplace (module `creator_marketplace`): creator applications
//! and profiles, listings with admin approval, the seller dashboard (balances,
//! ledger, per-product sales), payouts, and public creator pages. Sales are
//! accrued on `invoice.paid` and reversed on refunds and chargebacks.
use super::payouts::{provider, Sent};
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::routes::downloads::sniff;
use crate::routes::ticket_attachments::clean_name;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::marketplace::{self as mk, Creator, Profile};
use pelty_database::{audit, catalog, digital as dg};
use pelty_extensions::events::DomainEvent;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/client/creator/listings/{id}/versions",
            post(upload_version),
        )
        .layer(DefaultBodyLimit::max(
            pelty_core::config::upload_limits().download + 1024,
        ))
        .route("/public/creators", get(public_creators))
        .route("/public/creators/{slug}", get(public_creator))
        .route("/public/marketplace", get(public_config))
        .route(
            "/client/creator",
            get(dashboard).post(apply).put(update_profile),
        )
        .route(
            "/client/creator/listings",
            get(my_listings).post(create_listing),
        )
        .route("/client/creator/listings/{id}/listed", post(set_listed))
        .route("/client/creator/ledger", get(my_ledger))
        .route(
            "/client/creator/payouts",
            get(my_payouts).post(request_payout),
        )
        .route("/admin/creators", get(admin_creators))
        .route("/admin/creators/{id}/status", post(admin_creator_status))
        .route("/admin/creator-listings", get(admin_listings))
        .route("/admin/creator-listings/{id}/review", post(admin_review))
        .route("/admin/creator-payouts", get(admin_payouts))
        .route("/admin/creator-payouts/{id}/send", post(admin_send))
        .route(
            "/admin/creator-payouts/{id}/status",
            post(admin_payout_status),
        )
}

async fn me(s: &AppState, u: &CurrentUser) -> ApiResult<Creator> {
    Ok(mk::by_user(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::NotFound)?)
}

async fn approved(s: &AppState, u: &CurrentUser) -> ApiResult<(Creator, mk::Config)> {
    let cfg = mk::config(&s.pool).await?;
    if !cfg.multi_vendor() {
        return Err(AppError::Conflict("error.market_closed").into());
    }
    let c = me(s, u).await?;
    if c.status != "approved" {
        return Err(AppError::Forbidden.into());
    }
    Ok((c, cfg))
}

// ------------------------------------------------------------------ public

/// Marketplace mode for the storefront (`multi_vendor` shows creator pages).
#[utoipa::path(get, path = "/api/v1/public/marketplace", responses((status = 200, body = mk::Config)))]
pub async fn public_config(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let c = mk::config(&s.pool).await?;
    Ok(Json(json!({ "mode": c.mode })))
}

fn public_profile(c: &Creator) -> Value {
    json!({ "id": c.id, "slug": c.slug, "display_name": c.display_name, "bio": c.bio, "website": c.website, "avatar_url": c.avatar_url, "since": c.created_at.date().to_string() })
}

#[utoipa::path(get, path = "/api/v1/public/creators", responses((status = 200)))]
pub async fn public_creators(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = mk::list(&s.pool, None, true)
        .await?
        .iter()
        .map(|r| {
            let mut v = public_profile(&r.creator);
            v["listings"] = json!(r.listings);
            v["sales"] = json!(r.sales);
            v
        })
        .collect();
    Ok(Json(json!({ "items": items })))
}

/// A creator's public page: profile and approved listings.
#[utoipa::path(get, path = "/api/v1/public/creators/{slug}", responses((status = 200), (status = 404)))]
pub async fn public_creator(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let c = mk::by_slug(&s.pool, &slug).await?;
    let listings: Vec<Value> = mk::listings(&s.pool, Some(c.id), Some("approved"))
        .await?
        .into_iter()
        .map(|l| json!({ "product_id": l.product_id, "name": l.name, "slug": l.slug, "description": l.description, "image_url": l.image_url, "sales": l.sales }))
        .collect();
    Ok(Json(
        json!({ "creator": public_profile(&c), "listings": listings }),
    ))
}

// ------------------------------------------------------------------ client

/// Seller dashboard: profile, balances, per-product sales and settings.
#[utoipa::path(get, path = "/api/v1/client/creator", responses((status = 200), (status = 404)))]
pub async fn dashboard(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cfg = mk::config(&s.pool).await?;
    let Some(c) = mk::by_user(&s.pool, u.user_id).await? else {
        return Ok(Json(json!({ "creator": null, "mode": cfg.mode })));
    };
    Ok(Json(json!({
        "creator": c,
        "mode": cfg.mode,
        "commission_percent": c.commission_percent.unwrap_or(cfg.commission_percent).to_string(),
        "hold_days": cfg.hold_days,
        "min_payout": cfg.min_payout.to_string(),
        "balances": mk::balances(&s.pool, c.id).await?,
        "products": mk::product_stats(&s.pool, c.id).await?,
    })))
}

/// Apply to sell (or get approved at once when auto-approve is on).
#[utoipa::path(post, path = "/api/v1/client/creator", request_body = Profile, responses((status = 200, body = Creator), (status = 409), (status = 422)))]
pub async fn apply(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<Profile>,
) -> ApiResult<Json<Creator>> {
    let cfg = mk::config(&s.pool).await?;
    if !cfg.multi_vendor() {
        return Err(AppError::Conflict("error.market_closed").into());
    }
    let cust = customer_of(&s, &u).await?;
    let c = mk::apply(&s.pool, u.user_id, cust, &b, cfg.auto_approve_creators).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.applied",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "slug": c.slug }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "creator.applied",
        json!({ "creator_id": c.id, "status": c.status }),
    ));
    Ok(Json(c))
}

#[utoipa::path(put, path = "/api/v1/client/creator", request_body = Profile, responses((status = 200, body = Creator), (status = 404), (status = 409), (status = 422)))]
pub async fn update_profile(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<Profile>,
) -> ApiResult<Json<Creator>> {
    let c = me(&s, &u).await?;
    Ok(Json(mk::update_profile(&s.pool, c.id, &b).await?))
}

#[utoipa::path(get, path = "/api/v1/client/creator/listings", responses((status = 200, body = super::Items<mk::Listing>)))]
pub async fn my_listings(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    Ok(Json(
        json!({ "items": mk::listings(&s.pool, Some(c.id), None).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewListing {
    pub group_id: Uuid,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image_url: String,
    pub currency: String,
    /// `one_time` or a recurring cycle (subscriptions).
    pub cycle: String,
    #[schema(value_type = String)]
    pub price: Decimal,
}

/// Submit a product for review. It is created hidden (licensed digital
/// product) and goes live when an admin approves it.
#[utoipa::path(post, path = "/api/v1/client/creator/listings", request_body = NewListing, responses((status = 200, body = mk::Listing), (status = 403), (status = 409), (status = 422)))]
pub async fn create_listing(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<NewListing>,
) -> ApiResult<Json<mk::Listing>> {
    let (c, cfg) = approved(&s, &u).await?;
    if b.price <= Decimal::ZERO {
        return Err(AppError::Validation("error.invalid_price").into());
    }
    let groups = catalog::groups(&s.pool).await?;
    if !groups.iter().any(|g| g.id == b.group_id) {
        return Err(AppError::Validation("error.invalid_reference").into());
    }
    let live = cfg.auto_approve_listings;
    let p = catalog::create_product(
        &s.pool,
        &catalog::ProductInput {
            group_id: Some(b.group_id),
            name: Some(b.name.clone()),
            slug: Some(b.slug.clone()),
            description: Some(b.description.clone()),
            // Always hidden until the listing row exists, so a failure below
            // never leaves a buyable product without an owner.
            status: Some("hidden".into()),
            module: None,
            module_config: None,
            sort_order: None,
            image_url: Some(b.image_url.clone()),
        },
    )
    .await?;
    let price = catalog::Price {
        product_id: None,
        currency: b.currency.to_ascii_uppercase(),
        cycle: b.cycle.clone(),
        amount: b.price,
        setup_fee: Decimal::ZERO,
    };
    let setup = async {
        catalog::set_prices(&s.pool, p.id, &[price]).await?;
        dg::save_settings(&s.pool, p.id, &dg::Settings::default()).await?;
        mk::add_listing(&s.pool, p.id, c.id, live).await?;
        if live {
            mk::activate_product(&s.pool, p.id).await?;
        }
        Ok::<_, AppError>(())
    }
    .await;
    if let Err(e) = setup {
        let _ = sqlx::query("DELETE FROM products WHERE id = $1")
            .bind(p.id)
            .execute(&s.pool)
            .await;
        return Err(e.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.listing_submitted",
        Some(&p.id.to_string()),
        u.ip.as_deref(),
        json!({ "creator_id": c.id }),
    )
    .await?;
    Ok(Json(mk::listing(&s.pool, p.id).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListedBody {
    pub listed: bool,
}

#[utoipa::path(post, path = "/api/v1/client/creator/listings/{id}/listed", request_body = ListedBody, responses((status = 200), (status = 409)))]
pub async fn set_listed(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<ListedBody>,
) -> ApiResult<Json<Value>> {
    let (c, _) = approved(&s, &u).await?;
    mk::set_listed(&s.pool, id, c.id, b.listed).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct VersionQuery {
    pub version: String,
    pub filename: String,
    #[serde(default)]
    pub changelog: String,
}

/// Creators publish versions of their own listings (same rules as admin uploads).
#[utoipa::path(post, path = "/api/v1/client/creator/listings/{id}/versions", params(VersionQuery), request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses((status = 200, body = dg::Version), (status = 404), (status = 409), (status = 422)))]
pub async fn upload_version(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<VersionQuery>,
    body: Bytes,
) -> ApiResult<Json<dg::Version>> {
    let (c, _) = approved(&s, &u).await?;
    if mk::owner_of(&s.pool, id).await? != Some(c.id) {
        return Err(AppError::NotFound.into());
    }
    let ct = sniff(&body).map_err(|e| {
        if e == "error.download.too_large" {
            AppError::BadRequest(e)
        } else {
            AppError::Validation(e)
        }
    })?;
    let name = clean_name(&q.filename);
    let v = dg::add_version(
        &s.pool,
        dg::NewVersion {
            product_id: id,
            version: q.version.trim(),
            changelog: &q.changelog,
            filename: &name,
            content_type: ct,
            data: &body,
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "digital_version.created",
        Some(&v.id.to_string()),
        u.ip.as_deref(),
        json!({ "product_id": id, "by_creator": c.id }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "digital.version_released",
        json!({ "product_id": id, "version_id": v.id, "version": v.version }),
    ));
    Ok(Json(v))
}

#[utoipa::path(get, path = "/api/v1/client/creator/ledger", responses((status = 200, body = super::Items<mk::Entry>)))]
pub async fn my_ledger(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    Ok(Json(
        json!({ "items": mk::ledger(&s.pool, c.id, 500).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/creator/payouts", responses((status = 200, body = super::Items<mk::Payout>)))]
pub async fn my_payouts(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = me(&s, &u).await?;
    Ok(Json(
        json!({ "items": mk::payouts(&s.pool, Some(c.id), None).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(as = CreatorPayoutBody)]
pub struct PayoutBody {
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
}

/// Request a payout of available (held-period-passed) earnings.
#[utoipa::path(post, path = "/api/v1/client/creator/payouts", request_body = PayoutBody, responses((status = 200, body = mk::Payout), (status = 409), (status = 422)))]
pub async fn request_payout(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<PayoutBody>,
) -> ApiResult<Json<mk::Payout>> {
    let c = me(&s, &u).await?;
    if c.status != "approved" {
        return Err(AppError::Forbidden.into());
    }
    let cfg = mk::config(&s.pool).await?;
    let p = mk::request_payout(&s.pool, &c, &b.currency, b.amount, &cfg).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_requested",
        Some(&p.id.to_string()),
        u.ip.as_deref(),
        json!({ "amount": p.amount.to_string(), "currency": p.currency }),
    )
    .await?;
    s.emit(DomainEvent::new("creator.payout_requested", json!({ "payout_id": p.id, "creator_id": c.id, "amount": p.amount.to_string(), "currency": p.currency })));
    Ok(Json(p))
}

// ------------------------------------------------------------------ admin

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct StatusFilter {
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/creators", params(StatusFilter), responses((status = 200, body = super::Items<mk::CreatorRow>)))]
pub async fn admin_creators(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Query(f): Query<StatusFilter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": mk::list(&s.pool, f.status.as_deref(), false).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatorStatusBody {
    pub status: String,
    /// Commission override in percent.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub commission_percent: Option<Decimal>,
    /// Drop the override and use the marketplace default.
    #[serde(default)]
    pub default_commission: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/creators/{id}/status", request_body = CreatorStatusBody, responses((status = 200, body = Creator), (status = 404), (status = 422)))]
pub async fn admin_creator_status(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CreatorStatusBody>,
) -> ApiResult<Json<Creator>> {
    let c = mk::set_status(
        &s.pool,
        id,
        &b.status,
        if b.default_commission {
            Some(None)
        } else {
            b.commission_percent.map(Some)
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.status",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    Ok(Json(c))
}

#[utoipa::path(get, path = "/api/v1/admin/creator-listings", params(StatusFilter), responses((status = 200, body = super::Items<mk::Listing>)))]
pub async fn admin_listings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Query(f): Query<StatusFilter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": mk::listings(&s.pool, None, f.status.as_deref()).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(as = ListingReviewBody)]
pub struct ReviewBody {
    pub approve: bool,
    #[serde(default)]
    pub reason: String,
}

/// Approve (publish) or reject a creator listing.
#[utoipa::path(post, path = "/api/v1/admin/creator-listings/{id}/review", request_body = ReviewBody, responses((status = 200), (status = 404)))]
pub async fn admin_review(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ReviewBody>,
) -> ApiResult<Json<Value>> {
    mk::review(&s.pool, id, b.approve, b.reason.trim(), u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.listing_reviewed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "approve": b.approve }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/creator-payouts", params(StatusFilter), responses((status = 200, body = super::Items<mk::Payout>)))]
pub async fn admin_payouts(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AffiliatesManage>,
    Query(f): Query<StatusFilter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": mk::payouts(&s.pool, None, f.status.as_deref()).await? }),
    ))
}

/// Send a requested payout through the creator's payout method.
#[utoipa::path(post, path = "/api/v1/admin/creator-payouts/{id}/send", responses((status = 200, body = mk::Payout), (status = 409)))]
pub async fn admin_send(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<mk::Payout>> {
    let p = mk::payout(&s.pool, id).await?;
    if p.status != "requested" {
        return Err(AppError::Conflict("error.payout_state").into());
    }
    let c = mk::get(&s.pool, p.creator_id).await?;
    let prov =
        provider(&p.method).ok_or(AppError::Conflict("error.payout_provider_unconfigured"))?;
    // Claim first (requested → processing under a row lock) so two sends of
    // the same payout cannot both reach the provider. Providers get the
    // payout id as their idempotency key.
    mk::set_payout(&s.pool, id, "processing", "", "").await?;
    let out = match prov.send(&c, &p).await {
        Ok(Sent::Paid(r)) => mk::set_payout(&s.pool, id, "paid", &r, "").await?,
        Ok(Sent::Processing(r)) => mk::payout_ref(&s.pool, id, &r).await?,
        Err(code) => {
            mk::unclaim_payout(&s.pool, id).await?;
            return Err(AppError::Conflict(code).into());
        }
    };
    if out.status == "paid" {
        s.emit(DomainEvent::new(
            "creator.payout_paid",
            json!({ "payout_id": out.id, "creator_id": out.creator_id, "amount": out.amount.to_string(), "currency": out.currency }),
        ));
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_sent",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "method": p.method, "status": out.status }),
    )
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PayoutStatusBody {
    /// `paid`, `rejected` or `failed` (the last two credit the amount back).
    pub status: String,
    #[serde(default)]
    pub provider_ref: String,
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/admin/creator-payouts/{id}/status", request_body = PayoutStatusBody, responses((status = 200, body = mk::Payout), (status = 409), (status = 422)))]
pub async fn admin_payout_status(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<PayoutStatusBody>,
) -> ApiResult<Json<mk::Payout>> {
    if b.status == "processing" {
        return Err(AppError::Validation("error.validation").into());
    }
    let p = mk::set_payout(&s.pool, id, &b.status, b.provider_ref.trim(), b.note.trim()).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_status",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    if p.status == "paid" {
        s.emit(DomainEvent::new("creator.payout_paid", json!({ "payout_id": p.id, "creator_id": p.creator_id, "amount": p.amount.to_string(), "currency": p.currency })));
    }
    Ok(Json(p))
}

// ------------------------------------------------------------------ hooks

/// Event bus hook: accrue sales on payment, reverse them on refund/chargeback.
pub async fn on_event(s: &AppState, e: &DomainEvent) {
    if !matches!(
        e.name,
        "invoice.paid" | "order.refunded" | "invoice.refunded" | "payment.disputed"
    ) {
        return;
    }
    if !matches!(
        pelty_database::feature_modules::is_enabled(&s.pool, mk::MODULE).await,
        Ok(true)
    ) {
        return;
    }
    let id = |k: &str| e.payload[k].as_str().and_then(|v| v.parse::<Uuid>().ok());
    let r: pelty_core::AppResult<()> = async {
        match e.name {
            "invoice.paid" => {
                if let Some(inv) = id("invoice_id") {
                    let cfg = mk::config(&s.pool).await?;
                    mk::accrue_invoice(&s.pool, inv, &cfg).await?;
                }
            }
            _ => {
                let (kind, key) = match e.name {
                    "order.refunded" => ("order", "order_id"),
                    "invoice.refunded" => ("invoice", "invoice_id"),
                    _ => ("payment", "payment_id"),
                };
                if let Some(x) = id(key) {
                    let invoices = mk::invoices_for(&s.pool, kind, x).await?;
                    mk::reverse_invoices(&s.pool, &invoices, e.name).await?;
                }
            }
        }
        Ok(())
    }
    .await;
    if let Err(err) = r {
        tracing::warn!(event = e.name, error = %err, "creator marketplace hook failed");
    }
}
