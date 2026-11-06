//! Orders, invoices, services and jobs. Every `/client/*` handler resolves
//! the caller's customer from the session and filters by it in SQL, so a
//! customer can never read another customer's records (404, not 403, to
//! avoid leaking existence).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::{customer_for, customer_of};
use crate::routes::{paged, ListQuery};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::money::{Currency, Money};
use pelty_core::AppError;
use pelty_database::invoices::{self, NewItem};
use pelty_database::orders::{self, CartItem};
use pelty_database::{audit, invoice_ops, jobs, services};
use pelty_extensions::events::DomainEvent;
use pelty_payments::{PaymentInit, PaymentRequest};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const DUE_DAYS: i64 = 7;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/client/orders",
            get(client_orders).post(client_place_order),
        )
        .route("/client/invoices", get(client_invoices))
        .route("/client/invoices/{id}", get(client_invoice))
        .route("/client/invoices/{id}/pay", post(client_pay))
        .route("/client/invoices/pay-many", post(client_pay_many))
        .route(
            "/client/invoices/pay-many/gateways",
            get(client_pay_many_gateways),
        )
        .route("/client/gateways", get(client_gateways))
        .route("/client/services", get(client_services))
        .route("/client/services/{id}", get(client_service))
        .route("/client/dashboard", get(client_dashboard))
        .route("/admin/dashboard", get(admin_dashboard))
        .route(
            "/admin/orders",
            get(admin_orders).post(crate::routes::store::admin_create_order),
        )
        .route(
            "/admin/orders/{id}",
            get(admin_order).delete(admin_delete_order),
        )
        .route("/admin/orders/{id}/pending", post(admin_pending_order))
        .route("/admin/orders/{id}/approve", post(admin_approve))
        .route("/admin/orders/{id}/cancel", post(admin_cancel))
        .route(
            "/admin/orders/{id}/refund",
            post(crate::routes::store::admin_refund_order),
        )
        .route(
            "/admin/orders/{id}/notes",
            post(crate::routes::store::admin_add_note),
        )
        .route(
            "/admin/invoices",
            get(admin_invoices).post(admin_create_invoice),
        )
        .route("/admin/invoices/{id}", get(admin_invoice))
        .route("/admin/invoices/{id}/finalize", post(admin_finalize))
        .route("/admin/invoices/{id}/void", post(admin_void))
        .route("/admin/invoices/{id}/credit-note", post(admin_credit_note))
        .route("/admin/invoices/{id}/debit-note", post(admin_debit_note))
        .route("/admin/invoices/{id}/payments", post(admin_record_payment))
        .route("/admin/services", get(admin_services))
        .route("/admin/services/{id}", get(admin_service))
        .route("/admin/services/{id}/action", post(admin_service_action))
        .route("/admin/jobs", get(admin_jobs))
        .route("/admin/jobs/{id}/retry", post(admin_retry_job))
        .route("/admin/jobs/recurring", post(admin_run_recurring))
}

fn status_filter<'a>(q: &'a ListQuery, allowed: &[&str]) -> ApiResult<Option<&'a str>> {
    match q.status() {
        Some(s) if !allowed.contains(&s) => {
            Err(AppError::Validation("error.invalid_status").into())
        }
        s => Ok(s),
    }
}

const INVOICE_STATUSES: &[&str] = &["draft", "unpaid", "paid", "void", "refunded"];
const ORDER_STATUSES: &[&str] = &["pending", "active", "cancelled", "fraud"];

async fn invoice_detail(s: &AppState, inv: invoices::Invoice) -> ApiResult<Value> {
    let items = s.invoices().items(inv.id).await?;
    let payments = s.invoices().payments(inv.id).await?;
    let debit_notes = if inv.kind == "invoice" {
        s.invoices().debit_notes_for(inv.id).await?
    } else {
        Vec::new()
    };
    let related = match inv.credit_for {
        Some(r) => s
            .invoices()
            .get(r)
            .await?
            .map(|o| json!({ "id": o.id, "number": o.number, "kind": o.kind })),
        None => None,
    };
    let reason = if inv.kind == "invoice" {
        String::new()
    } else {
        s.invoices().note_reason(inv.id).await?
    };
    let (_, client_notes, merged_into, split_from) =
        pelty_database::invoice_ops::notes(&s.pool, inv.id).await?;
    Ok(
        json!({ "invoice": inv, "items": items, "payments": payments, "balance": inv.balance(),
        "debit_notes": debit_notes, "related": related, "note_reason": reason,
        "client_notes": client_notes, "merged_into": merged_into, "split_from": split_from }),
    )
}

// client

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceOrder {
    /// Defaults to the customer's currency.
    pub currency: Option<String>,
    pub items: Vec<CartItem>,
    /// Optional promo code.
    #[serde(default)]
    pub coupon: Option<String>,
    /// Bundle the cart fulfils (discount applies when every item is present).
    #[serde(default)]
    pub bundle_id: Option<Uuid>,
    /// Order form template used (validates products and extra fields).
    #[serde(default)]
    pub order_form_id: Option<Uuid>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub form_answers: Value,
}

#[utoipa::path(post, path = "/api/v1/client/orders", request_body = PlaceOrder, responses((status = 200), (status = 401), (status = 422)))]
pub async fn client_place_order(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<PlaceOrder>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "orders").await?;
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
    let bundle = match b.bundle_id {
        Some(id) => Some(crate::routes::bundles::discount_for(&s, id, &b.items).await?),
        None => None,
    };
    let form = match b.order_form_id {
        Some(id) => {
            let f = pelty_database::order_forms::get(&s.pool, id)
                .await?
                .filter(|f| f.active)
                .ok_or(AppError::NotFound)?;
            let ids: Vec<Uuid> = b.items.iter().map(|i| i.product_id).collect();
            if !pelty_database::order_forms::allows(&s.pool, &f, &ids).await? {
                return Err(AppError::Validation("error.order_form_product").into());
            }
            let answers = pelty_database::order_forms::check_answers(&f, &b.form_answers)?;
            Some((f, answers))
        }
        None => None,
    };
    let (order, invoice) = place_order(
        &s,
        &u,
        cid,
        &currency,
        &b.items,
        b.coupon.as_deref(),
        bundle.as_ref(),
    )
    .await?;
    let redirect = match &form {
        Some((f, answers)) => {
            pelty_database::order_forms::attach(&s.pool, order.id, f.id, answers).await?;
            f.redirect_url.clone()
        }
        None => None,
    };
    Ok(Json(
        json!({ "order": order, "invoice": invoice, "redirect_url": redirect }),
    ))
}

/// Place an order for `cid` (order + finalized invoice), run fraud
/// screening and emit the order/invoice events. Shared by the client order
/// form, the storefront checkout and admin manual orders.
#[allow(clippy::too_many_arguments)]
/// Filter `checkout.discounts`: addon discounts priced before the order
/// transaction (money changes need `money.modify` and are audited by the
/// filter runner). `place_full` re-quotes and clamps them.
async fn addon_discounts(
    s: &AppState,
    cid: Uuid,
    currency: &str,
    items: &[CartItem],
) -> ApiResult<Vec<orders::AddonDiscount>> {
    if !crate::addon_filters::has(s, "checkout.discounts") || items.is_empty() {
        return Ok(vec![]);
    }
    let Ok(quotes) = s.orders().line_amounts(currency, items).await else {
        return Ok(vec![]);
    };
    let lines: Vec<Value> = quotes
        .iter()
        .enumerate()
        .map(|(i, q)| json!({ "id": i, "product_id": q.product_id, "cycle": q.cycle, "amount": q.amount.to_string(), "discount": "0", "label": "" }))
        .collect();
    let out = crate::addon_filters::apply(
        s,
        "checkout.discounts",
        json!({ "lines": lines }),
        json!({ "customer_id": cid, "currency": currency }),
    )
    .await;
    let mut v = vec![];
    for l in out["lines"].as_array().into_iter().flatten() {
        let (Some(item), Some(amount)) = (
            l["id"].as_u64().map(|i| i as usize),
            l.get("discount").and_then(crate::addon_ext::exact_money),
        ) else {
            continue;
        };
        if item >= quotes.len() || amount <= rust_decimal::Decimal::ZERO {
            continue;
        }
        let label = l["label"]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("Addon discount");
        v.push(orders::AddonDiscount {
            item,
            amount,
            label: label.chars().take(120).collect(),
        });
    }
    Ok(v.into_iter().take(50).collect())
}

pub(crate) async fn place_order(
    s: &AppState,
    u: &CurrentUser,
    cid: Uuid,
    currency: &str,
    items: &[CartItem],
    coupon: Option<&str>,
    bundle: Option<&pelty_database::orders::BundleDiscount>,
) -> ApiResult<(orders::Order, invoices::Invoice)> {
    pelty_database::kyc::ensure_can_order(&s.pool, cid).await?;
    // Pre-event: addons may cancel, or set/clear the coupon code.
    let pre = crate::addon_ext::pre_event(
        s,
        "order.before_place",
        json!({ "customer_id": cid, "currency": currency, "items": items, "coupon": coupon }),
    )
    .await?;
    let coupon_owned: Option<String> = match pre.get("coupon") {
        Some(Value::String(c)) if !c.trim().is_empty() && c.len() <= 64 => Some(c.clone()),
        Some(Value::Null) => None,
        _ => coupon.map(String::from),
    };
    let coupon = coupon_owned.as_deref();
    let discounts = addon_discounts(s, cid, currency, items).await?;
    let (order, invoice) = s
        .orders()
        .place_full(cid, currency, items, coupon, DUE_DAYS, bundle, &discounts)
        .await?;
    if !discounts.is_empty() {
        let applied: Vec<Value> = discounts
            .iter()
            .map(|d| json!({ "item": d.item, "amount": d.amount.to_string(), "label": d.label }))
            .collect();
        audit::record(
            &s.pool,
            Some(u.user_id),
            "order.addon_discounts",
            Some(&order.id.to_string()),
            u.ip.as_deref(),
            json!({ "requested": applied }),
        )
        .await?;
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.created",
        Some(&order.id.to_string()),
        u.ip.as_deref(),
        json!({ "number": order.number, "total": order.total }),
    )
    .await?;
    if let Some(f) = crate::fraud::screen(
        s,
        order.id,
        cid,
        u.ip.as_deref(),
        &u.signals,
        order.total,
        &order.currency,
    )
    .await?
    {
        match f.verdict {
            "reject" => {
                s.orders().cancel(order.id, true).await?;
                audit::record(
                    &s.pool,
                    None,
                    "order.fraud_rejected",
                    Some(&order.id.to_string()),
                    u.ip.as_deref(),
                    json!(f),
                )
                .await?;
                s.emit(DomainEvent::new(
                    "order.fraud_rejected",
                    json!({ "order_id": order.id, "customer_id": cid, "score": f.score }),
                ));
                return Err(AppError::Validation("error.order_rejected").into());
            }
            "review" => {
                audit::record(
                    &s.pool,
                    None,
                    "order.fraud_review",
                    Some(&order.id.to_string()),
                    u.ip.as_deref(),
                    json!(f),
                )
                .await?;
                s.emit(DomainEvent::new("order.fraud_review", json!({ "order_id": order.id, "customer_id": cid, "score": f.score, "check_id": f.check_id })));
            }
            _ => {}
        }
    }
    s.emit(DomainEvent::new("order.created", json!({ "order_id": order.id, "customer_id": cid, "total": order.total, "currency": order.currency })));
    s.emit(DomainEvent::new(
        "invoice.created",
        json!({ "invoice_id": invoice.id, "customer_id": cid, "number": invoice.number }),
    ));
    if invoice.status == "paid" {
        s.emit(DomainEvent::new(
            "invoice.paid",
            json!({ "invoice_id": invoice.id, "customer_id": cid }),
        ));
    }
    Ok((order, invoice))
}

#[utoipa::path(get, path = "/api/v1/client/orders", params(ListQuery), responses((status = 200), (status = 401)))]
pub async fn client_orders(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "orders").await?;
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = orders::list(
        &s.pool,
        Some(cid),
        status_filter(&q, ORDER_STATUSES)?,
        limit,
        offset,
    )
    .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/client/invoices", params(ListQuery), responses((status = 200), (status = 401)))]
pub async fn client_invoices(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "invoices").await?;
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = invoices::list(
        &s.pool,
        Some(cid),
        status_filter(&q, INVOICE_STATUSES)?,
        limit,
        offset,
    )
    .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/client/invoices/{id}", responses((status = 200), (status = 404)))]
pub async fn client_invoice(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "invoices").await?;
    let inv = s
        .invoices()
        .get_owned(id, cid)
        .await?
        .ok_or(AppError::NotFound)?;
    let detail = invoice_detail(&s, inv).await?;
    if !crate::addon_filters::has(&s, "invoice.view") {
        return Ok(Json(detail));
    }
    // Display-only filter: money keys always stay as stored.
    Ok(Json(
        crate::addon_filters::apply_with(
            &s,
            "invoice.view",
            detail,
            json!({ "customer_id": cid, "invoice_id": id }),
            false,
        )
        .await,
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PayBody {
    pub gateway: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PayManyBody {
    pub invoice_ids: Vec<Uuid>,
    pub gateway: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PayManyGatewaysQuery {
    /// Comma-separated invoice ids (2-50).
    pub ids: String,
}

async fn load_pay_many(s: &AppState, cid: Uuid, ids: &[Uuid]) -> ApiResult<Vec<invoices::Invoice>> {
    let mut uniq = ids.to_vec();
    uniq.sort();
    uniq.dedup();
    if uniq.len() < 2 || uniq.len() > 50 {
        return Err(AppError::Validation("error.merge_needs_two").into());
    }
    let mut invs = Vec::with_capacity(uniq.len());
    for id in &uniq {
        let inv = s
            .invoices()
            .get_owned(*id, cid)
            .await?
            .ok_or(AppError::NotFound)?;
        invs.push(inv);
    }
    Ok(invs)
}

fn public_url() -> String {
    std::env::var("PELTY_PUBLIC_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "http://localhost:8080".into())
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct GatewaysQuery {
    /// Only gateways that accept this invoice's balance and currency.
    pub invoice_id: Option<Uuid>,
}

/// Enabled payment gateways (ids only; never config or secrets). With
/// `invoice_id`, gateways whose limits/currencies exclude the invoice are
/// left out and `fees` shows each gateway's processing fee.
#[utoipa::path(get, path = "/api/v1/client/gateways", params(GatewaysQuery), responses((status = 200)))]
pub async fn client_gateways(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<GatewaysQuery>,
) -> ApiResult<Json<Value>> {
    let ids: Vec<String> = pelty_database::extensions::gateways(&s.pool)
        .await?
        .into_iter()
        .filter(|g| g.enabled)
        .map(|g| g.id)
        .collect();
    let Some(iid) = q.invoice_id else {
        return Ok(Json(json!({ "items": ids })));
    };
    let cid = customer_for(&s, &u, "invoices").await?;
    let inv = s
        .invoices()
        .get_owned(iid, cid)
        .await?
        .ok_or(AppError::NotFound)?;
    let exp = pelty_database::branding::exponent(&s.pool, &inv.currency).await?;
    let (mut items, mut fees, mut excluded) =
        (vec![], serde_json::Map::new(), serde_json::Map::new());
    for id in ids {
        let r = pelty_database::gateway_rules::get(&s.pool, &id).await?;
        match r.check(&inv.currency, inv.balance()) {
            Ok(()) => {
                fees.insert(id.clone(), json!(r.fee(&inv.currency, inv.balance(), exp)));
                items.push(id);
            }
            Err(code) => {
                excluded.insert(id, json!(code));
            }
        }
    }
    Ok(Json(
        json!({ "items": items, "fees": fees, "excluded": excluded }),
    ))
}

/// Gateways that can take the combined balance of several unpaid invoices.
#[utoipa::path(get, path = "/api/v1/client/invoices/pay-many/gateways", params(PayManyGatewaysQuery), responses((status = 200), (status = 404), (status = 422)))]
pub async fn client_pay_many_gateways(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<PayManyGatewaysQuery>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "invoices").await?;
    let ids: Vec<Uuid> = q
        .ids
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Validation("error.validation"))?;
    let invs = load_pay_many(&s, cid, &ids).await?;
    let currency = invs[0].currency.clone();
    let amount = invs.iter().fold(Decimal::ZERO, |a, i| a + i.balance());
    let items = crate::routes::gateways::eligible_for(&s, &currency, amount).await?;
    Ok(Json(json!({
        "items": items,
        "currency": currency,
        "amount": amount,
        "invoice_ids": invs.iter().map(|i| i.id).collect::<Vec<_>>(),
        "count": invs.len(),
    })))
}

/// Pay several unpaid invoices in one gateway checkout without merging them.
#[utoipa::path(post, path = "/api/v1/client/invoices/pay-many", request_body = PayManyBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn client_pay_many(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<PayManyBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "invoices").await?;
    let invs = load_pay_many(&s, cid, &b.invoice_ids).await?;
    let currency = &invs[0].currency;
    let amount = invs.iter().fold(Decimal::ZERO, |a, i| a + i.balance());
    let row = pelty_database::extensions::gateway(&s.pool, &b.gateway)
        .await?
        .filter(|r| r.enabled)
        .ok_or(AppError::Validation("error.unknown_gateway"))?;
    let desc = s
        .gateways
        .descriptor(&b.gateway)
        .ok_or(AppError::Validation("error.unknown_gateway"))?;
    if !crate::addons::gateway_enabled(&s, &b.gateway).await? {
        return Err(AppError::Validation("error.unknown_gateway").into());
    }
    if !crate::routes::gateways::eligible(&desc, &row, currency, amount) {
        return Err(AppError::Validation("error.gateway_ineligible").into());
    }
    let gw = s
        .gateways
        .build(&b.gateway, &crate::routes::webhooks::resolve_config(&row))
        .map_err(|e| {
            tracing::error!(gateway = %b.gateway, error = %e, "gateway misconfigured");
            AppError::BadRequest("error.gateway")
        })?;
    let mut id_bits: Vec<String> = invs.iter().map(|i| i.id.to_string()).collect();
    id_bits.sort();
    let key = format!(
        "checkout-many:{}:{}:{}",
        id_bits.join(","),
        b.gateway,
        amount.normalize()
    );
    let rules = pelty_database::gateway_rules::get(&s.pool, &b.gateway).await?;
    rules
        .check(currency, amount)
        .map_err(AppError::Validation)?;
    let pay = s
        .invoices()
        .create_pending_multi(&invs, &b.gateway, Some(u.user_id), &key)
        .await?;
    let exp = pelty_database::branding::exponent(&s.pool, currency).await?;
    pelty_database::gateway_rules::record_fee(
        &s.pool,
        pay.id,
        rules.fee(currency, pay.amount, exp),
    )
    .await?;
    let cur = Currency::parse(currency).ok_or(AppError::Validation("error.invalid_currency"))?;
    let email = s.clients().get(cid).await?.map(|c| c.email);
    let base = public_url();
    let ids_q = id_bits.join(",");
    let req = PaymentRequest {
        payment_id: pay.id.to_string(),
        reference: format!("invoices:{}", ids_q),
        amount: Money::new(pay.amount, cur),
        description: format!("Invoices ({})", invs.len()),
        customer_email: email,
        success_url: format!(
            "{base}/invoices/pay-many?ids={ids_q}&payment={}&return=1",
            pay.id
        ),
        cancel_url: format!("{base}/invoices/pay-many?ids={ids_q}&cancelled=1"),
        webhook_url: format!("{base}/api/v1/webhooks/{}", b.gateway),
        idempotency_key: key,
    };
    let init = gw.create_payment(&req).await.map_err(|e| {
        tracing::warn!(gateway = %b.gateway, error = %e, "create_payment failed");
        AppError::BadRequest("error.gateway")
    })?;
    if let PaymentInit::Redirect { provider_ref, .. } = &init {
        s.invoices().set_provider_ref(pay.id, provider_ref).await?;
    }
    let bank = if matches!(init, PaymentInit::Instructions { .. }) {
        Some(pelty_database::settings::bank_transfer(&s.pool).await?).filter(|b| b.is_configured())
    } else {
        None
    };
    Ok(Json(
        json!({ "payment_id": pay.id, "init": init, "bank": bank, "invoice_ids": invs.iter().map(|i| i.id).collect::<Vec<_>>() }),
    ))
}

/// Start paying an invoice with a gateway: redirect (Stripe) or manual
/// instructions. Repeated calls reuse the same pending payment.
#[utoipa::path(post, path = "/api/v1/client/invoices/{id}/pay", request_body = PayBody, responses((status = 200), (status = 404), (status = 409)))]
pub async fn client_pay(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<PayBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "invoices").await?;
    let inv = s
        .invoices()
        .get_owned(id, cid)
        .await?
        .ok_or(AppError::NotFound)?;
    let row = pelty_database::extensions::gateway(&s.pool, &b.gateway)
        .await?
        .filter(|r| r.enabled)
        .ok_or(AppError::Validation("error.unknown_gateway"))?;
    let desc = s
        .gateways
        .descriptor(&b.gateway)
        .ok_or(AppError::Validation("error.unknown_gateway"))?;
    if !crate::addons::gateway_enabled(&s, &b.gateway).await? {
        return Err(AppError::Validation("error.unknown_gateway").into());
    }
    if !crate::routes::gateways::eligible(&desc, &row, &inv.currency, inv.balance()) {
        return Err(AppError::Validation("error.gateway_ineligible").into());
    }
    let gw = s
        .gateways
        .build(&b.gateway, &crate::routes::webhooks::resolve_config(&row))
        .map_err(|e| {
            tracing::error!(gateway = %b.gateway, error = %e, "gateway misconfigured");
            AppError::BadRequest("error.gateway")
        })?;
    let key = format!(
        "checkout:{}:{}:{}",
        inv.id,
        b.gateway,
        inv.amount_paid.normalize()
    );
    let rules = pelty_database::gateway_rules::get(&s.pool, &b.gateway).await?;
    rules
        .check(&inv.currency, inv.balance())
        .map_err(AppError::Validation)?;
    let pay = s
        .invoices()
        .create_pending(&inv, &b.gateway, Some(u.user_id), &key)
        .await?;
    let exp = pelty_database::branding::exponent(&s.pool, &inv.currency).await?;
    pelty_database::gateway_rules::record_fee(
        &s.pool,
        pay.id,
        rules.fee(&inv.currency, pay.amount, exp),
    )
    .await?;
    let currency =
        Currency::parse(&inv.currency).ok_or(AppError::Validation("error.invalid_currency"))?;
    let email = s.clients().get(cid).await?.map(|c| c.email);
    let base = public_url();
    let req = PaymentRequest {
        payment_id: pay.id.to_string(),
        reference: inv.number.clone().unwrap_or_default(),
        amount: Money::new(pay.amount, currency),
        description: format!("Invoice {}", inv.number.clone().unwrap_or_default()),
        customer_email: email,
        success_url: format!("{base}/invoices/{}/pay?payment={}&return=1", inv.id, pay.id),
        cancel_url: format!("{base}/invoices/{}/pay?cancelled=1", inv.id),
        webhook_url: format!("{base}/api/v1/webhooks/{}", b.gateway),
        idempotency_key: key,
    };
    let init = gw.create_payment(&req).await.map_err(|e| {
        tracing::warn!(gateway = %b.gateway, error = %e, "create_payment failed");
        AppError::BadRequest("error.gateway")
    })?;
    if let PaymentInit::Redirect { provider_ref, .. } = &init {
        s.invoices().set_provider_ref(pay.id, provider_ref).await?;
    }
    // Manual transfer: attach operator bank details (null when unconfigured;
    // the UI then shows a neutral translated message, never dummy data).
    let bank = if matches!(init, PaymentInit::Instructions { .. }) {
        Some(pelty_database::settings::bank_transfer(&s.pool).await?).filter(|b| b.is_configured())
    } else {
        None
    };
    Ok(Json(
        json!({ "payment_id": pay.id, "init": init, "bank": bank }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/services", params(ListQuery), responses((status = 200), (status = 401)))]
pub async fn client_services(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "services").await?;
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = services::list(
        &s.pool,
        Some(cid),
        status_filter(&q, pelty_billing::lifecycle::STATUSES)?,
        limit,
        offset,
    )
    .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/client/services/{id}", responses((status = 200), (status = 404)))]
pub async fn client_service(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_for(&s, &u, "services").await?;
    let svc = s
        .hosting()
        .get_owned(id, cid)
        .await?
        .ok_or(AppError::NotFound)?;
    let Json(v) = service_json(&s, svc).await?;
    if !crate::addon_filters::has(&s, "service.view") {
        return Ok(Json(v));
    }
    Ok(Json(
        crate::addon_filters::apply_with(
            &s,
            "service.view",
            v,
            json!({ "customer_id": cid, "service_id": id }),
            false,
        )
        .await,
    ))
}

/// A service with its addons attached.
async fn service_json(s: &AppState, svc: services::Service) -> ApiResult<Json<Value>> {
    let addons = pelty_database::addons::for_service(&s.pool, svc.id).await?;
    let mut v = json!(svc);
    v["addons"] = json!(addons);
    Ok(Json(v))
}

// admin

#[utoipa::path(get, path = "/api/v1/admin/orders", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_orders(
    State(s): State<AppState>,
    _p: RequirePermission<perms::OrdersManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = orders::list(
        &s.pool,
        None,
        status_filter(&q, ORDER_STATUSES)?,
        limit,
        offset,
    )
    .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/orders/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_order(
    State(s): State<AppState>,
    _p: RequirePermission<perms::OrdersManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    // Accept the UUID or the order number shown in lists (e.g. /admin/orders/1005).
    let o = if let Ok(uuid) = Uuid::parse_str(&id) {
        s.orders().get(uuid).await?
    } else if let Ok(n) = id.trim_start_matches('#').parse::<i64>() {
        s.orders().get_by_number(n).await?
    } else {
        None
    }
    .ok_or(AppError::NotFound)?;
    Ok(Json(crate::routes::store::admin_order_detail(&s, o).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/orders/{id}/approve", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_approve(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    crate::addon_ext::pre_event(
        &s,
        "order.before_approve",
        json!({ "order_id": id, "by": u.user_id }),
    )
    .await?;
    let ids = s.orders().approve(id, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.approved",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "services": ids }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "order.approved",
        json!({ "order_id": id, "services": ids }),
    ));
    for sid in &ids {
        s.emit(DomainEvent::new(
            "service.created",
            json!({ "service_id": sid, "order_id": id }),
        ));
        crate::provision::on_transition(&s, *sid, "pending", "active").await;
    }
    Ok(Json(json!({ "services": ids })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema, Default)]
pub struct CancelBody {
    #[serde(default)]
    pub fraud: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/orders/{id}/cancel", request_body = CancelBody, responses((status = 200), (status = 409)))]
pub async fn admin_cancel(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CancelBody>,
) -> ApiResult<Json<Value>> {
    crate::addon_ext::pre_event(
        &s,
        "order.before_cancel",
        json!({ "order_id": id, "fraud": b.fraud, "by": u.user_id }),
    )
    .await?;
    let o = s.orders().cancel(id, b.fraud).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.cancelled",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "fraud": b.fraud }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "order.cancelled",
        json!({ "order_id": id, "fraud": b.fraud }),
    ));
    Ok(Json(json!(o)))
}

#[utoipa::path(post, path = "/api/v1/admin/orders/{id}/pending", responses((status = 200), (status = 409)))]
pub async fn admin_pending_order(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let o = s.orders().reopen(id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.reopened",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(DomainEvent::new(
        "order.reopened",
        json!({ "order_id": id, "by": u.user_id }),
    ));
    Ok(Json(json!(o)))
}

#[utoipa::path(delete, path = "/api/v1/admin/orders/{id}", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_delete_order(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::OrdersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    crate::addon_ext::pre_event(
        &s,
        "order.before_delete",
        json!({ "order_id": id, "by": u.user_id }),
    )
    .await?;
    s.orders().delete(id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "order.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(DomainEvent::new(
        "order.deleted",
        json!({ "order_id": id, "by": u.user_id }),
    ));
    Ok(Json(json!({ "deleted": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/invoices", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_invoices(
    State(s): State<AppState>,
    _p: RequirePermission<perms::InvoicesManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = invoices::list(
        &s.pool,
        None,
        status_filter(&q, INVOICE_STATUSES)?,
        limit,
        offset,
    )
    .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/invoices/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_invoice(
    State(s): State<AppState>,
    _p: RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(invoice_detail(&s, inv).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemBody {
    pub description: String,
    #[schema(value_type = String)]
    pub quantity: Decimal,
    #[schema(value_type = String)]
    pub unit_price: Decimal,
    /// Omit to let the tax engine price this line at finalization.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub tax_rate: Option<Decimal>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInvoice {
    pub customer_id: Uuid,
    pub currency: String,
    pub items: Vec<ItemBody>,
    #[serde(default)]
    pub finalize: bool,
    /// Days until due when finalizing (0-365). Omit for the default (7, or the legal entity).
    #[serde(default)]
    pub due_days: Option<i32>,
    #[serde(default)]
    pub admin_notes: Option<String>,
    #[serde(default)]
    pub client_notes: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/invoices", request_body = CreateInvoice, responses((status = 200), (status = 422)))]
pub async fn admin_create_invoice(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Json(b): Json<CreateInvoice>,
) -> ApiResult<Json<Value>> {
    if b.items.is_empty() || b.items.len() > 100 {
        return Err(AppError::Validation("error.validation").into());
    }
    if b.due_days.is_some_and(|d| !(0..=365).contains(&d)) {
        return Err(AppError::Validation("error.validation").into());
    }
    let cur = b.currency.to_ascii_uppercase();
    // Pre-event: addons may cancel or rewrite line descriptions; unit prices
    // change only through addons holding `money.modify` (enforced in run_hooks).
    let pre = crate::addon_ext::pre_event(
        &s,
        "invoice.before_create",
        json!({ "customer_id": b.customer_id, "currency": cur, "finalize": b.finalize,
                "items": b.items.iter().map(|i| json!({ "description": i.description, "quantity": i.quantity.to_string(), "unit_price": i.unit_price.to_string() })).collect::<Vec<_>>() }),
    )
    .await?;
    let descs: Vec<Option<String>> = pre
        .get("items")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|i| {
                    i.get("description")
                        .and_then(Value::as_str)
                        .filter(|d| !d.trim().is_empty() && d.len() <= 500)
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    let prices: Vec<Option<rust_decimal::Decimal>> = pre
        .get("items")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|i| i.get("unit_price").and_then(crate::addon_ext::exact_money))
                .collect()
        })
        .unwrap_or_default();
    let mut lines: Vec<NewItem> = b
        .items
        .iter()
        .map(|i| NewItem {
            description: i.description.clone(),
            quantity: i.quantity,
            unit_price: i.unit_price,
            tax_rate: i.tax_rate.unwrap_or_default(),
            service_id: None,
            period: None,
            auto_tax: i.tax_rate.is_none(),
        })
        .collect();
    for (l, d) in lines.iter_mut().zip(descs) {
        if let Some(d) = d {
            l.description = d;
        }
    }
    for (l, p) in lines.iter_mut().zip(prices) {
        if let Some(p) = p.filter(|p| !p.is_sign_negative()) {
            l.unit_price = p;
        }
    }
    let due = b.due_days.map(i64::from).unwrap_or(DUE_DAYS);
    let mut tx = s.pool.begin().await.map_err(pelty_database::db_err)?;
    let id = invoices::create_draft(&mut tx, b.customer_id, &cur, None).await?;
    invoices::add_items(&mut tx, id, &cur, &lines).await?;
    if b.finalize {
        invoices::finalize(&mut tx, id, due).await?;
    }
    tx.commit().await.map_err(pelty_database::db_err)?;
    let admin_notes = b.admin_notes.as_deref().unwrap_or("");
    let client_notes = b.client_notes.as_deref().unwrap_or("");
    if !admin_notes.trim().is_empty() || !client_notes.trim().is_empty() {
        invoice_ops::set_notes(&s.pool, id, admin_notes, client_notes).await?;
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "finalized": b.finalize, "due_days": due }),
    )
    .await?;
    if b.finalize {
        s.emit(DomainEvent::new(
            "invoice.created",
            json!({ "invoice_id": id, "customer_id": b.customer_id }),
        ));
    }
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(invoice_detail(&s, inv).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/finalize", responses((status = 200), (status = 409)))]
pub async fn admin_finalize(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await.map_err(pelty_database::db_err)?;
    let inv = invoices::finalize(&mut tx, id, DUE_DAYS).await?;
    tx.commit().await.map_err(pelty_database::db_err)?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.finalized",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "number": inv.number }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "invoice.created",
        json!({ "invoice_id": id, "customer_id": inv.customer_id, "number": inv.number }),
    ));
    Ok(Json(json!(inv)))
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/void", responses((status = 200), (status = 409)))]
pub async fn admin_void(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let inv = s.invoices().void(id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.voided",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(DomainEvent::new(
        "invoice.voided",
        json!({ "invoice_id": id }),
    ));
    Ok(Json(json!(inv)))
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/credit-note", responses((status = 200), (status = 409)))]
pub async fn admin_credit_note(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cn = s.invoices().credit_note(id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.credit_note",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "credit_note": cn.id }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "invoice.refunded",
        json!({ "invoice_id": id, "credit_note_id": cn.id }),
    ));
    Ok(Json(json!(cn)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DebitNoteBody {
    pub lines: Vec<invoices::DebitLine>,
    #[serde(default)]
    pub reason: String,
}

/// Issue a debit note (extra charge) against an unpaid or paid invoice.
/// The note is numbered in its own DN sequence, taxed by the tax engine and
/// payable like any invoice.
#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/debit-note", request_body = DebitNoteBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_debit_note(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<DebitNoteBody>,
) -> ApiResult<Json<Value>> {
    let dn = s
        .invoices()
        .debit_note(id, &b.lines, &b.reason, DUE_DAYS)
        .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.debit_note",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "debit_note": dn.id, "total": dn.total }),
    )
    .await?;
    s.emit(DomainEvent::new("invoice.created", json!({ "invoice_id": dn.id, "customer_id": dn.customer_id, "number": dn.number, "debit_for": id })));
    Ok(Json(json!(dn)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordPayment {
    #[schema(value_type = String)]
    pub amount: Decimal,
    /// Client-chosen key; replays with the same key are no-ops.
    pub idempotency_key: String,
    /// Bank reference etc.
    pub reference: Option<String>,
    #[serde(default = "manual")]
    pub gateway: String,
}
fn manual() -> String {
    "manual".into()
}

/// Record a manual payment (bank transfer…). Idempotent.
#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/payments", request_body = RecordPayment, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_record_payment(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::InvoicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<RecordPayment>,
) -> ApiResult<Json<Value>> {
    let key = b.idempotency_key.trim();
    if key.is_empty() || key.len() > 200 || b.reference.as_ref().is_some_and(|r| r.len() > 200) {
        return Err(AppError::Validation("error.validation").into());
    }
    let applied = invoices::record_payment(
        &s.pool,
        id,
        &b.gateway,
        b.reference.as_deref(),
        b.amount,
        &format!("manual:{id}:{key}"),
        Some(u.user_id),
    )
    .await?;
    if !applied.duplicate {
        audit::record(
            &s.pool,
            Some(u.user_id),
            "payment.recorded",
            Some(&id.to_string()),
            u.ip.as_deref(),
            json!({ "amount": b.amount, "payment_id": applied.payment_id }),
        )
        .await?;
        s.emit(DomainEvent::new(
            "payment.succeeded",
            json!({ "payment_id": applied.payment_id, "invoice_id": id, "gateway": b.gateway }),
        ));
        if applied.invoice_paid {
            s.emit(DomainEvent::new(
                "invoice.paid",
                json!({ "invoice_id": id }),
            ));
            crate::changes::after_payment(&s, id).await;
        }
    }
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "applied": applied, "invoice": inv })))
}

#[utoipa::path(get, path = "/api/v1/admin/services", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_services(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let status = status_filter(&q, pelty_billing::lifecycle::STATUSES)?;
    if let Some(id) = q.q().and_then(|q| Uuid::parse_str(q).ok()) {
        let service = s
            .hosting()
            .get(id)
            .await?
            .filter(|service| status.is_none_or(|status| service.status == status));
        let total = i64::from(service.is_some());
        let items: Vec<_> = service
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        return Ok(Json(paged(items, page, per, total)));
    }
    let (items, total) = s
        .hosting()
        .list(q.customer_id, status, limit, offset)
        .await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/services/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_service(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let svc = s.hosting().get(id).await?.ok_or(AppError::NotFound)?;
    service_json(&s, svc).await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceAction {
    /// activate | suspend | unsuspend | terminate | cancel | fraud
    pub action: String,
    #[serde(default)]
    pub reason: String,
}

#[utoipa::path(post, path = "/api/v1/admin/services/{id}/action", request_body = ServiceAction, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_service_action(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ServiceAction>,
) -> ApiResult<Json<Value>> {
    // Pre-event: addons may veto the transition or rewrite the reason.
    let pre = crate::addon_ext::pre_event(
        &s,
        "service.before_action",
        json!({ "service_id": id, "action": b.action, "reason": b.reason, "by": u.user_id }),
    )
    .await?;
    let reason = pre
        .get("reason")
        .and_then(Value::as_str)
        .filter(|r| r.len() <= 500)
        .map(str::to_string)
        .unwrap_or_else(|| b.reason.clone());
    let b = ServiceAction {
        action: b.action,
        reason,
    };
    let (from, svc) = s.hosting().transition(id, &b.action, &b.reason).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.transition",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "from": from, "to": svc.status, "reason": b.reason }),
    )
    .await?;
    crate::provision::on_transition(&s, id, &from, &svc.status).await;
    let ev = match (from.as_str(), svc.status.as_str()) {
        (_, "suspended") => Some("service.suspended"),
        ("suspended", "active") => Some("service.unsuspended"),
        (_, "terminated") => Some("service.terminated"),
        (_, "cancelled") => Some("service.cancelled"),
        _ => None,
    };
    if svc.status == "active" && from != "active" && from != "suspended" {
        s.emit(DomainEvent::new(
            "service.activated",
            json!({ "service_id": id, "from": from, "reason": b.reason }),
        ));
    }
    if let Some(ev) = ev {
        s.emit(DomainEvent::new(
            ev,
            json!({ "service_id": id, "from": from, "reason": b.reason, "provisioned": true }),
        ));
    }
    Ok(Json(json!(svc)))
}

#[utoipa::path(get, path = "/api/v1/admin/jobs", params(ListQuery), responses((status = 200), (status = 403)))]
pub async fn admin_jobs(
    State(s): State<AppState>,
    _p: RequirePermission<perms::JobsManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let st = status_filter(&q, &["queued", "running", "succeeded", "failed", "dead"])?;
    let (items, total) = jobs::list(&s.pool, st, limit, offset).await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/admin/jobs/{id}/retry", responses((status = 200), (status = 409)))]
pub async fn admin_retry_job(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let j = jobs::retry(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "job.retried",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "kind": j.kind }),
    )
    .await?;
    Ok(Json(json!(j)))
}

/// Enqueue a recurring-billing run now (deduplicated per minute).
#[utoipa::path(post, path = "/api/v1/admin/jobs/recurring", responses((status = 200), (status = 403)))]
pub async fn admin_run_recurring(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::JobsManage>,
) -> ApiResult<Json<Value>> {
    let now = time::OffsetDateTime::now_utc();
    let key = format!("recurring-manual:{}", now.unix_timestamp() / 60);
    let id = jobs::enqueue(
        &s.pool,
        crate::worker::RECURRING,
        json!({ "manual": true }),
        None,
        Some(&key),
        5,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "job.enqueued",
        None,
        u.ip.as_deref(),
        json!({ "kind": crate::worker::RECURRING, "id": id }),
    )
    .await?;
    Ok(Json(json!({ "id": id, "duplicate": id.is_none() })))
}

#[utoipa::path(get, path = "/api/v1/client/dashboard", responses((status = 200, body = pelty_database::dashboard::ClientDashboard), (status = 401)))]
pub async fn client_dashboard(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let d = serde_json::to_value(pelty_database::dashboard::client(&s.pool, cid).await?)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    if !crate::addon_filters::has(&s, "client.dashboard") {
        return Ok(Json(d));
    }
    Ok(Json(
        crate::addon_filters::apply(&s, "client.dashboard", d, json!({ "customer_id": cid })).await,
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/dashboard", responses((status = 200, body = pelty_database::dashboard::AdminDashboard), (status = 403)))]
pub async fn admin_dashboard(
    State(s): State<AppState>,
    _p: RequirePermission<perms::InvoicesManage>,
) -> ApiResult<Json<pelty_database::dashboard::AdminDashboard>> {
    Ok(Json(pelty_database::dashboard::admin(&s.pool).await?))
}
