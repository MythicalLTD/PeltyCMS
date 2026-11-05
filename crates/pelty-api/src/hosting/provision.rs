//! Provisioning job runner: loads the service + server, decrypts the panel
//! key, runs the module action and records the outcome. Transient errors are
//! retried by the queue; config errors and 4xx rejections go dead at once.
use crate::worker::JobError;
use crate::AppState;
use pelty_database::jobs::Job;
use pelty_database::module_ext as mx;
use pelty_database::provisioning as db;
use pelty_extensions::events::DomainEvent;
use pelty_provisioning::{ProvisionError, ServerConn, ServiceCtx};
use serde_json::{json, Value};
use uuid::Uuid;

fn service_id(job: &Job) -> Option<Uuid> {
    job.payload.get("service_id").and_then(Value::as_str).and_then(|s| s.parse().ok())
}

/// Everything needed to call a module for one service.
pub struct Loaded {
    pub module: std::sync::Arc<dyn pelty_provisioning::ProvisioningModule>,
    pub module_id: String,
    pub server_id: Uuid,
    pub conn: ServerConn,
    pub ctx: ServiceCtx,
}

/// Load module, server (picking one from the product's server group when the
/// service has none yet) and decrypted credentials. `Err` is a job error so
/// the queue and the admin/client routes share one path.
pub async fn load(s: &AppState, sid: Uuid) -> Result<Loaded, JobError> {
    let perm = |e: String| JobError::Permanent(e);
    let retry = |e: pelty_core::AppError| JobError::Retry(e.to_string());
    let t = db::target(&s.pool, sid).await.map_err(retry)?.ok_or_else(|| perm("service not found".into()))?;
    let module_id = t.module.clone().ok_or_else(|| perm("product has no provisioning module".into()))?;
    let module = s.provisioning.get(&module_id).ok_or_else(|| perm(format!("module {module_id} is not installed")))?;
    if !crate::addons::provisioning_enabled(s, &module_id).await.map_err(retry)? {
        return Err(perm(format!("module {module_id} is not installed")));
    }
    let server_id = match t.server_id {
        Some(id) => id,
        None => db::assign_from_group(&s.pool, sid, &module_id)
            .await
            .map_err(retry)?
            // A full group may gain capacity (new server, cancellations): retry.
            .ok_or_else(|| if t.server_group_id.is_some() { JobError::Retry("no server with capacity in the server group".into()) } else { perm("no server assigned to the product or service".into()) })?,
    };
    let (srv_module, base_url, sealed, config, enabled) =
        db::server_secret(&s.pool, server_id).await.map_err(retry)?.ok_or_else(|| perm("server not found".into()))?;
    if srv_module != module_id {
        return Err(perm(format!("server uses module {srv_module}, product uses {module_id}")));
    }
    if !enabled {
        return Err(JobError::Retry("server is disabled".into()));
    }
    let key = s.secret_box.as_ref().ok_or_else(|| perm("PELTY_ENCRYPTION_KEY is not configured".into()))?;
    let api_key = key.open(&sealed).and_then(|k| String::from_utf8(k).ok()).ok_or_else(|| perm("stored API key cannot be decrypted".into()))?;
    let conn = ServerConn { base_url, api_key, config };
    let ctx = ServiceCtx {
        service_id: sid.to_string(),
        customer_email: t.email,
        customer_first_name: t.first_name,
        customer_last_name: t.last_name,
        product_config: t.product_config,
        custom_fields: t.custom_fields,
        remote_id: t.remote_id,
        module_data: t.module_data,
    };
    Ok(Loaded { module, module_id, server_id, conn, ctx })
}

/// Module data keys holding a one-time secret: stored sealed, never plain.
pub const SEALED_KEYS: &[&str] = &["initial_password", "password", "root_password"];

/// Replace secret keys in module output with `<key>_sealed` (base64 of the
/// secret box). Without a key the secret is dropped.
pub fn seal_secrets(s: &AppState, data: &Value) -> Value {
    use base64::Engine;
    let Some(obj) = data.as_object() else { return data.clone() };
    let mut out = serde_json::Map::new();
    for (k, v) in obj {
        if SEALED_KEYS.contains(&k.as_str()) {
            if let (Some(b), Some(plain)) = (s.secret_box.as_ref(), v.as_str()) {
                out.insert(format!("{k}_sealed"), Value::String(base64::engine::general_purpose::STANDARD.encode(b.seal(plain.as_bytes()))));
            }
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// Open a `<key>_sealed` value from module data.
pub fn open_secret(s: &AppState, data: &Value, key: &str) -> Option<String> {
    use base64::Engine;
    let raw = data.get(format!("{key}_sealed"))?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
    s.secret_box.as_ref()?.open(&bytes).and_then(|p| String::from_utf8(p).ok())
}

pub async fn run_job(s: &AppState, job: &Job) -> Result<Value, JobError> {
    let sid = service_id(job).ok_or_else(|| JobError::Permanent("payload has no service_id".into()))?;
    let action = job.payload.get("action").and_then(Value::as_str).unwrap_or_default().to_string();
    if !pelty_provisioning::ACTIONS.contains(&action.as_str()) {
        return Err(JobError::Permanent(format!("unknown action {action}")));
    }
    let mut l = load(s, sid).await?;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct ProvInput {
        product_config: Value,
        custom_fields: Value,
    }
    let i = crate::addon_functions::rewrite_input(
        s,
        "service.provision",
        json!({ "service_id": sid, "action": action, "module": l.module_id }),
        ProvInput { product_config: l.ctx.product_config.clone(), custom_fields: l.ctx.custom_fields.clone() },
    )
    .await;
    (l.ctx.product_config, l.ctx.custom_fields) = (i.product_config, i.custom_fields);
    let started = std::time::Instant::now();
    let res = pelty_provisioning::run(l.module.as_ref(), &action, &l.conn, &l.ctx).await;
    let ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let (ok, err_s, out) = match &res {
        Ok(c) => (true, String::new(), c.as_ref().map(|c| json!({ "remote_id": c.remote_id, "module_data": c.module_data })).unwrap_or(json!({}))),
        Err(e) => (false, e.to_string(), json!({ "attempt": job.attempts + 1, "retryable": e.retryable() })),
    };
    let _ = mx::log(&s.pool, mx::LogEntry { service: Some(sid), server: Some(l.server_id), module: &l.module_id, action: &action, actor: None, source: "queue", ok, error: &err_s, output: &out, duration_ms: ms }).await;
    match res {
        Ok(created) => {
            let sealed = created.as_ref().map(|c| seal_secrets(s, &c.module_data));
            let c = created.as_ref().zip(sealed.as_ref()).map(|(c, d)| (c.remote_id.as_str(), d));
            db::record_success(&s.pool, sid, Some(l.server_id), c).await.map_err(|e| JobError::Retry(e.to_string()))?;
            s.emit(DomainEvent::new("service.provisioned", json!({ "service_id": sid, "action": action, "module": l.module_id })));
            Ok(json!({ "action": action, "remote_id": created.map(|c| c.remote_id) }))
        }
        Err(e) => {
            let _ = db::set_error(&s.pool, sid, &err_s).await;
            if e.retryable() {
                Err(JobError::Retry(err_s))
            } else {
                Err(JobError::Permanent(err_s))
            }
        }
    }
}

/// Out of attempts (or permanent error): mark the service, tell plugins and
/// mail every staff member holding `services.manage`.
pub async fn on_dead(s: &AppState, job: &Job, error: &str) {
    if let Some(sid) = service_id(job) {
        if let Err(e) = db::record_failure(&s.pool, sid, error).await {
            tracing::error!(error = %e, "record provisioning failure");
        }
        let action = job.payload.get("action").and_then(Value::as_str).unwrap_or_default().to_string();
        s.emit(DomainEvent::new("service.provision_failed", json!({ "service_id": sid, "action": action, "error": error })));
        if let Err(e) = notify_staff(s, sid, &action, error).await {
            tracing::warn!(error = %e, "provisioning failure mail");
        }
    }
}

async fn notify_staff(s: &AppState, sid: Uuid, action: &str, error: &str) -> pelty_core::AppResult<()> {
    let link = format!("{}/admin/services/{sid}", s.cfg.public_url);
    let err: String = error.chars().take(500).collect();
    let id = sid.to_string();
    for (to, locale) in pelty_database::affiliates::staff_recipients(&s.pool, "services.manage").await? {
        let locale = locale.filter(|l| s.i18n.has(l)).unwrap_or_else(|| s.cfg.default_locale.clone());
        let _ = crate::mail::enqueue(s, "provisioning_failed", &locale, &to, crate::mail::vars(&[("service", &id), ("action", action), ("error", &err), ("link", &link)])).await;
    }
    Ok(())
}

/// `ProvisionError` → stable API error code for the connection test.
pub fn test_error_code(e: &ProvisionError) -> &'static str {
    match e {
        ProvisionError::Config(_) => "error.server_config",
        ProvisionError::Rejected { status: 401 | 403, .. } => "error.server_auth",
        ProvisionError::Rejected { .. } => "error.server_rejected",
        ProvisionError::Transient(_) => "error.server_unreachable",
    }
}

/// Queue the panel action matching a lifecycle transition (if the product
/// uses a module). Never fails the caller: errors are logged.
pub async fn on_transition(s: &AppState, service: Uuid, from: &str, to: &str) {
    let action = match (from, to) {
        ("pending", "active") => "create",
        (_, "suspended") => "suspend",
        ("suspended", "active") => "unsuspend",
        (_, "terminated" | "cancelled" | "fraud") if from != "pending" => "terminate",
        _ => return,
    };
    enqueue_action(s, service, action).await;
}

/// A plan change was applied: push the new product config to the panel.
pub async fn on_plan_change(s: &AppState, service: Uuid) {
    enqueue_action(s, service, "change_package").await;
}

/// Internal event-bus subscriber: lifecycle changes made by automations
/// that only publish events (e.g. overdue suspend / unsuspend-on-payment)
/// get provisioning jobs here. Request handlers that already enqueued a
/// job mark their event with `"provisioned": true` and are skipped.
pub async fn on_event(s: &AppState, ev: &pelty_extensions::events::DomainEvent) {
    if ev.payload.get("provisioned").and_then(serde_json::Value::as_bool) == Some(true) {
        return;
    }
    let Some(sid) = ev.payload.get("service_id").and_then(|v| v.as_str()).and_then(|v| Uuid::parse_str(v).ok()) else { return };
    let (from, to) = match ev.name {
        "service.suspended" => ("active", "suspended"),
        "service.unsuspended" => ("suspended", "active"),
        _ => return,
    };
    on_transition(s, sid, from, to).await;
}

async fn enqueue_action(s: &AppState, service: Uuid, action: &str) {
    // Only products bound to an installed module are provisioned here
    // (other module ids may belong to plugins handling `service.*` events).
    match db::target(&s.pool, service).await {
        Ok(Some(t)) if t.module.as_deref().is_some_and(|m| s.provisioning.get(m).is_some()) => {
            let m = t.module.as_deref().unwrap_or_default();
            if !crate::addons::provisioning_enabled(s, m).await.unwrap_or(false) {
                return;
            }
        }
        Ok(_) => return,
        Err(e) => {
            tracing::error!(error = %e, %service, "load provisioning target");
            return;
        }
    }
    // `create` is keyed once per service; others per occurrence.
    let suffix = if action == "create" { "once".to_string() } else { Uuid::new_v4().simple().to_string() };
    if let Err(e) = db::enqueue(&s.pool, service, action, &suffix).await {
        tracing::error!(error = %e, %service, action, "enqueue provisioning job");
    }
}
