//! SolusIO / SolusVM 2 (REST API v1, Bearer token). Written against the
//! public API docs; covered by in-process mock servers only, not exercised
//! against a live cluster.
use pelty_provisioning::http::{call, Method};
use pelty_provisioning::util::{gen_password, gone_ok, hostname, new_password};
use pelty_provisioning::{
    cfg_u64, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

const J: &str = "application/json";

#[derive(Default)]
pub struct SolusIo;

fn num_id(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
        Ok(id)
    } else {
        Err(ProvisionError::Config("invalid remote id".into()))
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn user_id(server: &ServerConn, email: &str) -> ProvisionResult<u64> {
    let r = call(
        server,
        Method::GET,
        &format!("/api/v1/users?filter%5Bsearch%5D={}", enc(email)),
        J,
        None,
    )
    .await?;
    let hit = r.get("data").and_then(Value::as_array).and_then(|a| {
        a.iter()
            .find(|u| {
                u.get("email")
                    .and_then(Value::as_str)
                    .is_some_and(|e| e.eq_ignore_ascii_case(email))
            })
            .and_then(|u| u.get("id"))
            .and_then(Value::as_u64)
    });
    if let Some(id) = hit {
        return Ok(id);
    }
    let r = call(
        server,
        Method::POST,
        "/api/v1/users",
        J,
        Some(&json!({ "email": email, "password": gen_password(20) })),
    )
    .await?;
    r.pointer("/data/id")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProvisionError::Transient("SolusIO returned no user id".into()))
}

async fn act(server: &ServerConn, svc: &ServiceCtx, verb: &str) -> ProvisionResult<()> {
    call(
        server,
        Method::POST,
        &format!("/api/v1/servers/{}/{verb}", num_id(svc)?),
        J,
        None,
    )
    .await
    .map(|_| ())
}

impl ProvisioningModule for SolusIo {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "solusio",
            name: "SolusIO (SolusVM 2)",
            server_fields: vec![],
            product_fields: vec![
                fld!("plan_id", "number", true),
                fld!("location_id", "number", true),
                fld!("os_image_version_id", "number", true),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "restart", "change_password"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(server, Method::GET, "/api/v1/plans", J, None)
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
            let uid = user_id(server, &svc.customer_email).await?;
            let (pw, host) = (gen_password(16), hostname(svc));
            let body = json!({ "name": host, "plan": cfg_u64(c, "plan_id")?, "location": cfg_u64(c, "location_id")?, "os": cfg_u64(c, "os_image_version_id")?, "user": uid, "password": pw });
            let r = call(server, Method::POST, "/api/v1/servers", J, Some(&body)).await?;
            let id = r
                .pointer("/data/id")
                .and_then(Value::as_u64)
                .ok_or_else(|| ProvisionError::Transient("SolusIO returned no server id".into()))?;
            Ok(Created {
                remote_id: id.to_string(),
                module_data: json!({ "user_id": uid, "hostname": host, "initial_password": pw }),
            })
        })
    }
    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(act(server, svc, "suspend"))
    }
    fn unsuspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(act(server, svc, "resume"))
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
                    &format!("/api/v1/servers/{}", num_id(svc)?),
                    J,
                    None,
                )
                .await
                .map(|_| ()),
            )
        })
    }
    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let body = json!({ "plan_id": cfg_u64(&svc.product_config, "plan_id")?, "preserve_disk": true });
            call(
                server,
                Method::POST,
                &format!("/api/v1/servers/{}/resize", num_id(svc)?),
                J,
                Some(&body),
            )
            .await
            .map(|_| ())
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
            match action {
                "start" | "stop" | "restart" => act(server, svc, action)
                    .await
                    .map(|_| json!({ "ok": true })),
                "change_password" => {
                    let pw = new_password(params)?;
                    call(
                        server,
                        Method::POST,
                        &format!("/api/v1/servers/{}/reset_password", num_id(svc)?),
                        J,
                        Some(&json!({ "password": pw })),
                    )
                    .await?;
                    Ok(json!({ "password": pw }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
