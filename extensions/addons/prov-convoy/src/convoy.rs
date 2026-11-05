//! Convoy, VirtFusion and Enhance modules, written against their public API
//! docs (Convoy application API, VirtFusion API v1, Enhance OpenAPI v12).
//! Covered by in-process mock servers only; not exercised against a live
//! panel.
use pelty_provisioning::http::call;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{gen_password, gone_ok, hostname, new_password};
use pelty_provisioning::{
    cfg_str, cfg_u64, cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError,
    ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

const J: &str = "application/json";

fn safe_seg(s: &str) -> ProvisionResult<&str> {
    if !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(s)
    } else {
        Err(ProvisionError::Config("invalid remote id".into()))
    }
}

fn id_str(v: Option<&Value>) -> Option<String> {
    v.and_then(|x| {
        x.as_str()
            .map(str::to_string)
            .or_else(|| x.as_u64().map(|n| n.to_string()))
    })
    .filter(|s| !s.is_empty())
}

fn display_name(svc: &ServiceCtx) -> String {
    let n = format!("{} {}", svc.customer_first_name, svc.customer_last_name)
        .trim()
        .to_string();
    if n.is_empty() {
        svc.customer_email.clone()
    } else {
        n
    }
}

#[derive(Default)]
pub struct Convoy;

async fn convoy_user(
    server: &ServerConn,
    svc: &ServiceCtx,
) -> ProvisionResult<(u64, Option<String>)> {
    let q = format!(
        "/api/application/users?filter[email]={}",
        pelty_provisioning::util::enc(&svc.customer_email)
    );
    let r = call(server, Method::GET, &q, J, None).await?;
    if let Some(id) = r.pointer("/data/0/id").and_then(Value::as_u64) {
        return Ok((id, None));
    }
    let pw = gen_password(16);
    let r = call(server, Method::POST, "/api/application/users", J, Some(&json!({ "root_admin": false, "name": display_name(svc), "email": svc.customer_email, "password": pw }))).await?;
    let id = r
        .pointer("/data/id")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProvisionError::Transient("Convoy returned no user id".into()))?;
    Ok((id, Some(pw)))
}

impl ProvisioningModule for Convoy {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "convoy",
            name: "Convoy",
            server_fields: vec![],
            product_fields: vec![
                fld!("node_id", "number", true),
                fld!("template_uuid", "text", true),
                fld!("cpu", "number", true, 1),
                fld!("memory", "number", true, 1024),
                fld!("disk", "number", true, 20480),
                fld!("snapshots", "number", false, 0),
                fld!("backups", "number", false, 0),
                fld!("bandwidth", "number", false, 0),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "restart", "kill", "change_password"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(
                server,
                Method::GET,
                "/api/application/nodes?per_page=1",
                J,
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
                return Ok(Created {
                    remote_id: id.clone(),
                    module_data: svc.module_data.clone(),
                });
            }
            let c = &svc.product_config;
            let (user_id, user_pw) = convoy_user(server, svc).await?;
            let root = gen_password(16);
            let host = hostname(svc);
            let mem_mb = cfg_u64(c, "memory")?;
            let disk_mb = cfg_u64(c, "disk")?;
            let bw = cfg_u64_or(c, "bandwidth", 0)?;
            let body = json!({
                "node_id": cfg_u64(c, "node_id")?, "user_id": user_id, "name": host, "hostname": host,
                "limits": { "cpu": cfg_u64(c, "cpu")?, "memory": mem_mb * 1024 * 1024, "disk": disk_mb * 1024 * 1024,
                    "snapshots": cfg_u64_or(c, "snapshots", 0)?, "backups": cfg_u64_or(c, "backups", 0)?,
                    "bandwidth": if bw == 0 { Value::Null } else { json!(bw * 1024 * 1024) }, "address_ids": [] },
                "account_password": root, "should_create_server": true, "template_uuid": cfg_str(c, "template_uuid")?, "start_on_completion": true,
            });
            let r = call(
                server,
                Method::POST,
                "/api/application/servers",
                J,
                Some(&body),
            )
            .await?;
            let uuid = id_str(r.pointer("/data/uuid")).ok_or_else(|| {
                ProvisionError::Transient("Convoy returned no server uuid".into())
            })?;
            Ok(Created {
                remote_id: uuid,
                module_data: json!({ "user_id": user_id, "hostname": host, "initial_password": root, "panel_password": user_pw }),
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
                &format!(
                    "/api/application/servers/{}/settings/suspend",
                    safe_seg(remote_id(svc)?)?
                ),
                J,
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
                &format!(
                    "/api/application/servers/{}/settings/unsuspend",
                    safe_seg(remote_id(svc)?)?
                ),
                J,
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
            gone_ok(
                call(
                    server,
                    Method::DELETE,
                    &format!("/api/application/servers/{}", safe_seg(remote_id(svc)?)?),
                    J,
                    None,
                )
                .await,
            )
        })
    }
    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let c = &svc.product_config;
            let body = json!({ "cpu": cfg_u64(c, "cpu")?, "memory": cfg_u64(c, "memory")? * 1024 * 1024, "disk": cfg_u64(c, "disk")? * 1024 * 1024,
                "snapshot_limit": cfg_u64_or(c, "snapshots", 0)?, "backup_limit": cfg_u64_or(c, "backups", 0)? });
            call(
                server,
                Method::PATCH,
                &format!(
                    "/api/application/servers/{}/settings/build",
                    safe_seg(remote_id(svc)?)?
                ),
                J,
                Some(&body),
            )
            .await
            .map(|_| ())
        })
    }
    fn sso_url<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        _token: &'a str,
    ) -> BoxFuture<'a, ProvisionResult<Option<String>>> {
        Box::pin(async move {
            let uid = svc
                .module_data
                .get("user_id")
                .and_then(Value::as_u64)
                .ok_or_else(|| ProvisionError::Config("module data has no user_id".into()))?;
            let r = call(
                server,
                Method::POST,
                &format!("/api/application/users/{uid}/generate-sso-token"),
                J,
                None,
            )
            .await?;
            let tok = r
                .pointer("/data/token")
                .and_then(Value::as_str)
                .ok_or_else(|| ProvisionError::Transient("Convoy returned no SSO token".into()))?;
            Ok(Some(format!(
                "{}/authenticate?token={}",
                server.base_url.trim_end_matches('/'),
                pelty_provisioning::util::enc(tok)
            )))
        })
    }
    fn custom<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        action: &'a str,
        params: &'a Value,
    ) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let id = safe_seg(remote_id(svc)?)?;
            match action {
                "start" | "stop" | "restart" | "kill" => call(
                    server,
                    Method::PATCH,
                    &format!("/api/client/servers/{id}/state"),
                    J,
                    Some(&json!({ "state": action })),
                )
                .await
                .map(|_| json!({ "ok": true })),
                "change_password" => {
                    let pw = new_password(params)?;
                    call(
                        server,
                        Method::POST,
                        &format!("/api/client/servers/{id}/settings/security/password"),
                        J,
                        Some(&json!({ "type": "cipassword", "password": pw })),
                    )
                    .await?;
                    Ok(json!({ "password": pw }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
