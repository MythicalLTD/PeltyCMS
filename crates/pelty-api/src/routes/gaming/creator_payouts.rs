//! Live creator payouts (module `creator_marketplace`).
//!
//! * Stripe Connect: creators onboard an Express account through a hosted
//!   account link; payouts are **separate transfers** from the platform
//!   balance (buyers pay the platform through the normal gateways, the sale
//!   is accrued to the creator ledger minus commission and held for
//!   `hold_days`, then paid out). Refunds or chargebacks that leave a
//!   creator's balance negative reverse the newest paid transfers.
//! * PayPal Payouts: creators save a PayPal e-mail; payouts go out in
//!   batches, item status comes from webhooks and an hourly poll, and failed
//!   or returned items credit the ledger again.
//! * Scheduling: `payout_schedule` (manual / weekly / monthly) creates
//!   payouts of available balances; with `payout_approval` off they are sent
//!   at once, otherwise staff send them from the payouts page.
use super::payouts::{self as gw, Outcome, PayPal, PayPalItem, ProvErr, Stripe};
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::creator_payouts as cp;
use pelty_database::marketplace::{self as mk, Creator, Payout};
use pelty_database::{audit, feature_modules};
use pelty_extensions::events::DomainEvent;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// Hourly sync job (PayPal status, retries, unapproved sends).
pub const SYNC_JOB: &str = "creator_payouts.sync";
/// Scheduled payout creation (once per due day).
pub const AUTO_JOB: &str = "creator_payouts.auto";
/// Settings field names (kept as constants so they never look like secrets).
const STRIPE_FIELD: &str = "stripe";
const PAYPAL_FIELD: &str = "paypal";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/creator/payout-settings", put(save_settings))
        .route("/client/creator/stripe/onboard", post(stripe_onboard))
        .route("/client/creator/stripe/sync", post(stripe_sync))
        .route("/client/creator/stripe/dashboard", post(stripe_dashboard))
        .route(
            "/client/creator/listings/{id}/gallery",
            get(my_gallery).put(save_gallery),
        )
        .route("/admin/creators/{id}/payout", put(admin_creator_payout))
        .route("/admin/creator-payouts/overview", get(admin_overview))
        .route("/admin/creator-payouts/run", post(admin_run))
        .route("/admin/creator-payouts/send-batch", post(admin_send_batch))
        .route("/webhooks/creator-payouts/stripe", post(stripe_webhook))
        .route("/webhooks/creator-payouts/paypal", post(paypal_webhook))
}

fn stripe() -> Result<Stripe, ProvErr> {
    Stripe::from_env()?.ok_or(ProvErr::Unconfigured)
}

fn paypal() -> Result<PayPal, ProvErr> {
    PayPal::from_env()?.ok_or(ProvErr::Unconfigured)
}

fn prov(e: ProvErr) -> AppError {
    AppError::Conflict(e.code())
}

async fn me(s: &AppState, u: &CurrentUser) -> ApiResult<(Creator, mk::Config)> {
    let c = mk::by_user(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if c.status != "approved" {
        return Err(AppError::Forbidden.into());
    }
    Ok((c, mk::config(&s.pool).await?))
}

/// What the creator dashboard shows about payouts.
pub async fn dashboard_extra(
    s: &AppState,
    c: &Creator,
    cfg: &mk::Config,
) -> pelty_core::AppResult<Value> {
    let st = Stripe::from_env().ok().flatten();
    let pp = PayPal::from_env().ok().flatten();
    let transit: Vec<Value> = cp::in_transit(&s.pool, c.id)
        .await?
        .into_iter()
        .map(|(cur, a)| json!({ "currency": cur, "amount": a.to_string() }))
        .collect();
    Ok(json!({
        "method": c.payout_method,
        "locked": c.payout_method_locked,
        "methods": cfg.payout_methods,
        "paypal_email": c.paypal_email,
        (STRIPE_FIELD): {
            "available": st.is_some(),
            "test_mode": st.as_ref().is_some_and(Stripe::test_mode),
            "connected": !c.stripe_account_id.is_empty(),
            "status": c.stripe_status,
            "detail": c.stripe_status_detail,
        },
        (PAYPAL_FIELD): { "available": pp.is_some(), "test_mode": pp.as_ref().is_some_and(PayPal::test_mode) },
        "schedule": cfg.payout_schedule,
        "weekday": cfg.payout_weekday,
        "monthday": cfg.payout_monthday,
        "approval": cfg.payout_approval,
        "in_transit": transit,
    }))
}

// ------------------------------------------------------------------ creator

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PayoutSettingsBody {
    /// `manual`, `stripe_connect` or `paypal` (one the marketplace allows).
    pub method: String,
    #[serde(default)]
    pub paypal_email: String,
}

/// Creator payout method and PayPal e-mail.
#[utoipa::path(put, path = "/api/v1/client/creator/payout-settings", request_body = PayoutSettingsBody,
    responses((status = 200, body = Creator), (status = 403), (status = 409), (status = 422)))]
pub async fn save_settings(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<PayoutSettingsBody>,
) -> ApiResult<Json<Creator>> {
    let (c, cfg) = me(&s, &u).await?;
    let out =
        cp::save_settings(&s.pool, &c, &b.method, &b.paypal_email, &cfg.payout_methods).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_settings",
        Some(&c.id.to_string()),
        u.ip.as_deref(),
        json!({ "method": out.payout_method, "paypal_email_set": !out.paypal_email.is_empty() }),
    )
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LinkOut {
    pub url: String,
}

/// Start (or resume) Stripe Connect onboarding: creates the Express account
/// once, then returns a fresh hosted onboarding link.
#[utoipa::path(post, path = "/api/v1/client/creator/stripe/onboard", responses((status = 200, body = LinkOut), (status = 403), (status = 409)))]
pub async fn stripe_onboard(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<LinkOut>> {
    let (c, cfg) = me(&s, &u).await?;
    if c.payout_method != "stripe_connect"
        && !cfg.payout_methods.iter().any(|m| m == "stripe_connect")
    {
        return Err(AppError::Validation("error.payout_method_unavailable").into());
    }
    let st = stripe().map_err(prov)?;
    let t = s.http();
    let mut account = c.stripe_account_id.clone();
    if account.is_empty() || c.stripe_status == "disconnected" {
        let (email, country): (String, String) = sqlx::query_as(
            "SELECT COALESCE(NULLIF(cu.email, ''), us.email), COALESCE(cu.country, '') FROM creators c
               JOIN customers cu ON cu.id = c.customer_id JOIN users us ON us.id = c.user_id WHERE c.id = $1",
        )
        .bind(c.id)
        .fetch_one(&s.pool)
        .await
        .map_err(pelty_database::db_err)?;
        account = st
            .create_account(
                &*t,
                c.id,
                &email,
                Some(country.as_str()).filter(|x| !x.is_empty()),
            )
            .await
            .map_err(prov)?;
        cp::set_stripe_account(&s.pool, c.id, &account).await?;
        audit::record(
            &s.pool,
            Some(u.user_id),
            "creator.stripe_account",
            Some(&c.id.to_string()),
            u.ip.as_deref(),
            json!({ "account": account }),
        )
        .await?;
    }
    let back = format!(
        "{}/creator?tab=payouts",
        s.cfg.public_url.trim_end_matches('/')
    );
    let url = st
        .account_link(
            &*t,
            &account,
            &format!("{back}&stripe=refresh"),
            &format!("{back}&stripe=return"),
        )
        .await
        .map_err(prov)?;
    Ok(Json(LinkOut { url }))
}

/// Re-read the connected account from Stripe (after returning from onboarding).
#[utoipa::path(post, path = "/api/v1/client/creator/stripe/sync", responses((status = 200), (status = 404), (status = 409)))]
pub async fn stripe_sync(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let (c, _) = me(&s, &u).await?;
    if c.stripe_account_id.is_empty() {
        return Err(AppError::NotFound.into());
    }
    let st = stripe().map_err(prov)?;
    let a = st
        .account(&*s.http(), &c.stripe_account_id)
        .await
        .map_err(prov)?;
    let (status, detail) = gw::account_status(&a);
    cp::set_stripe_status(&s.pool, &c.stripe_account_id, status, &detail).await?;
    Ok(Json(json!({ "status": status, "detail": detail })))
}

/// One-time login link to the creator's Stripe Express dashboard.
#[utoipa::path(post, path = "/api/v1/client/creator/stripe/dashboard", responses((status = 200, body = LinkOut), (status = 404), (status = 409)))]
pub async fn stripe_dashboard(
    State(s): State<AppState>,
    u: CurrentUser,
) -> ApiResult<Json<LinkOut>> {
    let (c, _) = me(&s, &u).await?;
    if c.stripe_account_id.is_empty() || c.stripe_status == "disconnected" {
        return Err(AppError::NotFound.into());
    }
    let url = stripe()
        .map_err(prov)?
        .login_link(&*s.http(), &c.stripe_account_id)
        .await
        .map_err(prov)?;
    Ok(Json(LinkOut { url }))
}

/// Gallery of one of the creator's listings (any review status).
#[utoipa::path(get, path = "/api/v1/client/creator/listings/{id}/gallery", responses((status = 200, body = super::Items<cp::GalleryImage>), (status = 404)))]
pub async fn my_gallery(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let (c, _) = me(&s, &u).await?;
    if mk::owner_of(&s.pool, id).await? != Some(c.id) {
        return Err(AppError::NotFound.into());
    }
    Ok(Json(json!({ "items": cp::gallery(&s.pool, id).await? })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GalleryBody {
    pub images: Vec<cp::GalleryImage>,
}

/// Replace the screenshot gallery of one of the creator's listings.
#[utoipa::path(put, path = "/api/v1/client/creator/listings/{id}/gallery", request_body = GalleryBody,
    responses((status = 200, body = super::Items<cp::GalleryImage>), (status = 404), (status = 422)))]
pub async fn save_gallery(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<GalleryBody>,
) -> ApiResult<Json<Value>> {
    let (c, _) = me(&s, &u).await?;
    if mk::owner_of(&s.pool, id).await? != Some(c.id) {
        return Err(AppError::NotFound.into());
    }
    let items = cp::set_gallery(&s.pool, id, &b.images).await?;
    Ok(Json(json!({ "items": items })))
}

// ------------------------------------------------------------------ admin

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatorPayoutBody {
    pub method: String,
    /// The creator cannot change the method while locked.
    #[serde(default)]
    pub locked: bool,
}

/// Staff set a creator's payout method.
#[utoipa::path(put, path = "/api/v1/admin/creators/{id}/payout", request_body = CreatorPayoutBody,
    responses((status = 200, body = Creator), (status = 404), (status = 422)))]
pub async fn admin_creator_payout(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CreatorPayoutBody>,
) -> ApiResult<Json<Creator>> {
    let c = cp::admin_set_method(&s.pool, id, &b.method, b.locked).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_method",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "method": b.method, "locked": b.locked }),
    )
    .await?;
    Ok(Json(c))
}

/// Provider readiness (never the secrets), webhook URLs and payout totals.
#[utoipa::path(get, path = "/api/v1/admin/creator-payouts/overview", responses((status = 200), (status = 403)))]
pub async fn admin_overview(
    State(s): State<AppState>,
    _p: RequirePermission<perms::AffiliatesManage>,
) -> ApiResult<Json<Value>> {
    let cfg = mk::config(&s.pool).await?;
    let base = format!(
        "{}/api/v1/webhooks/creator-payouts",
        s.cfg.public_url.trim_end_matches('/')
    );
    let st = Stripe::from_env();
    let pp = PayPal::from_env();
    let totals: Vec<(String, String, i64, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT status, currency::text, count(*), sum(amount) FROM creator_payouts GROUP BY status, currency ORDER BY status, currency",
    )
    .fetch_all(&s.pool)
    .await
    .map_err(pelty_database::db_err)?;
    let provider = |configured: bool,
                    refused: bool,
                    test: bool,
                    webhook: bool,
                    url: String,
                    keys: &[&str]| {
        json!({ "configured": configured, "live_refused": refused, "test_mode": test, "webhook_ready": webhook, "webhook_url": url, "env": keys })
    };
    Ok(Json(json!({
        (STRIPE_FIELD): provider(
            matches!(st, Ok(Some(_))), st.is_err(),
            matches!(&st, Ok(Some(x)) if x.test_mode()), matches!(&st, Ok(Some(x)) if x.webhook_ready()),
            format!("{base}/stripe"), &[gw::env::STRIPE_KEY, gw::env::STRIPE_WEBHOOK],
        ),
        (PAYPAL_FIELD): provider(
            matches!(pp, Ok(Some(_))), pp.is_err(),
            matches!(&pp, Ok(Some(x)) if x.test_mode()), matches!(&pp, Ok(Some(x)) if x.webhook_ready()),
            format!("{base}/paypal"), &[gw::env::PAYPAL_ID, gw::env::PAYPAL_SECRET, gw::env::PAYPAL_MODE, gw::env::PAYPAL_WEBHOOK],
        ),
        "live_env": gw::env::LIVE,
        "config": cfg,
        "totals": totals.into_iter().map(|(st, cur, n, sum)| json!({ "status": st, "currency": cur, "count": n, "amount": sum.to_string() })).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RunBody {
    /// Create payouts for every available balance now (ignores the schedule).
    #[serde(default)]
    pub create: bool,
    /// Also send every requested Stripe/PayPal payout now (approval given).
    #[serde(default)]
    pub send: bool,
}

/// Run payouts now: optionally create them from balances, send approved
/// ones, and sync PayPal batch status.
#[utoipa::path(post, path = "/api/v1/admin/creator-payouts/run", request_body = RunBody, responses((status = 200), (status = 403)))]
pub async fn admin_run(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Json(b): Json<RunBody>,
) -> ApiResult<Json<Value>> {
    let cfg = mk::config(&s.pool).await?;
    let created = if b.create {
        create_due(&s, &cfg).await?
    } else {
        0
    };
    let sent = if b.send {
        let ids = requested_ids(&s).await?;
        send_many(&s, &ids).await?
    } else {
        Summary::default()
    };
    let synced = sync_paypal(&s).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_run",
        None,
        u.ip.as_deref(),
        json!({ "created": created, "sent": sent.sent.len() }),
    )
    .await?;
    Ok(Json(
        json!({ "created": created, "send": sent, "synced": synced }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BatchBody {
    pub ids: Vec<Uuid>,
}

/// Approve and send several requested payouts (PayPal ones in one batch).
#[utoipa::path(post, path = "/api/v1/admin/creator-payouts/send-batch", request_body = BatchBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn admin_send_batch(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::AffiliatesManage>,
    Json(b): Json<BatchBody>,
) -> ApiResult<Json<Summary>> {
    if b.ids.is_empty() || b.ids.len() > 500 {
        return Err(AppError::Validation("error.validation").into());
    }
    let out = send_many(&s, &b.ids).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "creator.payout_batch",
        None,
        u.ip.as_deref(),
        json!({ "ids": b.ids.len(), "sent": out.sent.len() }),
    )
    .await?;
    Ok(Json(out))
}

// ------------------------------------------------------------------ sending

/// Result of a send: payouts handed to a provider and ones left alone.
#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct Summary {
    pub sent: Vec<Uuid>,
    /// `(payout, error code)` for payouts not sent.
    pub skipped: Vec<(Uuid, String)>,
}

async fn requested_ids(s: &AppState) -> pelty_core::AppResult<Vec<Uuid>> {
    let mut ids = cp::open_payouts(&s.pool, "stripe_connect", "requested", 500).await?;
    ids.extend(cp::open_payouts(&s.pool, "paypal", "requested", 500).await?);
    Ok(ids)
}

/// Send one requested payout through its method (staff "Send").
pub async fn send_one(s: &AppState, id: Uuid) -> ApiResult<Payout> {
    let p = mk::payout(&s.pool, id).await?;
    if p.status != "requested" {
        return Err(AppError::Conflict("error.payout_state").into());
    }
    let out = send_many(s, &[id]).await?;
    if let Some((_, code)) = out.skipped.first() {
        let code: &'static str = match code.as_str() {
            "error.payout_provider_unconfigured" => "error.payout_provider_unconfigured",
            "error.payout_provider_unavailable" => "error.payout_provider_unavailable",
            "error.payout_provider_rejected" => "error.payout_provider_rejected",
            "error.stripe_not_ready" => "error.stripe_not_ready",
            "error.paypal_email_required" => "error.paypal_email_required",
            "error.invalid_amount" => "error.invalid_amount",
            _ => "error.payout_state",
        };
        return Err(AppError::Conflict(code).into());
    }
    Ok(mk::payout(&s.pool, id).await?)
}

/// Send requested payouts: manual ones become `processing` (staff pay by
/// hand), Stripe ones are transferred one by one, PayPal ones go out in one
/// batch. Never raises for provider errors; they land in `skipped`.
pub async fn send_many(s: &AppState, ids: &[Uuid]) -> pelty_core::AppResult<Summary> {
    let mut out = Summary::default();
    let mut paypal_items: Vec<(Payout, String)> = vec![];
    for id in ids {
        let p = match mk::payout(&s.pool, *id).await {
            Ok(p) if p.status == "requested" => p,
            Ok(_) => {
                out.skipped.push((*id, "error.payout_state".into()));
                continue;
            }
            Err(AppError::NotFound) => {
                out.skipped.push((*id, "error.not_found".into()));
                continue;
            }
            Err(e) => return Err(e),
        };
        let c = mk::get(&s.pool, p.creator_id).await?;
        match p.method.as_str() {
            "manual" => {
                if !cp::claim(&s.pool, &[p.id]).await?.is_empty() {
                    out.sent.push(p.id);
                }
            }
            "stripe_connect" => match send_stripe(s, &p, &c).await {
                Ok(()) => out.sent.push(p.id),
                Err(code) => out.skipped.push((p.id, code.into())),
            },
            "paypal" => {
                if c.paypal_email.is_empty() {
                    out.skipped
                        .push((p.id, "error.paypal_email_required".into()));
                } else {
                    paypal_items.push((p, c.paypal_email.clone()));
                }
            }
            _ => out
                .skipped
                .push((p.id, "error.payout_provider_unconfigured".into())),
        }
    }
    if !paypal_items.is_empty() {
        send_paypal(s, paypal_items, &mut out).await?;
    }
    Ok(out)
}

async fn send_stripe(s: &AppState, p: &Payout, c: &Creator) -> Result<(), &'static str> {
    let st = stripe().map_err(|e| e.code())?;
    if c.stripe_account_id.is_empty() || c.stripe_status != "enabled" {
        return Err("error.stripe_not_ready");
    }
    let exp = cp::exponent(&s.pool, &p.currency)
        .await
        .map_err(|_| "error.internal")?;
    let minor = gw::to_minor(p.amount, exp)
        .filter(|m| *m > 0)
        .ok_or("error.invalid_amount")?;
    // Claim first so two sends cannot both reach Stripe; the payout id is
    // also the idempotency key.
    if cp::claim(&s.pool, &[p.id])
        .await
        .map_err(|_| "error.internal")?
        .is_empty()
    {
        return Err("error.payout_state");
    }
    match st
        .transfer(&*s.http(), &c.stripe_account_id, minor, &p.currency, p.id)
        .await
    {
        Ok(tr) => {
            if let Ok(Some(paid)) = cp::provider_paid(&s.pool, p.id, &tr).await {
                paid_event(s, &paid).await;
            }
            Ok(())
        }
        Err(ProvErr::Rejected(m)) => {
            // Stripe refused (e.g. insufficient platform balance): the payout
            // fails and the amount returns to the creator's balance.
            if let Ok(Some(f)) = cp::provider_failed(&s.pool, p.id, &format!("stripe: {m}")).await {
                failed_event(s, &f).await;
            }
            Err("error.payout_provider_rejected")
        }
        Err(e) => {
            // Unknown outcome: back to requested; the same idempotency key
            // makes the retry safe (a `transfer.created` webhook may also settle it).
            let _ = mk::unclaim_payout(&s.pool, p.id).await;
            let _ = cp::note(&s.pool, p.id, &format!("stripe: {}", e.message())).await;
            Err(e.code())
        }
    }
}

async fn paypal_items(
    s: &AppState,
    list: &[(Payout, String)],
) -> pelty_core::AppResult<Vec<PayPalItem>> {
    let mut items = vec![];
    for (p, email) in list {
        let exp = cp::exponent(&s.pool, &p.currency).await?;
        items.push(PayPalItem {
            payout: p.id,
            email: email.clone(),
            value: gw::decimal_string(p.amount, exp),
            currency: p.currency.clone(),
        });
    }
    Ok(items)
}

async fn send_paypal(
    s: &AppState,
    list: Vec<(Payout, String)>,
    out: &mut Summary,
) -> pelty_core::AppResult<()> {
    let pp = match paypal() {
        Ok(pp) => pp,
        Err(e) => {
            out.skipped
                .extend(list.iter().map(|(p, _)| (p.id, e.code().to_string())));
            return Ok(());
        }
    };
    let ids: Vec<Uuid> = list.iter().map(|(p, _)| p.id).collect();
    let claimed = cp::claim(&s.pool, &ids).await?;
    let list: Vec<(Payout, String)> = list
        .into_iter()
        .filter(|(p, _)| claimed.contains(&p.id))
        .collect();
    out.skipped.extend(
        ids.iter()
            .filter(|i| !claimed.contains(i))
            .map(|i| (*i, "error.payout_state".to_string())),
    );
    if list.is_empty() {
        return Ok(());
    }
    let sender = Uuid::new_v4().simple().to_string();
    cp::set_batch(&s.pool, &claimed, &format!("pending:{sender}")).await?;
    let items = paypal_items(s, &list).await?;
    let t = s.http();
    let r = match pp.token(&*t).await {
        Ok(tok) => {
            pp.create_batch(&*t, &tok, &sender, "You have a payout", &items)
                .await
        }
        Err(e) => Err(e),
    };
    match r {
        Ok(batch) => {
            cp::set_batch(&s.pool, &claimed, &batch).await?;
            out.sent.extend(claimed);
        }
        Err(ProvErr::Retry(m)) => {
            // Outcome unknown: keep `pending:<sender>`; the sync job re-posts
            // with the same PayPal-Request-Id, which cannot pay twice.
            for id in &claimed {
                cp::note(&s.pool, *id, &format!("paypal: {m}")).await?;
            }
            out.sent.extend(claimed);
        }
        Err(e) => {
            cp::unclaim(&s.pool, &claimed).await?;
            for id in &claimed {
                cp::note(&s.pool, *id, &format!("paypal: {}", e.message())).await?;
            }
            out.skipped
                .extend(claimed.iter().map(|i| (*i, e.code().to_string())));
        }
    }
    Ok(())
}

/// Re-post batches whose answer was lost, then poll open batches. Returns
/// the number of payouts settled (paid or failed).
pub async fn sync_paypal(s: &AppState) -> pelty_core::AppResult<usize> {
    let batches = cp::open_batches(&s.pool).await?;
    if batches.is_empty() {
        return Ok(0);
    }
    let Ok(pp) = paypal() else { return Ok(0) };
    let t = s.http();
    let Ok(tok) = pp.token(&*t).await else {
        return Ok(0);
    };
    let mut settled = 0;
    for b in batches {
        if let Some(sender) = b.strip_prefix("pending:") {
            let rows = cp::by_batch(&s.pool, &b).await?;
            let mut list = vec![];
            for p in rows {
                let c = mk::get(&s.pool, p.creator_id).await?;
                list.push((p, c.paypal_email));
            }
            let items = paypal_items(s, &list).await?;
            let ids: Vec<Uuid> = list.iter().map(|(p, _)| p.id).collect();
            match pp
                .create_batch(&*t, &tok, sender, "You have a payout", &items)
                .await
            {
                Ok(batch) => cp::set_batch(&s.pool, &ids, &batch).await?,
                Err(ProvErr::Rejected(m)) => {
                    cp::unclaim(&s.pool, &ids).await?;
                    for id in &ids {
                        cp::note(&s.pool, *id, &format!("paypal: {m}")).await?;
                    }
                }
                Err(_) => {}
            }
            continue;
        }
        let Ok(items) = pp.batch(&*t, &tok, &b).await else {
            continue;
        };
        for i in items {
            if apply_item(s, &i).await? {
                settled += 1;
            }
        }
    }
    Ok(settled)
}

/// Apply one PayPal item status; `true` when the payout settled.
async fn apply_item(s: &AppState, i: &gw::ItemStatus) -> pelty_core::AppResult<bool> {
    match gw::paypal_outcome(&i.status) {
        Outcome::Paid => {
            let r = if i.item_id.is_empty() {
                i.status.clone()
            } else {
                i.item_id.clone()
            };
            if let Some(p) = cp::provider_paid(&s.pool, i.payout, &r).await? {
                paid_event(s, &p).await;
                return Ok(true);
            }
        }
        Outcome::Failed => {
            let note = format!("paypal: {} {}", i.status, i.error)
                .trim()
                .to_string();
            if let Some(p) = cp::provider_failed(&s.pool, i.payout, &note).await? {
                failed_event(s, &p).await;
                return Ok(true);
            }
        }
        Outcome::Pending => {}
    }
    Ok(false)
}

async fn mail_creator(s: &AppState, creator: Uuid, template: &str, vars: &[(&str, &str)]) {
    let Ok(Some((to, name, locale))) = cp::contact(&s.pool, creator).await else {
        return;
    };
    let locale = locale
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    let link = format!(
        "{}/creator?tab=payouts",
        s.cfg.public_url.trim_end_matches('/')
    );
    let mut v: Vec<(&str, &str)> = vec![("name", &name), ("link", &link)];
    v.extend_from_slice(vars);
    if let Err(e) = crate::mail::enqueue(s, template, &locale, &to, crate::mail::vars(&v)).await {
        tracing::warn!(error = %e, template, "creator payout mail failed");
    }
}

async fn paid_event(s: &AppState, p: &Payout) {
    s.emit(DomainEvent::new(
        "creator.payout_paid",
        json!({ "payout_id": p.id, "creator_id": p.creator_id, "amount": p.amount.to_string(), "currency": p.currency }),
    ));
    let amount = format!("{} {}", p.amount, p.currency);
    mail_creator(
        s,
        p.creator_id,
        "creator_payout_paid",
        &[("amount", &amount), ("method", &p.method)],
    )
    .await;
}

async fn failed_event(s: &AppState, p: &Payout) {
    s.emit(DomainEvent::new(
        "creator.payout_failed",
        json!({ "payout_id": p.id, "creator_id": p.creator_id, "amount": p.amount.to_string(), "currency": p.currency, "reason": p.note }),
    ));
    let amount = format!("{} {}", p.amount, p.currency);
    mail_creator(
        s,
        p.creator_id,
        "creator_payout_failed",
        &[("amount", &amount), ("reason", &p.note)],
    )
    .await;
}

// ------------------------------------------------------------------ refunds

/// After refunds/chargebacks: creators paid by Stripe whose balance went
/// negative get the difference pulled back from their newest transfers.
pub async fn clawback(
    s: &AppState,
    invoices: &[Uuid],
    cfg: &mk::Config,
) -> pelty_core::AppResult<usize> {
    if !cfg.reverse_transfers_on_refund || invoices.is_empty() {
        return Ok(0);
    }
    let Ok(st) = stripe() else { return Ok(0) };
    let t = s.http();
    let mut n = 0;
    for (creator, currency, balance) in cp::negative_balances(&s.pool, invoices).await? {
        let exp = cp::exponent(&s.pool, &currency).await?;
        let mut need = -balance;
        for (payout, transfer, left) in cp::reversible(&s.pool, creator, &currency).await? {
            if need <= rust_decimal::Decimal::ZERO {
                break;
            }
            let amount = need
                .min(left)
                .round_dp_with_strategy(exp, rust_decimal::RoundingStrategy::ToZero);
            let Some(minor) = gw::to_minor(amount, exp).filter(|m| *m > 0) else {
                continue;
            };
            let done = cp::reversal_ids(&s.pool, payout).await?.len();
            let key = format!("pelty-rev-{payout}-{done}");
            match st.reverse(&*t, &transfer, minor, &key).await {
                Ok((rev, got)) => {
                    let got = gw::from_minor(got, exp);
                    if cp::record_reversal(
                        &s.pool,
                        payout,
                        &rev,
                        got,
                        &format!("stripe reversal {rev}"),
                    )
                    .await?
                    {
                        n += 1;
                    }
                    need -= got;
                }
                Err(e) => {
                    tracing::warn!(payout = %payout, error = %e.message(), "transfer reversal failed");
                    cp::note(
                        &s.pool,
                        payout,
                        &format!("reversal failed: {}", e.message()),
                    )
                    .await?;
                }
            }
        }
    }
    Ok(n)
}

// ------------------------------------------------------------------ webhooks

/// Stripe Connect webhook (account, transfer and connected-account payout events).
#[utoipa::path(post, path = "/api/v1/webhooks/creator-payouts/stripe", request_body(content = String, content_type = "application/json"),
    responses((status = 200), (status = 400), (status = 404)))]
pub async fn stripe_webhook(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let st = stripe().map_err(prov)?;
    let sig = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !st.verify(sig, &body, time::OffsetDateTime::now_utc().unix_timestamp()) {
        return Err(AppError::BadRequest("error.webhook_signature").into());
    }
    let ev: Value =
        serde_json::from_slice(&body).map_err(|_| AppError::BadRequest("error.webhook_payload"))?;
    let (Some(id), Some(kind)) = (ev["id"].as_str(), ev["type"].as_str()) else {
        return Err(AppError::BadRequest("error.webhook_payload").into());
    };
    if !cp::first_event(&s.pool, "stripe", id, kind).await? {
        return Ok(Json(json!({ "ok": true, "duplicate": true })));
    }
    if let Err(e) = apply_stripe(&s, kind, &ev).await {
        // Let Stripe retry.
        cp::forget_event(&s.pool, "stripe", id).await?;
        return Err(e.into());
    }
    Ok(Json(json!({ "ok": true })))
}

async fn apply_stripe(s: &AppState, kind: &str, ev: &Value) -> pelty_core::AppResult<()> {
    let obj = &ev["data"]["object"];
    match kind {
        "account.updated" => {
            if let Some(acct) = obj["id"].as_str() {
                let (status, detail) = gw::account_status(obj);
                cp::set_stripe_status(&s.pool, acct, status, &detail).await?;
            }
        }
        "account.application.deauthorized" => {
            if let Some(acct) = ev["account"].as_str() {
                cp::set_stripe_status(&s.pool, acct, "disconnected", "deauthorized").await?;
            }
        }
        "transfer.created" | "transfer.updated" | "transfer.reversed" => {
            let Some(tr) = obj["id"].as_str() else {
                return Ok(());
            };
            let payout = match obj["metadata"]["payout_id"]
                .as_str()
                .and_then(|x| x.parse::<Uuid>().ok())
            {
                Some(p) => Some(p),
                None => cp::by_transfer(&s.pool, tr).await?,
            };
            let Some(payout) = payout else { return Ok(()) };
            // Settles a transfer whose API answer was lost.
            if let Some(p) = cp::provider_paid(&s.pool, payout, tr).await? {
                paid_event(s, &p).await;
            }
            let exp = cp::exponent(
                &s.pool,
                &obj["currency"].as_str().unwrap_or("").to_ascii_uppercase(),
            )
            .await?;
            for r in obj["reversals"]["data"].as_array().into_iter().flatten() {
                if let (Some(rid), Some(a)) = (r["id"].as_str(), r["amount"].as_i64()) {
                    if a > 0 {
                        cp::record_reversal(
                            &s.pool,
                            payout,
                            rid,
                            gw::from_minor(a, exp),
                            &format!("stripe reversal {rid}"),
                        )
                        .await?;
                    }
                }
            }
        }
        "payout.failed" => {
            // A bank payout on the creator's own Stripe account failed: the
            // money stays in their Stripe balance (our ledger is unchanged).
            if let Some(acct) = ev["account"].as_str() {
                let msg = obj["failure_message"]
                    .as_str()
                    .or(obj["failure_code"].as_str())
                    .unwrap_or("payout failed");
                let note = format!("bank payout failed: {msg}");
                if let Some(c) = cp::set_stripe_detail(&s.pool, acct, &note).await? {
                    let amount = match (obj["amount"].as_i64(), obj["currency"].as_str()) {
                        (Some(a), Some(cur)) => {
                            let cur = cur.to_ascii_uppercase();
                            format!(
                                "{} {cur}",
                                gw::from_minor(a, cp::exponent(&s.pool, &cur).await?)
                            )
                        }
                        _ => String::new(),
                    };
                    mail_creator(
                        s,
                        c,
                        "creator_payout_failed",
                        &[("amount", &amount), ("reason", &note)],
                    )
                    .await;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// PayPal Payouts webhook (verified with PayPal, then applied by item).
#[utoipa::path(post, path = "/api/v1/webhooks/creator-payouts/paypal", request_body(content = String, content_type = "application/json"),
    responses((status = 200), (status = 400), (status = 404)))]
pub async fn paypal_webhook(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let pp = paypal().map_err(prov)?;
    if !pp.webhook_ready() {
        return Err(prov(ProvErr::Unconfigured).into());
    }
    let ev: Value =
        serde_json::from_slice(&body).map_err(|_| AppError::BadRequest("error.webhook_payload"))?;
    let t = s.http();
    let tok = pp.token(&*t).await.map_err(prov)?;
    let h: std::collections::HashMap<String, String> = headers
        .iter()
        .filter(|(k, _)| k.as_str().starts_with("paypal-"))
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_string(), v.to_string()))
        })
        .collect();
    if !pp.verify(&*t, &tok, &h, &ev).await.map_err(prov)? {
        return Err(AppError::BadRequest("error.webhook_signature").into());
    }
    let (Some(id), Some(kind)) = (ev["id"].as_str(), ev["event_type"].as_str()) else {
        return Err(AppError::BadRequest("error.webhook_payload").into());
    };
    if !cp::first_event(&s.pool, "paypal", id, kind).await? {
        return Ok(Json(json!({ "ok": true, "duplicate": true })));
    }
    let r: pelty_core::AppResult<()> = async {
        if kind.starts_with("PAYMENT.PAYOUTS-ITEM.") {
            if let Some(i) = gw::item_status(&ev["resource"]) {
                apply_item(&s, &i).await?;
            }
        } else if kind.starts_with("PAYMENT.PAYOUTSBATCH.") {
            if let Some(b) = ev["resource"]["batch_header"]["payout_batch_id"].as_str() {
                for i in pp.batch(&*t, &tok, b).await.unwrap_or_default() {
                    apply_item(&s, &i).await?;
                }
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = r {
        cp::forget_event(&s.pool, "paypal", id).await?;
        return Err(e.into());
    }
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ schedule

/// Create payouts for every creator whose available balance reached the
/// minimum (scheduled run or staff "create now"). Returns how many.
pub async fn create_due(s: &AppState, cfg: &mk::Config) -> pelty_core::AppResult<usize> {
    let mut n = 0;
    for (creator, currency, available) in cp::due_creators(
        &s.pool,
        cfg.min_payout.max(rust_decimal::Decimal::new(1, 2)),
    )
    .await?
    {
        let c = mk::get(&s.pool, creator).await?;
        let exp = cp::exponent(&s.pool, &currency).await?;
        let amount = available.round_dp_with_strategy(exp, rust_decimal::RoundingStrategy::ToZero);
        if amount <= rust_decimal::Decimal::ZERO {
            continue;
        }
        match mk::request_payout(&s.pool, &c, &currency, amount, cfg, true).await {
            Ok(p) => {
                n += 1;
                s.emit(DomainEvent::new("creator.payout_requested", json!({ "payout_id": p.id, "creator_id": c.id, "amount": p.amount.to_string(), "currency": p.currency })));
            }
            Err(AppError::Conflict(_) | AppError::Validation(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Is a scheduled payout run due on this (company-local) date?
pub fn due_on(cfg: &mk::Config, d: time::Date) -> bool {
    match cfg.payout_schedule.as_str() {
        "weekly" => d.weekday().number_from_monday() == cfg.payout_weekday,
        "monthly" => d.day() == cfg.payout_monthday,
        _ => false,
    }
}

/// Scheduler: hourly sync while the module is on, plus the scheduled run
/// once on its due day.
pub async fn schedule(s: &AppState) -> pelty_core::AppResult<()> {
    if !feature_modules::is_enabled(&s.pool, mk::MODULE).await? {
        return Ok(());
    }
    let hour = time::OffsetDateTime::now_utc().unix_timestamp() / 3600;
    pelty_database::jobs::enqueue(
        &s.pool,
        SYNC_JOB,
        json!({}),
        None,
        Some(&format!("{SYNC_JOB}:{hour}")),
        3,
    )
    .await?;
    let cfg = mk::config(&s.pool).await?;
    let today = crate::routes::automation::local_today();
    if due_on(&cfg, today) {
        pelty_database::jobs::enqueue(
            &s.pool,
            AUTO_JOB,
            json!({ "date": today.to_string() }),
            None,
            Some(&format!("{AUTO_JOB}:{today}")),
            3,
        )
        .await?;
    }
    Ok(())
}

/// Worker entry for [`SYNC_JOB`] and [`AUTO_JOB`].
pub async fn run_job(s: &AppState, kind: &str) -> Result<Value, String> {
    let r: pelty_core::AppResult<Value> = async {
        if !feature_modules::is_enabled(&s.pool, mk::MODULE).await? {
            return Ok(json!({ "skipped": "module off" }));
        }
        let cfg = mk::config(&s.pool).await?;
        let created = if kind == AUTO_JOB {
            create_due(s, &cfg).await?
        } else {
            0
        };
        let sent = if cfg.payout_approval {
            0
        } else {
            let ids = requested_ids(s).await?;
            send_many(s, &ids).await?.sent.len()
        };
        let synced = sync_paypal(s).await?;
        Ok(json!({ "created": created, "sent": sent, "synced": synced }))
    }
    .await;
    r.map_err(|e| e.to_string())
}

/// Send a just-requested payout right away when approval is off.
pub fn kick_if_auto(s: &AppState, p: &Payout, cfg: &mk::Config) {
    if cfg.payout_approval || p.method == "manual" {
        return;
    }
    let s = s.clone();
    let id = p.id;
    tokio::spawn(async move {
        if let Err(e) = send_many(&s, &[id]).await {
            tracing::warn!(error = %e, "automatic payout send failed");
        }
    });
}

// ------------------------------------------------------------------ updates

/// Event hook: mail buyers when a new version of a digital product ships.
pub async fn on_event(s: &AppState, e: &DomainEvent) {
    if e.name != "digital.version_released" {
        return;
    }
    let r: pelty_core::AppResult<()> = async {
        if !feature_modules::is_enabled(&s.pool, "digital_products").await? {
            return Ok(());
        }
        let Some(product) = e.payload["product_id"]
            .as_str()
            .and_then(|x| x.parse::<Uuid>().ok())
        else {
            return Ok(());
        };
        let version = e.payload["version"].as_str().unwrap_or("").to_string();
        let name: String = sqlx::query_scalar("SELECT name FROM products WHERE id = $1")
            .bind(product)
            .fetch_optional(&s.pool)
            .await
            .map_err(pelty_database::db_err)?
            .unwrap_or_default();
        let link = format!("{}/digital", s.cfg.public_url.trim_end_matches('/'));
        for b in cp::buyers(&s.pool, product, 5000).await? {
            let locale = b
                .locale
                .filter(|l| s.i18n.has(l))
                .unwrap_or_else(|| s.cfg.default_locale.clone());
            crate::mail::enqueue(
                s,
                "digital_version_released",
                &locale,
                &b.email,
                crate::mail::vars(&[
                    ("name", &b.name),
                    ("product", &name),
                    ("version", &version),
                    ("link", &link),
                ]),
            )
            .await?;
        }
        Ok(())
    }
    .await;
    if let Err(err) = r {
        tracing::warn!(error = %err, "version update mails failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_days() {
        let mut cfg =
            mk::Config::from_value(&json!({"payout_schedule": "weekly", "payout_weekday": 3}));
        let wed = time::macros::date!(2026 - 11 - 11);
        assert!(due_on(&cfg, wed));
        assert!(!due_on(&cfg, time::macros::date!(2026 - 11 - 10)));
        cfg = mk::Config::from_value(&json!({"payout_schedule": "monthly", "payout_monthday": 11}));
        assert!(due_on(&cfg, wed));
        cfg = mk::Config::from_value(&json!({}));
        assert!(!due_on(&cfg, wed), "manual never runs");
        assert!(cfg.payout_approval);
    }
}
