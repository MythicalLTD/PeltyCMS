//! Invoice merge / split / duplicate / notes (staff), client mass-pay merge,
//! and gift cards (staff issue, client redeem).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, gift_cards, invoice_ops};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/invoices/merge", post(admin_merge))
        .route(
            "/admin/customers/{id}/mergeable-invoices",
            get(admin_mergeable),
        )
        .route("/admin/invoices/{id}/split", post(admin_split))
        .route("/admin/invoices/{id}/duplicate", post(admin_duplicate))
        .route(
            "/admin/invoices/{id}/notes",
            get(admin_notes).put(admin_set_notes),
        )
        .route("/client/invoices/merge", post(client_merge))
        .route("/admin/gift-cards", get(gift_list).post(gift_create))
        .route("/admin/gift-cards/{id}/disable", post(gift_disable))
        .route("/client/gift-cards/redeem", post(gift_redeem))
        .route("/admin/transactions", get(admin_transactions))
        .route(
            "/admin/transactions/{id}",
            axum::routing::patch(admin_update_transaction),
        )
}

type Im = RequirePermission<perms::InvoicesManage>;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeBody {
    pub invoice_ids: Vec<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/merge", request_body = MergeBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_merge(
    State(s): State<AppState>,
    RequirePermission(u, _): Im,
    Json(b): Json<MergeBody>,
) -> ApiResult<Json<Value>> {
    let inv = invoice_ops::merge(&s.pool, &b.invoice_ids).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.merged",
        Some(&inv.id.to_string()),
        u.ip.as_deref(),
        json!({ "sources": b.invoice_ids }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "invoice.merged",
        json!({ "invoice_id": inv.id, "sources": b.invoice_ids, "by_staff": true }),
    ));
    Ok(Json(json!({ "invoice": inv })))
}

/// Client-side merge (WHMCS "mass payment"): combine own unpaid invoices into
/// one so a single gateway payment (and one affiliate commission) covers all.
#[utoipa::path(post, path = "/api/v1/client/invoices/merge", request_body = MergeBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn client_merge(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<MergeBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    for id in &b.invoice_ids {
        if s.invoices().get_owned(*id, cid).await?.is_none() {
            return Err(AppError::NotFound.into());
        }
    }
    let inv = invoice_ops::merge(&s.pool, &b.invoice_ids).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.merged",
        Some(&inv.id.to_string()),
        u.ip.as_deref(),
        json!({ "sources": b.invoice_ids, "by": "client" }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "invoice.merged",
        json!({ "invoice_id": inv.id, "sources": b.invoice_ids, "by_staff": false }),
    ));
    Ok(Json(json!({ "invoice": inv })))
}

#[utoipa::path(get, path = "/api/v1/admin/customers/{id}/mergeable-invoices", responses((status = 200), (status = 403)))]
pub async fn admin_mergeable(
    State(s): State<AppState>,
    _p: Im,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let rows = invoice_ops::list_mergeable(&s.pool, id).await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(id, number, currency, total, due)| json!({ "id": id, "number": number, "currency": currency, "total": total, "due_at": due.and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok()) }))
        .collect();
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitBody {
    pub item_ids: Vec<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/split", request_body = SplitBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_split(
    State(s): State<AppState>,
    RequirePermission(u, _): Im,
    Path(id): Path<Uuid>,
    Json(b): Json<SplitBody>,
) -> ApiResult<Json<Value>> {
    let (a, c) = invoice_ops::split(&s.pool, id, &b.item_ids).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.split",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "into": [a.id, c.id] }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "invoice.split",
        json!({ "invoice_id": id, "into": [a.id, c.id] }),
    ));
    Ok(Json(json!({ "invoices": [a, c] })))
}

#[utoipa::path(post, path = "/api/v1/admin/invoices/{id}/duplicate", responses((status = 200), (status = 404), (status = 409)))]
pub async fn admin_duplicate(
    State(s): State<AppState>,
    RequirePermission(u, _): Im,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let n = invoice_ops::duplicate(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.duplicated",
        Some(&n.to_string()),
        u.ip.as_deref(),
        json!({ "from": id }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "invoice.duplicated",
        json!({ "invoice_id": n, "from": id }),
    ));
    Ok(Json(json!({ "id": n })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotesBody {
    #[serde(default)]
    pub admin_notes: String,
    #[serde(default)]
    pub client_notes: String,
}

#[utoipa::path(get, path = "/api/v1/admin/invoices/{id}/notes", responses((status = 200), (status = 404)))]
pub async fn admin_notes(
    State(s): State<AppState>,
    _p: Im,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let (a, c, m, sp) = invoice_ops::notes(&s.pool, id).await?;
    Ok(Json(
        json!({ "admin_notes": a, "client_notes": c, "merged_into": m, "split_from": sp }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/invoices/{id}/notes", request_body = NotesBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_set_notes(
    State(s): State<AppState>,
    RequirePermission(u, _): Im,
    Path(id): Path<Uuid>,
    Json(b): Json<NotesBody>,
) -> ApiResult<Json<Value>> {
    invoice_ops::set_notes(&s.pool, id, &b.admin_notes, &b.client_notes).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "invoice.notes_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/gift-cards", responses((status = 200), (status = 403)))]
pub async fn gift_list(State(s): State<AppState>, _p: Im) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": gift_cards::list(&s.pool, 500).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GiftBody {
    #[serde(default = "one")]
    pub count: u32,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    #[serde(default)]
    pub note: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<OffsetDateTime>,
}
fn one() -> u32 {
    1
}

#[utoipa::path(post, path = "/api/v1/admin/gift-cards", request_body = GiftBody, responses((status = 200), (status = 422)))]
pub async fn gift_create(
    State(s): State<AppState>,
    RequirePermission(u, _): Im,
    Json(b): Json<GiftBody>,
) -> ApiResult<Json<Value>> {
    let made = gift_cards::create(
        &s.pool,
        b.count,
        &b.currency,
        b.amount,
        &b.note,
        b.expires_at,
        Some(u.user_id),
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gift_card.created",
        None,
        u.ip.as_deref(),
        json!({ "count": made.len(), "currency": b.currency, "amount": b.amount.to_string() }),
    )
    .await?;
    let items: Vec<Value> = made
        .into_iter()
        .map(|(c, code)| json!({ "card": c, "code": code }))
        .collect();
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(post, path = "/api/v1/admin/gift-cards/{id}/disable", responses((status = 200), (status = 409)))]
pub async fn gift_disable(
    State(s): State<AppState>,
    RequirePermission(u, _): Im,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    gift_cards::disable(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gift_card.disabled",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RedeemBody {
    pub code: String,
}

#[utoipa::path(post, path = "/api/v1/client/gift-cards/redeem", request_body = RedeemBody, responses((status = 200), (status = 422)))]
pub async fn gift_redeem(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<RedeemBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let (currency, amount) = gift_cards::redeem(&s.pool, cid, &b.code).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "gift_card.redeemed",
        Some(&cid.to_string()),
        u.ip.as_deref(),
        json!({ "currency": currency, "amount": amount.to_string() }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "gift_card.redeemed",
        json!({ "customer_id": cid, "currency": currency, "amount": amount }),
    ));
    Ok(Json(json!({ "currency": currency, "amount": amount })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TxQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub gateway: Option<String>,
    pub status: Option<String>,
    pub currency: Option<String>,
    pub customer_id: Option<Uuid>,
    /// YYYY-MM-DD inclusive
    pub from: Option<String>,
    /// YYYY-MM-DD inclusive
    pub to: Option<String>,
    pub q: Option<String>,
}

fn day(v: &Option<String>, plus: i64) -> ApiResult<Option<OffsetDateTime>> {
    let Some(v) = v.as_deref().filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let d = time::Date::parse(v, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|_| AppError::Validation("error.invalid_date"))?;
    Ok(Some(
        (d + time::Duration::days(plus)).midnight().assume_utc(),
    ))
}

const TX_STATUSES: &[&str] = &["pending", "succeeded", "failed", "refunded"];

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TxNote {
    pub note: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/admin/transactions/{id}", request_body = TxNote, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update_transaction(
    State(s): State<AppState>,
    p: Im,
    Path(id): Path<Uuid>,
    Json(b): Json<TxNote>,
) -> ApiResult<Json<Value>> {
    pelty_database::transactions::set_note(&s.pool, id, b.note.as_deref()).await?;
    audit::record(
        &s.pool,
        Some(p.0.user_id),
        "transaction.updated",
        Some(&id.to_string()),
        p.0.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/transactions", params(TxQuery), responses((status = 200), (status = 403), (status = 422)))]
pub async fn admin_transactions(
    State(s): State<AppState>,
    _p: Im,
    axum::extract::Query(q): axum::extract::Query<TxQuery>,
) -> ApiResult<Json<Value>> {
    let per = q.per_page.unwrap_or(50).clamp(1, 200);
    let page = q.page.unwrap_or(1).max(1);
    let nz = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let status = nz(&q.status);
    if let Some(st) = &status {
        if !TX_STATUSES.contains(&st.as_str()) {
            return Err(AppError::Validation("error.invalid_status").into());
        }
    }
    let f = pelty_database::transactions::Filter {
        gateway: nz(&q.gateway),
        status,
        currency: nz(&q.currency).map(|c| c.to_uppercase()),
        customer: q.customer_id,
        from: day(&q.from, 0)?,
        to: day(&q.to, 1)?,
        q: nz(&q.q).map(|v| v.chars().take(100).collect()),
    };
    let (items, total, totals) =
        pelty_database::transactions::list(&s.pool, &f, per, (page - 1) * per).await?;
    Ok(Json(
        json!({ "items": items, "page": page, "per_page": per, "total": total, "totals": totals }),
    ))
}
