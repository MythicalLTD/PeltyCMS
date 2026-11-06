//! Module extras: server health, admin custom module buttons,
//! the client module panel, the module log, the failed-provisioning queue and
//! the public licensing verify endpoint.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::worker::JobError;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::module_ext as mx;
use pelty_database::{audit, jobs, provisioning as pdb, services};
use pelty_provisioning::licensing;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

/// Custom actions a customer may trigger from the client area (when the
/// module declares them). Everything else is admin-only.
pub const CLIENT_ACTIONS: &[&str] = &[
    "start",
    "stop",
    "reboot",
    "restart",
    "shutdown",
    "console",
    "change_password",
    "reset_password",
    "login",
    "reissue",
    "status",
    "download",
    "resend_validation",
];

/// Keys from module data shown to the customer.
const CLIENT_DATA: &[&str] = &[
    "hostname",
    "ip",
    "ipv4",
    "ipv6",
    "username",
    "domain",
    "license_key",
    "vmid",
    "node",
    "order_id",
    "status",
    "bound_domain",
    "bound_ip",
    "bound_directory",
    "expires_at",
];

const CALL_TIMEOUT: Duration = Duration::from_secs(25);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/servers/health", get(health))
        .route("/admin/servers/health/check", post(check_all))
        .route("/admin/services/{id}/module/custom", post(admin_custom))
        .route("/admin/services/{id}/module/usage", get(admin_usage))
        .route("/admin/module-log", get(log_list))
        .route("/admin/module-queue", get(queue))
        .route("/admin/module-queue/{id}/retry", post(queue_retry))
        .route("/client/services/{id}/module", get(client_panel))
        .route("/client/services/{id}/module/{action}", post(client_action))
        .route("/licensing/verify", post(license_verify))
}

fn job_err(e: JobError) -> AppError {
    let m = match e {
        JobError::Permanent(m) | JobError::Retry(m) => m,
    };
    tracing::info!(error = %m, "module load failed");
    AppError::Conflict("error.module_unavailable")
}

// ---- health -----------------------------------------------------------------

#[utoipa::path(get, path = "/api/v1/admin/servers/health", responses((status = 200)))]
pub async fn health(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    let rows = mx::health(&s.pool).await?;
    let data: Vec<Value> = rows
        .into_iter()
        .map(|h| {
            let usage = h
                .max_services
                .map(|m| ((h.active + h.suspended) as f64 / f64::from(m) * 100.0).round());
            json!({ "server": h, "usage_percent": usage })
        })
        .collect();
    Ok(Json(json!({ "data": data })))
}

/// Run the connection test on every enabled server and store the outcome.
pub async fn check_server(s: &AppState, id: Uuid) -> ApiResult<(bool, &'static str)> {
    let (module, base_url, sealed, config, _) = pdb::server_secret(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let res = match (
        s.provisioning.get(&module),
        s.secret_box
            .as_ref()
            .and_then(|b| b.open(&sealed))
            .and_then(|k| String::from_utf8(k).ok()),
    ) {
        (Some(m), Some(api_key)) => match tokio::time::timeout(
            CALL_TIMEOUT,
            m.test_connection(&pelty_provisioning::ServerConn {
                base_url,
                api_key,
                config,
            }),
        )
        .await
        {
            Ok(Ok(())) => (true, ""),
            Ok(Err(e)) => (false, crate::provision::test_error_code(&e)),
            Err(_) => (false, "error.server_unreachable"),
        },
        _ => (false, "error.server_config"),
    };
    mx::record_check(&s.pool, id, res.0, res.1).await?;
    Ok(res)
}

#[utoipa::path(post, path = "/api/v1/admin/servers/health/check", responses((status = 200)))]
pub async fn check_all(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    let mut out = vec![];
    for id in mx::server_ids(&s.pool).await? {
        let (ok, err) = check_server(&s, id).await?;
        out.push(json!({ "id": id, "ok": ok, "error": if ok { Value::Null } else { json!(err) } }));
    }
    Ok(Json(json!({ "data": out })))
}

// ---- custom actions ---------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomBody {
    pub action: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub params: Option<Value>,
}

/// Run a declared custom action, merge its `module_data_patch` and log it.
async fn run_custom(
    s: &AppState,
    sid: Uuid,
    action: &str,
    params: &Value,
    actor: Uuid,
    source: &str,
) -> ApiResult<Value> {
    if action.is_empty() || action.len() > 64 || params.to_string().len() > 8192 {
        return Err(AppError::Validation("error.invalid_module_action").into());
    }
    let l = crate::provision::load(s, sid).await.map_err(job_err)?;
    if !l.module.custom_actions().contains(&action) {
        return Err(AppError::Validation("error.invalid_module_action").into());
    }
    let started = std::time::Instant::now();
    let res = tokio::time::timeout(
        CALL_TIMEOUT,
        pelty_provisioning::run_custom(l.module.as_ref(), action, &l.conn, &l.ctx, params),
    )
    .await
    .unwrap_or_else(|_| {
        Err(pelty_provisioning::ProvisionError::Transient(
            "timed out".into(),
        ))
    });
    let ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let (ok, err) = match &res {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e.to_string()),
    };
    let empty = json!({});
    mx::log(
        &s.pool,
        mx::LogEntry {
            service: Some(sid),
            server: Some(l.server_id),
            module: &l.module_id,
            action,
            actor: Some(actor),
            source,
            ok,
            error: &err,
            output: res.as_ref().unwrap_or(&empty),
            duration_ms: ms,
        },
    )
    .await?;
    let mut out = match res {
        Ok(v) => v,
        Err(e) => {
            let code = match e {
                pelty_provisioning::ProvisionError::Config(_) => "error.server_config",
                pelty_provisioning::ProvisionError::Rejected { .. } => {
                    "error.module_action_rejected"
                }
                pelty_provisioning::ProvisionError::Transient(_) => "error.server_unreachable",
            };
            return Err(AppError::Conflict(code).into());
        }
    };
    if let Some(patch) = out
        .as_object_mut()
        .and_then(|o| o.remove("module_data_patch"))
    {
        mx::patch_module_data(&s.pool, sid, &crate::provision::seal_secrets(s, &patch)).await?;
    }
    Ok(out)
}

#[utoipa::path(post, path = "/api/v1/admin/services/{id}/module/custom", request_body = CustomBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_custom(
    State(s): State<AppState>,
    p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CustomBody>,
) -> ApiResult<Json<Value>> {
    let params = b.params.unwrap_or(json!({}));
    let out = run_custom(&s, id, &b.action, &params, p.0.user_id, "admin").await?;
    audit::record(
        &s.pool,
        Some(p.0.user_id),
        "service.module_custom",
        Some(&id.to_string()),
        p.0.ip.as_deref(),
        json!({ "action": b.action }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "result": out })))
}

pub(crate) async fn usage_of(s: &AppState, sid: Uuid) -> Option<Value> {
    let l = crate::provision::load(s, sid).await.ok()?;
    match tokio::time::timeout(CALL_TIMEOUT, l.module.usage(&l.conn, &l.ctx)).await {
        Ok(Ok(v)) => v,
        _ => None,
    }
}

#[utoipa::path(get, path = "/api/v1/admin/services/{id}/module/usage", responses((status = 200)))]
pub async fn admin_usage(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let t = pdb::target(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let actions = t
        .module
        .as_deref()
        .and_then(|m| s.provisioning.get(m))
        .map(|m| m.custom_actions().to_vec())
        .unwrap_or_default();
    Ok(Json(
        json!({ "module": t.module, "actions": actions, "usage": usage_of(&s, id).await }),
    ))
}

// ---- client panel -----------------------------------------------------------

fn client_actions(
    module: &dyn pelty_provisioning::ProvisioningModule,
    product_cfg: &Value,
) -> Vec<&'static str> {
    let allowed: Option<Vec<String>> = product_cfg
        .get("client_actions")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        });
    module
        .custom_actions()
        .iter()
        .copied()
        .filter(|a| CLIENT_ACTIONS.contains(a))
        .filter(|a| allowed.as_ref().is_none_or(|l| l.iter().any(|x| x == a)))
        .collect()
}

async fn owned(s: &AppState, u: &CurrentUser, id: Uuid) -> ApiResult<services::Service> {
    let c = customer_of(s, u).await?;
    Ok(s.hosting()
        .get(id)
        .await?
        .filter(|x| x.customer_id == c)
        .ok_or(AppError::NotFound)?)
}

#[utoipa::path(get, path = "/api/v1/client/services/{id}/module", responses((status = 200), (status = 404)))]
pub async fn client_panel(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let svc = owned(&s, &u, id).await?;
    let t = pdb::target(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let Some(m) = t.module.as_deref().and_then(|m| s.provisioning.get(m)) else {
        return Ok(Json(
            json!({ "module": null, "actions": [], "usage": null, "data": {} }),
        ));
    };
    let data: serde_json::Map<String, Value> = t
        .module_data
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| CLIENT_DATA.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut data = Value::Object(data);
    if t.module.as_deref() == Some("licensing") {
        data["license_key"] = json!(t.remote_id);
    }
    let active = svc.status == "active";
    let password = if active {
        crate::provision::open_secret(&s, &t.module_data, "initial_password")
    } else {
        None
    };
    let usage = if active { usage_of(&s, id).await } else { None };
    let actions = if active {
        client_actions(m.as_ref(), &t.product_config)
    } else {
        vec![]
    };
    // Module-declared client area (tabs + action meta), with optional product
    // config override under `client_area` (WHMCS-style per-product panels).
    let mut area = m.client_area();
    area.actions.retain(|a| actions.iter().any(|x| x == &a.id));
    if let Some(over) = t.product_config.get("client_area") {
        area = pelty_provisioning::parse_override(over, area);
        area.actions.retain(|a| actions.iter().any(|x| x == &a.id));
    }
    // Serialize action meta with owned strings so overrides can rename labels.
    let action_meta: Vec<Value> = area
        .actions
        .iter()
        .map(|a| {
            let label = t
                .product_config
                .pointer(&format!("/client_area/action_labels/{}", a.id))
                .and_then(|v| v.as_str())
                .unwrap_or(a.label);
            json!({
                "id": a.id,
                "label": label,
                "description": a.description,
                "danger": a.danger,
                "fields": a.fields.iter().map(|f| json!({
                    "key": f.key, "label": f.label, "kind": f.kind,
                    "required": f.required, "choices": f.choices,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({
        "module": t.module,
        "module_name": m.info().name,
        "status": svc.status,
        "provision_status": mx::provision_status(&s.pool, id).await?,
        "actions": actions,
        "action_meta": action_meta,
        "tabs": area.tabs,
        "sso": active,
        "usage": usage,
        "data": data,
        "initial_password": password,
    })))
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/module/{action}", request_body = CustomBody, responses((status = 200), (status = 404), (status = 409), (status = 422)))]
pub async fn client_action(
    State(s): State<AppState>,
    u: CurrentUser,
    Path((id, action)): Path<(Uuid, String)>,
    body: Option<Json<Value>>,
) -> ApiResult<Json<Value>> {
    let svc = owned(&s, &u, id).await?;
    if svc.status != "active" {
        return Err(AppError::Conflict("error.service_not_active").into());
    }
    if !s.auth_limiter.check(&format!("module:{}", u.user_id)) {
        return Err(AppError::RateLimited.into());
    }
    let t = pdb::target(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let m = t
        .module
        .as_deref()
        .and_then(|m| s.provisioning.get(m))
        .ok_or(AppError::Conflict("error.module_unavailable"))?;
    if !client_actions(m.as_ref(), &t.product_config).contains(&action.as_str()) {
        return Err(AppError::Validation("error.invalid_module_action").into());
    }
    // Customers may only pass fields declared on the action (password by
    // default for change_password / reset_password; others from action_meta).
    let allowed_keys: Vec<&str> = m
        .client_area()
        .actions
        .iter()
        .find(|a| a.id == action)
        .map(|a| a.fields.iter().map(|f| f.key).collect())
        .unwrap_or_else(|| {
            if matches!(action.as_str(), "change_password" | "reset_password") {
                vec!["password"]
            } else {
                vec![]
            }
        });
    let raw = body.map(|b| b.0).unwrap_or(json!({}));
    let mut params = serde_json::Map::new();
    if let Some(obj) = raw.as_object() {
        for k in allowed_keys {
            if let Some(v) = obj.get(k) {
                match v {
                    Value::String(s) if s.len() <= 512 => {
                        params.insert(k.to_string(), Value::String(s.clone()));
                    }
                    Value::Number(n) => {
                        params.insert(k.to_string(), Value::Number(n.clone()));
                    }
                    Value::Bool(b) => {
                        params.insert(k.to_string(), Value::Bool(*b));
                    }
                    _ => {}
                }
            }
        }
    }
    let params = Value::Object(params);
    let out = run_custom(&s, id, &action, &params, u.user_id, "client").await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "service.module_client",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "action": action }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "result": mx::redact(&out) })))
}

// ---- log + queue ------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    pub service_id: Option<Uuid>,
    #[serde(default)]
    pub errors: bool,
    pub limit: Option<i64>,
    pub before: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/module-log", responses((status = 200)))]
pub async fn log_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
    Query(q): Query<LogQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "data": mx::list_log(&s.pool, q.service_id, q.errors, q.limit.unwrap_or(50), q.before).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/module-queue", responses((status = 200)))]
pub async fn queue(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServicesManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "data": mx::failed(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/module-queue/{id}/retry", responses((status = 200), (status = 409)))]
pub async fn queue_retry(
    State(s): State<AppState>,
    p: RequirePermission<perms::ServicesManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let job = mx::last_dead_job(&s.pool, id)
        .await?
        .ok_or(AppError::Conflict("error.job_not_retryable"))?;
    jobs::retry(&s.pool, job).await?;
    mx::set_pending(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(p.0.user_id),
        "service.module_retry",
        Some(&id.to_string()),
        p.0.ip.as_deref(),
        json!({ "job": job }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "job_id": job })))
}

// ---- licensing --------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct VerifyBody {
    pub key: String,
    pub domain: Option<String>,
    pub ip: Option<String>,
    pub directory: Option<String>,
}

/// Public. Bind on first use, then verify; the response is HMAC-signed with
/// the licensing server's secret so installs can trust a cached copy.
#[utoipa::path(post, path = "/api/v1/licensing/verify", request_body = VerifyBody, responses((status = 200), (status = 429)))]
pub async fn license_verify(
    State(s): State<AppState>,
    ip: Ip,
    Json(b): Json<VerifyBody>,
) -> ApiResult<Json<Value>> {
    if !s
        .auth_limiter
        .check(&format!("lic:{}", ip.0.as_deref().unwrap_or("unknown")))
    {
        return Err(AppError::RateLimited.into());
    }
    let too_long = |x: &Option<String>| x.as_ref().is_some_and(|v| v.len() > 255);
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let invalid =
        || Json(json!({ "status": "invalid", "message": "license not found", "checked_at": now }));
    if !licensing::valid_key_format(&b.key)
        || too_long(&b.domain)
        || too_long(&b.ip)
        || too_long(&b.directory)
    {
        return Ok(invalid());
    }
    let Some(row) = mx::license(&s.pool, &b.key).await? else {
        return Ok(invalid());
    };
    let check = licensing::Check {
        domain: b.domain,
        ip: b.ip,
        directory: b.directory,
    };
    let v = licensing::decide(&row.status, &row.product_config, &row.module_data, &check);
    if let Some(bind) = &v.bind {
        let mut patch = serde_json::Map::new();
        for (k, val) in [
            ("bound_domain", &bind.domain),
            ("bound_ip", &bind.ip),
            ("bound_directory", &bind.directory),
        ] {
            if row.module_data.get(k).is_none() {
                if let Some(x) = val {
                    patch.insert(k.into(), json!(x));
                }
            }
        }
        mx::patch_module_data(&s.pool, row.service_id, &Value::Object(patch)).await?;
    }
    let payload = json!({
        "key": b.key,
        "status": v.status,
        "message": v.message,
        "product": row.product_name,
        "next_due_date": row.next_due_date.map(|d| d.to_string()),
        "checked_at": now,
    });
    let secret = match row.server_id {
        Some(sid) => pdb::server_secret(&s.pool, sid)
            .await?
            .and_then(|(_, _, sealed, _, _)| s.secret_box.as_ref()?.open(&sealed))
            .and_then(|k| String::from_utf8(k).ok()),
        None => None,
    };
    let mut out = payload.clone();
    if let Some(sec) = secret {
        out["signature"] = json!(licensing::sign(&sec, &payload));
    }
    Ok(Json(out))
}

/// Caller IP (for rate limiting the public endpoint).
pub struct Ip(pub Option<String>);

impl axum::extract::FromRequestParts<AppState> for Ip {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _s: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Ip(crate::extract::client_ip(parts)))
    }
}
