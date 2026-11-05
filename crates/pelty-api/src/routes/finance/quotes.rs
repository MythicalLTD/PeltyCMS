//! Quotes / proposals: admin drafts and sends (`quotes.manage`); the customer
//! views, accepts (→ finalized invoice) or declines.
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::ListQuery;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::quotes::{self, QuoteInput};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/quotes", get(admin_list).post(admin_create))
        .route(
            "/admin/quotes/{id}",
            get(admin_get).put(admin_update).delete(admin_delete),
        )
        .route("/admin/quotes/{id}/send", post(admin_send))
        .route("/client/quotes", get(client_list))
        .route("/client/quotes/{id}", get(client_get))
        .route("/client/quotes/{id}/accept", post(client_accept))
        .route("/client/quotes/{id}/decline", post(client_decline))
}

async fn log(s: &AppState, u: &CurrentUser, action: &str, id: Uuid) -> ApiResult<()> {
    audit::record(
        &s.pool,
        Some(u.user_id),
        action,
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(())
}

/// Mail the customer's login (or billing e-mail) a link to the quote.
async fn notify_customer(s: &AppState, id: Uuid) -> ApiResult<()> {
    let q = quotes::get(&s.pool, id, None)
        .await?
        .ok_or(AppError::NotFound)?;
    let c = s.clients().get(q.customer_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let owner = match c.owner_user_id {
        Some(uid) => pelty_database::users::find(&s.pool, uid).await?,
        None => None,
    };
    let (to, locale, name) = match owner {
        Some(u) => (
            u.email.clone(),
            crate::mail::user_locale(s, &u),
            if u.display_name.is_empty() {
                u.email.clone()
            } else {
                u.display_name.clone()
            },
        ),
        None => (
            c.email.clone(),
            s.cfg.default_locale.clone(),
            c.first_name.clone(),
        ),
    };
    let link = format!("{}/quotes/{}", s.cfg.public_url, q.id);
    let total = format!("{} {}", q.total, q.currency);
    let valid = q.valid_until.to_string();
    crate::mail::enqueue(
        s,
        "quote_sent",
        &locale,
        &to,
        crate::mail::vars(&[
            ("name", &name),
            ("quote_number", &q.number),
            ("subject", &q.subject),
            ("total", &total),
            ("valid_until", &valid),
            ("link", &link),
        ]),
    )
    .await?;
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/admin/quotes", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::QuotesManage>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let (lim, off, page, per) = q.bounds();
    let (items, total) = quotes::list(&s.pool, None, lim, off).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/quotes/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::QuotesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(quotes::get(&s.pool, id, None)
        .await?
        .ok_or(AppError::NotFound)?)))
}

#[utoipa::path(post, path = "/api/v1/admin/quotes", request_body = QuoteInput, responses((status = 200), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::QuotesManage>,
    Json(b): Json<QuoteInput>,
) -> ApiResult<Json<Value>> {
    let id = quotes::save(&s.pool, None, &b).await?;
    log(&s, &u, "quote.created", id).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("quote.created", json!({ "quote_id": id, "by": u.user_id })));
    Ok(Json(json!(quotes::get(&s.pool, id, None).await?)))
}

#[utoipa::path(put, path = "/api/v1/admin/quotes/{id}", request_body = QuoteInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::QuotesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<QuoteInput>,
) -> ApiResult<Json<Value>> {
    quotes::save(&s.pool, Some(id), &b).await?;
    log(&s, &u, "quote.updated", id).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("quote.updated", json!({ "quote_id": id, "by": u.user_id })));
    Ok(Json(json!(quotes::get(&s.pool, id, None).await?)))
}

#[utoipa::path(delete, path = "/api/v1/admin/quotes/{id}", responses((status = 200), (status = 409)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::QuotesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    quotes::delete(&s.pool, id).await?;
    log(&s, &u, "quote.deleted", id).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("quote.deleted", json!({ "quote_id": id, "by": u.user_id })));
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/quotes/{id}/send", responses((status = 200), (status = 409)))]
pub async fn admin_send(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::QuotesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    quotes::send(&s.pool, id).await?;
    log(&s, &u, "quote.sent", id).await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "quote.sent",
        json!({ "quote_id": id }),
    ));
    notify_customer(&s, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/client/quotes", responses((status = 200), (status = 401)))]
pub async fn client_list(
    State(s): State<AppState>,
    u: CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let (lim, off, page, per) = q.bounds();
    let (items, total) = quotes::list(&s.pool, Some(cid), lim, off).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/client/quotes/{id}", responses((status = 200), (status = 404)))]
pub async fn client_get(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    Ok(Json(json!(quotes::get(&s.pool, id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?)))
}

#[utoipa::path(post, path = "/api/v1/client/quotes/{id}/accept", responses((status = 200), (status = 404), (status = 409)))]
pub async fn client_accept(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    // Cancel-only pre-event: an addon may block acceptance (e.g. pending approval).
    crate::addon_ext::pre_event(&s, "quote.before_accept", json!({ "quote_id": id, "customer_id": cid })).await?;
    let inv = quotes::decide(&s.pool, id, cid, true).await?;
    log(&s, &u, "quote.accepted", id).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("quote.accepted", json!({ "quote_id": id, "customer_id": cid, "invoice_id": inv })));
    Ok(Json(json!({ "invoice_id": inv })))
}

#[utoipa::path(post, path = "/api/v1/client/quotes/{id}/decline", responses((status = 200), (status = 404), (status = 409)))]
pub async fn client_decline(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    quotes::decide(&s.pool, id, cid, false).await?;
    log(&s, &u, "quote.declined", id).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("quote.declined", json!({ "quote_id": id, "customer_id": cid })));
    Ok(Json(json!({ "ok": true })))
}
