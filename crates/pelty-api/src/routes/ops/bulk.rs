//! Bulk invoice actions, client mass-pay (from account credit), pre-due
//! reminder settings, and the order approval queue + rules.
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::invoice_tools::{self, DueReminderSettings};
use pelty_database::order_approval::{self, ApprovalRules};
use pelty_database::{audit, credit, invoices};
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

const MAX_IDS: usize = 100;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/invoices/bulk", post(invoices_bulk))
        .route("/client/invoices/mass-pay", post(mass_pay))
        .route("/admin/settings/due-reminders", get(due_get).put(due_put))
        .route("/admin/orders/approval-queue", get(queue))
        .route("/admin/orders/bulk", post(orders_bulk))
        .route("/admin/settings/order-approval", get(rules_get).put(rules_put))
}

fn ids_ok(ids: &[Uuid]) -> Result<Vec<Uuid>, AppError> {
    let mut v = ids.to_vec();
    v.sort();
    v.dedup();
    if v.is_empty() || v.len() > MAX_IDS {
        return Err(AppError::Validation("error.bulk_size"));
    }
    Ok(v)
}

fn res(id: Uuid, r: Result<Value, AppError>) -> Value {
    match r {
        Ok(v) => json!({ "id": id, "ok": true, "result": v }),
        Err(e) => json!({ "id": id, "ok": false, "error": e.code() }),
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InvoicesBulk {
    pub ids: Vec<Uuid>,
    /// `mark_paid` | `void` | `remind`
    pub action: String,
    /// Required for `mark_paid`: replays with the same key change nothing.
    pub idempotency_key: Option<String>,
    pub gateway: Option<String>,
    pub reference: Option<String>,
}

async fn paid_events(s: &AppState, id: Uuid, a: &invoices::Applied, gateway: &str) {
    s.emit(DomainEvent::new("payment.succeeded", json!({ "payment_id": a.payment_id, "invoice_id": id, "gateway": gateway })));
    if a.invoice_paid {
        s.emit(DomainEvent::new("invoice.paid", json!({ "invoice_id": id })));
        crate::changes::after_payment(s, id).await;
    }
}

/// Apply one action to many invoices; returns a per-invoice result.
#[utoipa::path(post, path = "/api/v1/admin/invoices/bulk", request_body = InvoicesBulk, responses((status = 200), (status = 403), (status = 422)))]
pub async fn invoices_bulk(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::InvoicesManage>, Json(b): Json<InvoicesBulk>) -> ApiResult<Json<Value>> {
    let ids = ids_ok(&b.ids)?;
    let key = b.idempotency_key.as_deref().map(str::trim).unwrap_or("");
    let gateway = b.gateway.as_deref().map(str::trim).filter(|g| !g.is_empty()).unwrap_or("manual").to_string();
    match b.action.as_str() {
        "mark_paid" if key.is_empty() || key.len() > 120 || gateway.len() > 40 || b.reference.as_ref().is_some_and(|r| r.len() > 200) => {
            return Err(AppError::Validation("error.validation").into())
        }
        "mark_paid" | "void" | "remind" => {}
        _ => return Err(AppError::Validation("error.invalid_action").into()),
    }
    let info = invoice_tools::balances(&s.pool, &ids).await?;
    let mut out = Vec::with_capacity(ids.len());
    let mut ok = 0usize;
    for id in ids {
        let Some((_, status, balance, customer)) = info.iter().find(|r| r.0 == id).cloned() else {
            out.push(res(id, Err(AppError::NotFound)));
            continue;
        };
        let r: Result<Value, AppError> = match b.action.as_str() {
            "mark_paid" => {
                if status != "unpaid" || balance <= rust_decimal::Decimal::ZERO {
                    Err(AppError::Conflict("error.invoice_not_unpaid"))
                } else {
                    match s.invoices().record_payment(id, &gateway, b.reference.as_deref(), balance, &format!("bulk:{id}:{key}"), Some(u.user_id)).await {
                        Ok(a) => {
                            if !a.duplicate {
                                audit::record(&s.pool, Some(u.user_id), "payment.recorded", Some(&id.to_string()), u.ip.as_deref(), json!({ "amount": balance, "payment_id": a.payment_id, "bulk": true })).await?;
                                paid_events(&s, id, &a, &gateway).await;
                            }
                            Ok(json!({ "amount": balance, "duplicate": a.duplicate }))
                        }
                        Err(e) => Err(e),
                    }
                }
            }
            "void" => match s.invoices().void(id).await {
                Ok(_) => {
                    audit::record(&s.pool, Some(u.user_id), "invoice.voided", Some(&id.to_string()), u.ip.as_deref(), json!({ "bulk": true })).await?;
                    s.emit(DomainEvent::new("invoice.voided", json!({ "invoice_id": id })));
                    Ok(json!({}))
                }
                Err(e) => Err(e),
            },
            _ => remind(&s, id, &status, balance, customer).await,
        };
        ok += usize::from(r.is_ok());
        out.push(res(id, r));
    }
    audit::record(&s.pool, Some(u.user_id), "invoice.bulk", None, u.ip.as_deref(), json!({ "action": b.action, "count": out.len(), "ok": ok })).await?;
    Ok(Json(json!({ "ok": ok, "failed": out.len() - ok, "results": out })))
}

async fn remind(s: &AppState, id: Uuid, status: &str, balance: rust_decimal::Decimal, customer: Uuid) -> Result<Value, AppError> {
    if status != "unpaid" || balance <= rust_decimal::Decimal::ZERO {
        return Err(AppError::Conflict("error.invoice_not_unpaid"));
    }
    let inv = s.invoices().get(id).await?.ok_or(AppError::NotFound)?;
    let (Some(number), Some(due)) = (inv.number.clone(), inv.due_at) else {
        return Err(AppError::Conflict("error.invoice_not_finalized"));
    };
    let today = time::OffsetDateTime::now_utc().date();
    let m = invoice_tools::DueSoon { invoice_id: id, customer_id: customer, number, balance, currency: inv.currency.clone(), due_date: due.date(), days_left: (due.date() - today).whole_days() as i32 };
    crate::approval::mail_due(s, &m).await?;
    Ok(json!({ "days_left": m.days_left }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MassPay {
    pub ids: Vec<Uuid>,
}

/// Pay several of my unpaid invoices from account credit, oldest due first.
#[utoipa::path(post, path = "/api/v1/client/invoices/mass-pay", request_body = MassPay, responses((status = 200), (status = 401), (status = 422)))]
pub async fn mass_pay(State(s): State<AppState>, u: CurrentUser, Json(b): Json<MassPay>) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let ids = ids_ok(&b.ids)?;
    let mut mine: Vec<(Uuid, Option<time::OffsetDateTime>)> = vec![];
    for id in &ids {
        if let Some(inv) = s.invoices().get(*id).await?.filter(|i| i.customer_id == cid) {
            mine.push((inv.id, inv.due_at));
        }
    }
    mine.sort_by_key(|(_, d)| *d);
    let mut out = vec![];
    let mut paid = 0usize;
    for id in ids.iter().filter(|i| !mine.iter().any(|m| m.0 == **i)) {
        out.push(res(*id, Err(AppError::NotFound)));
    }
    for (id, _) in mine {
        let r = match credit::apply_to_invoice(&s.pool, cid, id, None, Some(u.user_id)).await {
            Ok(c) => {
                audit::record(&s.pool, Some(u.user_id), "credit.applied", Some(&id.to_string()), u.ip.as_deref(), json!({ "amount": c.amount, "payment_id": c.applied.payment_id, "mass_pay": true })).await?;
                paid_events(&s, id, &c.applied, credit::GATEWAY).await;
                paid += usize::from(c.applied.invoice_paid);
                Ok(json!({ "amount": c.amount, "paid": c.applied.invoice_paid }))
            }
            Err(e) => Err(e),
        };
        out.push(res(id, r));
    }
    Ok(Json(json!({ "paid": paid, "results": out })))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/due-reminders", responses((status = 200), (status = 403)))]
pub async fn due_get(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<DueReminderSettings>> {
    Ok(Json(invoice_tools::due_settings(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/due-reminders", request_body = DueReminderSettings, responses((status = 200), (status = 422)))]
pub async fn due_put(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<DueReminderSettings>) -> ApiResult<Json<DueReminderSettings>> {
    let v = invoice_tools::set_due_settings(&s.pool, b).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.due_reminders", None, u.ip.as_deref(), json!(v)).await?;
    Ok(Json(v))
}

#[utoipa::path(get, path = "/api/v1/admin/orders/approval-queue", responses((status = 200), (status = 403)))]
pub async fn queue(State(s): State<AppState>, _p: RequirePermission<perms::OrdersManage>) -> ApiResult<Json<Value>> {
    let items = order_approval::queue(&s.pool).await?;
    Ok(Json(json!({ "items": items, "rules": order_approval::rules(&s.pool).await? })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrdersBulk {
    pub ids: Vec<Uuid>,
    /// `approve` | `cancel`
    pub action: String,
}

#[utoipa::path(post, path = "/api/v1/admin/orders/bulk", request_body = OrdersBulk, responses((status = 200), (status = 403), (status = 422)))]
pub async fn orders_bulk(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::OrdersManage>, Json(b): Json<OrdersBulk>) -> ApiResult<Json<Value>> {
    let ids = ids_ok(&b.ids)?;
    if !matches!(b.action.as_str(), "approve" | "cancel") {
        return Err(AppError::Validation("error.invalid_action").into());
    }
    let mut out = vec![];
    let mut ok = 0usize;
    for id in ids {
        let r = if b.action == "approve" {
            crate::approval::activate(&s, id, Some(u.user_id), u.ip.as_deref(), false).await.map(|ids| json!({ "services": ids }))
        } else {
            match s.orders().cancel(id, false).await {
                Ok(_) => {
                    audit::record(&s.pool, Some(u.user_id), "order.cancelled", Some(&id.to_string()), u.ip.as_deref(), json!({ "bulk": true })).await?;
                    s.emit(DomainEvent::new("order.cancelled", json!({ "order_id": id })));
                    Ok(json!({}))
                }
                Err(e) => Err(e),
            }
        };
        ok += usize::from(r.is_ok());
        out.push(res(id, r));
    }
    Ok(Json(json!({ "ok": ok, "failed": out.len() - ok, "results": out })))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/order-approval", responses((status = 200), (status = 403)))]
pub async fn rules_get(State(s): State<AppState>, _p: RequirePermission<perms::OrdersManage>) -> ApiResult<Json<ApprovalRules>> {
    Ok(Json(order_approval::rules(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/order-approval", request_body = ApprovalRules, responses((status = 200), (status = 422)))]
pub async fn rules_put(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<ApprovalRules>) -> ApiResult<Json<ApprovalRules>> {
    let v = order_approval::set_rules(&s.pool, b).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.order_approval", None, u.ip.as_deref(), json!(v)).await?;
    Ok(Json(v))
}
