//! Provisioning admin API: modules, panel servers (keys write-only), product
//! module binding and per-service module actions / status.
use crate::error::{ApiError, ApiResult};
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::provisioning::{self as db, ServerInput};
use pelty_provisioning::ServerConn;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/provisioning/modules", get(modules))
        .route("/admin/servers", get(list).post(create))
        .route("/admin/servers/{id}", put(update).delete(remove))
        .route("/admin/servers/{id}/test", post(test))
        .route("/admin/servers/{id}/accounts", get(accounts))
        .route("/admin/servers/{id}/import", post(import))
        .route("/admin/products/{id}/module", get(product_get).put(product_set))
        .route("/admin/services/{id}/provisioning", get(service_state))
        .route("/admin/services/{id}/module", post(service_action))
        .route("/admin/server-groups", get(groups_list).post(groups_create))
        .route("/admin/server-groups/{id}", put(groups_update).delete(groups_delete))
        .route("/admin/server-groups/{id}/servers", get(groups_members).put(groups_set_members))
        .route("/admin/products/{id}/server-group", get(product_group_get).put(product_group_set))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct GroupInput {
    name: String,
    #[serde(default = "default_strategy")]
    strategy: String,
}
fn default_strategy() -> String {
    "least_services".into()
}
fn check_group(b: &GroupInput) -> ApiResult<()> {
    let n = b.name.trim();
    if n.is_empty() || n.chars().count() > 120 || !db::GROUP_STRATEGIES.contains(&b.strategy.as_str()) {
        return Err(AppError::Validation("error.invalid_server_group").into());
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/admin/server-groups", responses((status = 200), (status = 403)))]
pub async fn groups_list(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": db::list_groups(&s.pool).await?, "strategies": db::GROUP_STRATEGIES })))
}
#[utoipa::path(post, path = "/api/v1/admin/server-groups", responses((status = 200), (status = 422)))]
pub async fn groups_create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Json(b): Json<GroupInput>) -> ApiResult<Json<Value>> {
    check_group(&b)?;
    let id = db::upsert_group(&s.pool, None, b.name.trim(), &b.strategy).await?;
    audit::record(&s.pool, Some(u.user_id), "server_group.created", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name.trim(), "strategy": b.strategy })).await?;
    Ok(Json(json!({ "id": id })))
}
#[utoipa::path(put, path = "/api/v1/admin/server-groups/{id}", responses((status = 200), (status = 404), (status = 422)))]
pub async fn groups_update(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<GroupInput>) -> ApiResult<Json<Value>> {
    check_group(&b)?;
    db::upsert_group(&s.pool, Some(id), b.name.trim(), &b.strategy).await?;
    audit::record(&s.pool, Some(u.user_id), "server_group.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "strategy": b.strategy })).await?;
    Ok(Json(json!({ "id": id })))
}
#[utoipa::path(delete, path = "/api/v1/admin/server-groups/{id}", responses((status = 204), (status = 404)))]
pub async fn groups_delete(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<axum::http::StatusCode> {
    if !db::delete_group(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "server_group.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
#[derive(Deserialize, utoipa::ToSchema)]
pub struct Members {
    servers: Vec<Uuid>,
}
#[utoipa::path(get, path = "/api/v1/admin/server-groups/{id}/servers", responses((status = 200)))]
pub async fn groups_members(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "servers": db::group_servers(&s.pool, id).await? })))
}
#[utoipa::path(put, path = "/api/v1/admin/server-groups/{id}/servers", responses((status = 200), (status = 422)))]
pub async fn groups_set_members(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<Members>) -> ApiResult<Json<Value>> {
    if b.servers.len() > 500 {
        return Err(AppError::Validation("error.invalid_server_group").into());
    }
    db::set_group_servers(&s.pool, id, &b.servers).await?;
    audit::record(&s.pool, Some(u.user_id), "server_group.members", Some(&id.to_string()), u.ip.as_deref(), json!({ "count": b.servers.len() })).await?;
    Ok(Json(json!({ "servers": db::group_servers(&s.pool, id).await? })))
}
#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProductGroup {
    group_id: Option<Uuid>,
}
#[utoipa::path(get, path = "/api/v1/admin/products/{id}/server-group", responses((status = 200)))]
pub async fn product_group_get(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "group_id": db::product_group(&s.pool, id).await? })))
}
#[utoipa::path(put, path = "/api/v1/admin/products/{id}/server-group", responses((status = 200), (status = 404)))]
pub async fn product_group_set(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<ProductGroup>) -> ApiResult<Json<Value>> {
    if !db::set_product_group(&s.pool, id, b.group_id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(&s.pool, Some(u.user_id), "product.server_group", Some(&id.to_string()), u.ip.as_deref(), json!({ "group_id": b.group_id })).await?;
    Ok(Json(json!({ "group_id": b.group_id })))
}

async fn known_module(s: &AppState, m: &str) -> ApiResult<()> {
    if s.provisioning.get(m).is_none() {
        if crate::addons::provided(s, "provisioning", m) {
            return Err(AppError::Validation("error.addon_not_installed").into());
        }
        return Err(AppError::Validation("error.unknown_module").into());
    }
    if !crate::addons::provisioning_enabled(s, m).await? {
        return Err(AppError::Validation("error.addon_not_installed").into());
    }
    Ok(())
}

fn seal(s: &AppState, key: &str) -> ApiResult<Vec<u8>> {
    let k = key.trim();
    if k.is_empty() || k.len() > 1000 {
        return Err(AppError::Validation("error.invalid_server").into());
    }
    let b = s.secret_box.as_ref().ok_or(AppError::Conflict("error.encryption_not_configured"))?;
    Ok(b.seal(k.as_bytes()))
}

#[utoipa::path(get, path = "/api/v1/admin/provisioning/modules", responses((status = 200), (status = 403)))]
pub async fn modules(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": s.provisioning.list() })))
}

#[utoipa::path(get, path = "/api/v1/admin/servers", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": db::list_servers(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/servers", request_body = ServerInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Json(b): Json<ServerInput>) -> ApiResult<Json<Value>> {
    known_module(&s, &b.module).await?;
    let sealed = seal(&s, b.api_key.as_deref().unwrap_or_default())?;
    let v = db::create_server(&s.pool, &b, sealed).await?;
    audit::record(&s.pool, Some(u.user_id), "server.created", Some(&v.id.to_string()), u.ip.as_deref(), json!({ "name": v.name, "module": v.module })).await?;
    Ok(Json(json!(v)))
}

#[utoipa::path(put, path = "/api/v1/admin/servers/{id}", request_body = ServerInput, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<ServerInput>) -> ApiResult<Json<Value>> {
    known_module(&s, &b.module).await?;
    let sealed = match b.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => Some(seal(&s, k)?),
        None => None,
    };
    let rotated = sealed.is_some();
    let v = db::update_server(&s.pool, id, &b, sealed).await?;
    audit::record(&s.pool, Some(u.user_id), "server.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": v.name, "key_rotated": rotated })).await?;
    Ok(Json(json!(v)))
}

#[utoipa::path(delete, path = "/api/v1/admin/servers/{id}", responses((status = 200), (status = 404), (status = 409)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    db::delete_server(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "server.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Check the stored credentials against the panel. Returns `{ok, error?}`
/// with a stable error code; never echoes the key.
#[utoipa::path(post, path = "/api/v1/admin/servers/{id}/test", responses((status = 200), (status = 404)))]
pub async fn test(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let (module, base_url, sealed, config, _) = db::server_secret(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let m = s.provisioning.get(&module).ok_or(AppError::Validation("error.unknown_module"))?;
    let b = s.secret_box.as_ref().ok_or(AppError::Conflict("error.encryption_not_configured"))?;
    let api_key = b.open(&sealed).and_then(|k| String::from_utf8(k).ok()).ok_or(AppError::Conflict("error.server_config"))?;
    match m.test_connection(&ServerConn { base_url, api_key, config }).await {
        Ok(()) => {
            pelty_database::module_ext::record_check(&s.pool, id, true, "").await?;
            Ok(Json(json!({ "ok": true })))
        }
        Err(e) => {
            tracing::info!(server = %id, error = %e, "server connection test failed");
            pelty_database::module_ext::record_check(&s.pool, id, false, crate::provision::test_error_code(&e)).await?;
            Ok(Json(json!({ "ok": false, "error": crate::provision::test_error_code(&e) })))
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductModule {
    pub module: Option<String>,
    #[serde(default)]
    pub server_id: Option<Uuid>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub config: Option<Value>,
}

#[utoipa::path(get, path = "/api/v1/admin/products/{id}/module", responses((status = 200), (status = 404)))]
pub async fn product_get(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let (module, server_id, config) = db::product_module(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "module": module, "server_id": server_id, "config": config })))
}

#[utoipa::path(put, path = "/api/v1/admin/products/{id}/module", request_body = ProductModule, responses((status = 200), (status = 404), (status = 422)))]
pub async fn product_set(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<ProductModule>) -> ApiResult<Json<Value>> {
    let module = b.module.as_deref().filter(|m| !m.is_empty());
    if let Some(m) = module {
        known_module(&s, m).await?;
    }
    if let Some(sid) = b.server_id {
        let v = db::get_server(&s.pool, sid).await?.ok_or(AppError::Validation("error.invalid_server"))?;
        if Some(v.module.as_str()) != module {
            return Err(AppError::Validation("error.server_module_mismatch").into());
        }
    }
    let config = b.config.unwrap_or_else(|| json!({}));
    db::set_product_module(&s.pool, id, module, b.server_id, &config).await?;
    let declared = module.and_then(|m| s.provisioning.get(m)).map(|m| m.checkout_fields()).unwrap_or_default();
    let fields: Vec<db::ModuleField> = declared
        .iter()
        .map(|f| db::ModuleField { key: f.key, name: f.name, description: f.description, kind: f.kind, choices: f.choices.iter().map(|c| c.to_string()).collect(), pattern: f.pattern, required: f.required })
        .collect();
    db::sync_module_fields(&s.pool, id, &fields).await?;
    audit::record(&s.pool, Some(u.user_id), "product.module_set", Some(&id.to_string()), u.ip.as_deref(), json!({ "module": module, "server_id": b.server_id })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/services/{id}/provisioning", responses((status = 200), (status = 404)))]
pub async fn service_state(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let st = db::state(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let jobs = db::jobs_for(&s.pool, id).await?;
    Ok(Json(json!({ "state": st, "jobs": jobs })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleAction {
    pub action: String,
}

/// Manually (re)run a module action. Does not change the billing status.
#[utoipa::path(post, path = "/api/v1/admin/services/{id}/module", request_body = ModuleAction, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn service_action(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<ModuleAction>) -> ApiResult<Json<Value>> {
    if !pelty_provisioning::ACTIONS.contains(&b.action.as_str()) {
        return Err(AppError::Validation("error.invalid_action").into());
    }
    let job = db::enqueue(&s.pool, id, &b.action, &Uuid::new_v4().simple().to_string()).await?;
    let job = job.ok_or(ApiError::from(AppError::Conflict("error.no_module")))?;
    audit::record(&s.pool, Some(u.user_id), "service.module_action", Some(&id.to_string()), u.ip.as_deref(), json!({ "action": b.action, "job": job })).await?;
    Ok(Json(json!({ "job_id": job })))
}

async fn remote_accounts(s: &AppState, id: Uuid) -> ApiResult<(String, Vec<pelty_provisioning::RemoteAccount>)> {
    let (module, base_url, sealed, config, _) = db::server_secret(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let m = s.provisioning.get(&module).ok_or(AppError::Validation("error.unknown_module"))?;
    let b = s.secret_box.as_ref().ok_or(AppError::Conflict("error.encryption_not_configured"))?;
    let api_key = b.open(&sealed).and_then(|k| String::from_utf8(k).ok()).ok_or(AppError::Conflict("error.server_config"))?;
    let conn = ServerConn { base_url, api_key, config };
    match tokio::time::timeout(std::time::Duration::from_secs(30), m.list_accounts(&conn)).await {
        Ok(Ok(Some(a))) => Ok((module, a)),
        Ok(Ok(None)) => Err(AppError::Conflict("error.module_no_account_list").into()),
        Ok(Err(e)) => {
            tracing::info!(server = %id, error = %e, "list_accounts failed");
            Err(AppError::Conflict("error.module_unavailable").into())
        }
        Err(_) => Err(AppError::Conflict("error.module_unavailable").into()),
    }
}

/// Accounts on the panel, each with the service it is linked to (if any).
#[utoipa::path(get, path = "/api/v1/admin/servers/{id}/accounts", responses((status = 200), (status = 409)))]
pub async fn accounts(State(s): State<AppState>, _p: RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let (module, accts) = remote_accounts(&s, id).await?;
    let linked = pelty_database::server_import::linked(&s.pool, id).await?;
    let items: Vec<Value> = accts
        .iter()
        .map(|a| {
            let sid = linked.iter().find(|(r, _)| *r == a.remote_id).map(|(_, s)| *s);
            json!({ "remote_id": a.remote_id, "username": a.username, "domain": a.domain, "email": a.email, "package": a.package, "status": a.status, "linked_service_id": sid })
        })
        .collect();
    Ok(Json(json!({ "module": module, "items": items })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ImportBody {
    pub items: Vec<pelty_database::server_import::ImportItem>,
}

/// Create services for panel accounts (no provisioning call; status `ok`).
#[utoipa::path(post, path = "/api/v1/admin/servers/{id}/import", request_body = ImportBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn import(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServersManage>, Path(id): Path<Uuid>, Json(b): Json<ImportBody>) -> ApiResult<Json<Value>> {
    let (module, accts) = remote_accounts(&s, id).await?;
    let known: Vec<String> = accts.into_iter().map(|a| a.remote_id).collect();
    let res = pelty_database::server_import::import(&s.pool, id, &module, &known, &b.items).await?;
    let n = res.iter().filter(|r| r.service_id.is_some()).count();
    audit::record(&s.pool, Some(u.user_id), "server.accounts_imported", Some(&id.to_string()), u.ip.as_deref(), json!({ "imported": n, "requested": b.items.len() })).await?;
    Ok(Json(json!({ "imported": n, "results": res })))
}
