//! Pterodactyl-style Application API module. Also drives Pelican (the
//! Pterodactyl fork with Wings daemons), which keeps the same Application API
//! surface but expects plain `application/json`.
//! Users are matched by email (created when missing); servers are deployed by
//! location so the panel picks a node + free allocation.
use pelty_provisioning::http::call;
use pelty_provisioning::http::Method;
use pelty_provisioning::{
    cfg_str, cfg_u64, cfg_u64_or, remote_id, username, BoxFuture, Created, FieldSpec, ModuleInfo,
    ProvisionError, ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

/// One Application-API flavour (Pterodactyl or Pelican).
#[derive(Debug, Clone, Copy)]
pub struct AppPanel {
    pub id: &'static str,
    pub name: &'static str,
    pub accept: &'static str,
}

pub const PTERODACTYL: AppPanel = AppPanel {
    id: "pterodactyl",
    name: "Pterodactyl",
    accept: "Application/vnd.pterodactyl.v1+json",
};
pub const PELICAN: AppPanel = AppPanel {
    id: "pelican",
    name: "Pelican (Wings)",
    accept: "application/json",
};

fn f(key: &'static str, kind: &'static str, required: bool) -> FieldSpec {
    FieldSpec {
        key,
        label: label(key),
        kind,
        required,
        default: None,
    }
}

fn label(key: &'static str) -> &'static str {
    match key {
        "egg" => "provisioning.field.egg",
        "docker_image" => "provisioning.field.docker_image",
        "startup" => "provisioning.field.startup",
        "environment" => "provisioning.field.environment",
        "memory" => "provisioning.field.memory",
        "swap" => "provisioning.field.swap",
        "disk" => "provisioning.field.disk",
        "io" => "provisioning.field.io",
        "cpu" => "provisioning.field.cpu",
        "databases" => "provisioning.field.databases",
        "allocations" => "provisioning.field.allocations",
        "backups" => "provisioning.field.backups",
        "location" => "provisioning.field.location",
        _ => "provisioning.field.other",
    }
}

/// Limits block shared by create and change-package.
pub(crate) fn limits(c: &Value) -> ProvisionResult<(Value, Value)> {
    let limits = json!({
        "memory": cfg_u64(c, "memory")?,
        "swap": cfg_u64_or(c, "swap", 0)?,
        "disk": cfg_u64(c, "disk")?,
        "io": cfg_u64_or(c, "io", 500)?,
        "cpu": cfg_u64_or(c, "cpu", 0)?,
    });
    let features = json!({
        "databases": cfg_u64_or(c, "databases", 0)?,
        "allocations": cfg_u64_or(c, "allocations", 1)?,
        "backups": cfg_u64_or(c, "backups", 0)?,
    });
    Ok((limits, features))
}

async fn find_or_create_user(
    accept: &str,
    server: &ServerConn,
    svc: &ServiceCtx,
) -> ProvisionResult<u64> {
    let email = svc.customer_email.trim();
    let q: String = email
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"@._-".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let found = call(
        server,
        Method::GET,
        &format!("/api/application/users?filter[email]={q}"),
        accept,
        None,
    )
    .await?;
    if let Some(id) = found
        .pointer("/data/0/attributes/id")
        .and_then(Value::as_u64)
    {
        return Ok(id);
    }
    let first = if svc.customer_first_name.trim().is_empty() {
        "Customer"
    } else {
        svc.customer_first_name.trim()
    };
    let last = if svc.customer_last_name.trim().is_empty() {
        "User"
    } else {
        svc.customer_last_name.trim()
    };
    let body = json!({ "email": email, "username": username(email, &svc.service_id), "first_name": first, "last_name": last, "external_id": format!("pelty-{}", svc.customer_email) });
    let made = call(
        server,
        Method::POST,
        "/api/application/users",
        accept,
        Some(&body),
    )
    .await?;
    made.pointer("/attributes/id")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProvisionError::Transient("panel returned no user id".into()))
}

impl ProvisioningModule for AppPanel {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            actions: vec![],
            id: self.id,
            name: self.name,
            server_fields: vec![],
            product_fields: vec![
                f("egg", "number", true),
                f("location", "number", true),
                f("docker_image", "text", true),
                f("startup", "text", true),
                f("environment", "json", false),
                f("memory", "number", true),
                f("swap", "number", false),
                f("disk", "number", true),
                f("io", "number", false),
                f("cpu", "number", false),
                f("databases", "number", false),
                f("allocations", "number", false),
                f("backups", "number", false),
            ],
        }
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(
                server,
                Method::GET,
                "/api/application/locations?per_page=1",
                self.accept,
                None,
            )
            .await
            .map(|_| ())
        })
    }

    fn create<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                // Idempotent retry: already created.
                return Ok(Created {
                    remote_id: id.clone(),
                    module_data: svc.module_data.clone(),
                });
            }
            let c = &svc.product_config;
            let (limits, features) = limits(c)?;
            let egg = cfg_u64(c, "egg")?;
            let location = cfg_u64(c, "location")?;
            let env = match c.get("environment") {
                Some(Value::Object(m)) => Value::Object(m.clone()),
                Some(Value::String(s)) if !s.trim().is_empty() => {
                    serde_json::from_str(s).map_err(|_| {
                        ProvisionError::Config("environment must be a JSON object".into())
                    })?
                }
                _ => json!({}),
            };
            let user = find_or_create_user(self.accept, server, svc).await?;
            let body = json!({
                "name": c.get("name_prefix").and_then(Value::as_str).map(|p| format!("{p}{}", &svc.service_id[..8.min(svc.service_id.len())])).unwrap_or_else(|| format!("svc-{}", &svc.service_id[..8.min(svc.service_id.len())])),
                "user": user,
                "egg": egg,
                "docker_image": cfg_str(c, "docker_image")?,
                "startup": cfg_str(c, "startup")?,
                "environment": env,
                "limits": limits,
                "feature_limits": features,
                "deploy": { "locations": [location], "dedicated_ip": false, "port_range": [] },
                "start_on_completion": true,
                "external_id": format!("pelty-{}", svc.service_id),
            });
            let r = call(
                server,
                Method::POST,
                "/api/application/servers",
                self.accept,
                Some(&body),
            )
            .await?;
            let id = r
                .pointer("/attributes/id")
                .and_then(Value::as_u64)
                .ok_or_else(|| ProvisionError::Transient("panel returned no server id".into()))?;
            let identifier = r
                .pointer("/attributes/identifier")
                .cloned()
                .unwrap_or(Value::Null);
            Ok(Created {
                remote_id: id.to_string(),
                module_data: json!({ "user_id": user, "identifier": identifier }),
            })
        })
    }

    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(
                server,
                Method::POST,
                &format!("/api/application/servers/{}/suspend", remote_id(svc)?),
                self.accept,
                None,
            )
            .await
            .map(|_| ())
        })
    }

    fn unsuspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(
                server,
                Method::POST,
                &format!("/api/application/servers/{}/unsuspend", remote_id(svc)?),
                self.accept,
                None,
            )
            .await
            .map(|_| ())
        })
    }

    fn terminate<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            match call(
                server,
                Method::DELETE,
                &format!("/api/application/servers/{}", remote_id(svc)?),
                self.accept,
                None,
            )
            .await
            {
                // Already gone counts as terminated (idempotent retries).
                Err(ProvisionError::Rejected { status: 404, .. }) => Ok(()),
                r => r.map(|_| ()),
            }
        })
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &["reinstall", "status"]
    }

    fn custom<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        action: &'a str,
        _params: &'a Value,
    ) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let id = remote_id(svc)?;
            match action {
                "reinstall" => call(
                    server,
                    Method::POST,
                    &format!("/api/application/servers/{id}/reinstall"),
                    self.accept,
                    None,
                )
                .await
                .map(|_| json!({ "ok": true })),
                "status" => {
                    let cur = call(
                        server,
                        Method::GET,
                        &format!("/api/application/servers/{id}"),
                        self.accept,
                        None,
                    )
                    .await?;
                    let a = &cur["attributes"];
                    Ok(
                        json!({ "ok": true, "module_data_patch": { "identifier": a["identifier"], "status": a["status"], "suspended": a["suspended"] } }),
                    )
                }
                other => Err(ProvisionError::Config(format!(
                    "action {other} is not supported by this module"
                ))),
            }
        })
    }

    /// Plan limits as the panel reports them (memory/disk/cpu + feature limits).
    fn usage<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move {
            let id = remote_id(svc)?;
            let cur = call(
                server,
                Method::GET,
                &format!("/api/application/servers/{id}"),
                self.accept,
                None,
            )
            .await?;
            let a = &cur["attributes"];
            Ok(Some(json!({
                "memory_mb": a.pointer("/limits/memory"), "disk_mb": a.pointer("/limits/disk"), "cpu_percent": a.pointer("/limits/cpu"),
                "databases": a.pointer("/feature_limits/databases"), "allocations": a.pointer("/feature_limits/allocations"), "backups": a.pointer("/feature_limits/backups"),
            })))
        })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let id = remote_id(svc)?;
            let cur = call(
                server,
                Method::GET,
                &format!("/api/application/servers/{id}"),
                self.accept,
                None,
            )
            .await?;
            let allocation = cur
                .pointer("/attributes/allocation")
                .and_then(Value::as_u64)
                .ok_or_else(|| ProvisionError::Transient("panel returned no allocation".into()))?;
            let (limits, features) = limits(&svc.product_config)?;
            let mut body = limits;
            body["allocation"] = json!(allocation);
            body["feature_limits"] = features;
            call(
                server,
                Method::PATCH,
                &format!("/api/application/servers/{id}/build"),
                self.accept,
                Some(&body),
            )
            .await
            .map(|_| ())
        })
    }
}
