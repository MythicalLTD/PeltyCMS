//! Admin list mass actions (WHMCS "With selected…") for clients, services,
//! tickets and domains. Invoices and orders use `bulk.rs`. Each id is handled
//! on its own and reported back, so one bad row never blocks the rest.
use crate::error::{ApiError, ApiResult};
use crate::extract::{perms, CurrentUser, Permission, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, client_groups, mass};
use serde::Deserialize;
use serde_json::{json, Value};
use std::marker::PhantomData;
use uuid::Uuid;

pub const MAX_IDS: usize = 500;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/mass/customers", post(customers))
        .route("/admin/mass/services", post(services))
        .route("/admin/mass/tickets", post(tickets))
        .route("/admin/mass/domains", post(domains))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MassBody {
    pub ids: Vec<Uuid>,
    /// customers: set_status | set_group · services: suspend | unsuspend | terminate | cancel ·
    /// tickets: set_status | set_priority | assign · domains: auto_renew_on | auto_renew_off
    pub action: String,
    /// Status, priority, group id or staff id (null clears group / assignee).
    #[serde(default)]
    pub value: Value,
}

fn ids(b: &MassBody) -> Result<Vec<Uuid>, ApiError> {
    let mut v = b.ids.clone();
    v.sort();
    v.dedup();
    if v.is_empty() || v.len() > MAX_IDS {
        return Err(AppError::Validation("error.mass_ids").into());
    }
    Ok(v)
}

fn as_uuid(v: &Value) -> Result<Option<Uuid>, ApiError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => s
            .parse()
            .map(Some)
            .map_err(|_| AppError::Validation("error.validation").into()),
        _ => Err(AppError::Validation("error.validation").into()),
    }
}

fn err_code(e: &ApiError) -> String {
    e.0.code().to_string()
}

async fn finish(
    s: &AppState,
    u: &CurrentUser,
    entity: &str,
    action: &str,
    out: Vec<Value>,
) -> ApiResult<Json<Value>> {
    let ok = out.iter().filter(|r| r["ok"] == true).count();
    audit::record(
        &s.pool,
        Some(u.user_id),
        &format!("{entity}.mass"),
        None,
        u.ip.as_deref(),
        json!({ "action": action, "count": out.len(), "ok": ok }),
    )
    .await?;
    Ok(Json(
        json!({ "ok": ok, "failed": out.len() - ok, "results": out }),
    ))
}

fn row(id: Uuid, r: Result<(), ApiError>) -> Value {
    match r {
        Ok(()) => json!({ "id": id, "ok": true }),
        Err(e) => json!({ "id": id, "ok": false, "error": err_code(&e) }),
    }
}

fn perm<P: Permission>(u: &CurrentUser) -> RequirePermission<P> {
    RequirePermission(u.clone(), PhantomData)
}

#[utoipa::path(post, path = "/api/v1/admin/mass/customers", request_body = MassBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn customers(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomersManage>,
    Json(b): Json<MassBody>,
) -> ApiResult<Json<Value>> {
    let list = ids(&b)?;
    let mut out = Vec::new();
    match b.action.as_str() {
        "set_status" => {
            let st = b
                .value
                .as_str()
                .filter(|v| mass::CUSTOMER_STATUSES.contains(v))
                .ok_or(AppError::Validation("error.invalid_status"))?;
            for id in list {
                out.push(row(
                    id,
                    mass::customer_status(&s.pool, id, st)
                        .await
                        .map_err(Into::into),
                ));
            }
        }
        "set_group" => {
            let g = as_uuid(&b.value)?;
            for id in list {
                let r = client_groups::assign(&s.pool, id, g)
                    .await
                    .map_err(ApiError::from)
                    .and_then(|f| {
                        if f {
                            Ok(())
                        } else {
                            Err(AppError::NotFound.into())
                        }
                    });
                out.push(row(id, r));
            }
        }
        _ => return Err(AppError::Validation("error.invalid_action").into()),
    }
    finish(&s, &u, "customer", &b.action, out).await
}

#[utoipa::path(post, path = "/api/v1/admin/mass/services", request_body = MassBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn services(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Json(b): Json<MassBody>,
) -> ApiResult<Json<Value>> {
    let list = ids(&b)?;
    if !matches!(
        b.action.as_str(),
        "suspend" | "unsuspend" | "terminate" | "cancel"
    ) {
        return Err(AppError::Validation("error.invalid_action").into());
    }
    let reason = b
        .value
        .as_str()
        .unwrap_or("")
        .chars()
        .take(500)
        .collect::<String>();
    let mut out = Vec::new();
    for id in list {
        // Same path as the single-service action: transition, provisioning hook, event.
        let body = serde_json::from_value(json!({ "action": b.action, "reason": reason }))
            .map_err(|_| AppError::Validation("error.validation"))?;
        let r = crate::routes::billing::admin_service_action(
            State(s.clone()),
            perm(&u),
            Path(id),
            Json(body),
        )
        .await
        .map(|_| ());
        out.push(row(id, r));
    }
    finish(&s, &u, "service", &b.action, out).await
}

#[utoipa::path(post, path = "/api/v1/admin/mass/tickets", request_body = MassBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn tickets(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    Json(b): Json<MassBody>,
) -> ApiResult<Json<Value>> {
    let list = ids(&b)?;
    let patch = match b.action.as_str() {
        "set_status" => json!({ "status": b.value }),
        "set_priority" => json!({ "priority": b.value }),
        "assign" => json!({ "assigned_to": b.value }),
        _ => return Err(AppError::Validation("error.invalid_action").into()),
    };
    let mut out = Vec::new();
    for id in list {
        let body = serde_json::from_value(patch.clone())
            .map_err(|_| AppError::Validation("error.validation"))?;
        let r =
            crate::routes::tickets::admin_update(State(s.clone()), perm(&u), Path(id), Json(body))
                .await
                .map(|_| ());
        out.push(row(id, r));
    }
    finish(&s, &u, "ticket", &b.action, out).await
}

#[utoipa::path(post, path = "/api/v1/admin/mass/domains", request_body = MassBody, responses((status = 200), (status = 403), (status = 422)))]
pub async fn domains(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServicesManage>,
    Json(b): Json<MassBody>,
) -> ApiResult<Json<Value>> {
    let list = ids(&b)?;
    let on = match b.action.as_str() {
        "auto_renew_on" => true,
        "auto_renew_off" => false,
        _ => return Err(AppError::Validation("error.invalid_action").into()),
    };
    let mut out = Vec::new();
    for id in list {
        out.push(row(
            id,
            mass::domain_auto_renew(&s.pool, id, on)
                .await
                .map_err(Into::into),
        ));
    }
    finish(&s, &u, "domain", &b.action, out).await
}
