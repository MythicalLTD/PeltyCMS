//! FeatherPanel admin API module (`/api/admin/*`, bearer API key).
//! Endpoints mirror FeatherPanel's routes: PUT creates, PATCH updates,
//! POST /suspend|/unsuspend, DELETE removes. Responses are wrapped as
//! `{ "success": true, "data": {…} }`.
use pelty_provisioning::http::call;
use pelty_provisioning::{cfg_u64, cfg_u64_or, remote_id, username, BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx};
use pelty_provisioning::http::Method;
use serde_json::{json, Value};

const ACCEPT: &str = "application/json";

#[derive(Default)]
pub struct FeatherPanel;

fn f(key: &'static str, label: &'static str, kind: &'static str, required: bool) -> FieldSpec {
    FieldSpec { key, label, kind, required, default: None }
}

fn limits(c: &Value) -> ProvisionResult<Value> {
    Ok(json!({
        "memory": cfg_u64(c, "memory")?,
        "swap": cfg_u64_or(c, "swap", 0)?,
        "disk": cfg_u64(c, "disk")?,
        "io": cfg_u64_or(c, "io", 500)?,
        "cpu": cfg_u64_or(c, "cpu", 0)?,
        "database_limit": cfg_u64_or(c, "databases", 0)?,
        "allocation_limit": cfg_u64_or(c, "allocations", 1)?,
        "backup_limit": cfg_u64_or(c, "backups", 0)?,
    }))
}

/// Random-enough initial password for a new panel account (the customer is
/// expected to reset it; never stored by PeltyCMS).
fn initial_password(seed: &str) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::from("Pc-");
    for i in 0..3 {
        let mut h = RandomState::new().build_hasher();
        h.write(seed.as_bytes());
        h.write_u8(i);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

async fn find_or_create_user(server: &ServerConn, svc: &ServiceCtx) -> ProvisionResult<u64> {
    let email = svc.customer_email.trim();
    let q: String = email.bytes().map(|b| if b.is_ascii_alphanumeric() || b"@._-".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let found = call(server, Method::GET, &format!("/api/admin/users?search={q}&limit=25"), ACCEPT, None).await?;
    if let Some(users) = found.pointer("/data/users").and_then(Value::as_array) {
        if let Some(id) = users.iter().find(|u| u.get("email").and_then(Value::as_str).is_some_and(|e| e.eq_ignore_ascii_case(email))).and_then(|u| u.get("id")).and_then(Value::as_u64) {
            return Ok(id);
        }
    }
    let first = if svc.customer_first_name.trim().is_empty() { "Customer" } else { svc.customer_first_name.trim() };
    let last = if svc.customer_last_name.trim().is_empty() { "User" } else { svc.customer_last_name.trim() };
    let body = json!({ "username": username(email, &svc.service_id), "first_name": first, "last_name": last, "email": email, "password": initial_password(&svc.service_id) });
    let made = call(server, Method::PUT, "/api/admin/users", ACCEPT, Some(&body)).await?;
    made.pointer("/data/user_id").and_then(Value::as_u64).ok_or_else(|| ProvisionError::Transient("panel returned no user id".into()))
}

impl ProvisioningModule for FeatherPanel {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            actions: vec![],
            id: "featherpanel",
            name: "FeatherPanel",
            server_fields: vec![],
            product_fields: vec![
                f("node_id", "provisioning.field.node", "number", true),
                f("allocation_id", "provisioning.field.allocation", "number", true),
                f("realms_id", "provisioning.field.realm", "number", true),
                f("spell_id", "provisioning.field.spell", "number", true),
                f("image", "provisioning.field.docker_image", "text", true),
                f("startup", "provisioning.field.startup", "text", true),
                f("variables", "provisioning.field.environment", "json", false),
                f("memory", "provisioning.field.memory", "number", true),
                f("swap", "provisioning.field.swap", "number", false),
                f("disk", "provisioning.field.disk", "number", true),
                f("io", "provisioning.field.io", "number", false),
                f("cpu", "provisioning.field.cpu", "number", false),
                f("databases", "provisioning.field.databases", "number", false),
                f("allocations", "provisioning.field.allocations", "number", false),
                f("backups", "provisioning.field.backups", "number", false),
            ],
        }
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { call(server, Method::GET, "/api/admin/servers?page=1&limit=1", ACCEPT, None).await.map(|_| ()) })
    }

    fn create<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                return Ok(Created { remote_id: id.clone(), module_data: svc.module_data.clone() });
            }
            let c = &svc.product_config;
            let owner = find_or_create_user(server, svc).await?;
            let mut body = limits(c)?;
            let short = &svc.service_id[..8.min(svc.service_id.len())];
            body["name"] = json!(format!("svc-{short}"));
            body["owner_id"] = json!(owner);
            for k in ["node_id", "allocation_id", "realms_id", "spell_id"] {
                body[k] = json!(cfg_u64(c, k)?);
            }
            for k in ["image", "startup"] {
                body[k] = json!(pelty_provisioning::cfg_str(c, k)?);
            }
            if let Some(v) = c.get("variables").filter(|v| v.is_object()) {
                body["variables"] = v.clone();
            }
            body["external_id"] = json!(format!("pelty-{}", svc.service_id));
            let r = call(server, Method::PUT, "/api/admin/servers", ACCEPT, Some(&body)).await?;
            let id = r.pointer("/data/server_id").and_then(Value::as_u64).ok_or_else(|| ProvisionError::Transient("panel returned no server id".into()))?;
            Ok(Created { remote_id: id.to_string(), module_data: json!({ "owner_id": owner }) })
        })
    }

    fn suspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { call(server, Method::POST, &format!("/api/admin/servers/{}/suspend", remote_id(svc)?), ACCEPT, None).await.map(|_| ()) })
    }

    fn unsuspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { call(server, Method::POST, &format!("/api/admin/servers/{}/unsuspend", remote_id(svc)?), ACCEPT, None).await.map(|_| ()) })
    }

    fn terminate<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            match call(server, Method::DELETE, &format!("/api/admin/servers/{}", remote_id(svc)?), ACCEPT, None).await {
                Err(ProvisionError::Rejected { status: 404, .. }) => Ok(()),
                r => r.map(|_| ()),
            }
        })
    }

    fn change_package<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let body = limits(&svc.product_config)?;
            call(server, Method::PATCH, &format!("/api/admin/servers/{}", remote_id(svc)?), ACCEPT, Some(&body)).await.map(|_| ())
        })
    }
}
