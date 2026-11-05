//! Convoy, VirtFusion and Enhance modules, written against their public API
//! docs (Convoy application API, VirtFusion API v1, Enhance OpenAPI v12).
//! Covered by in-process mock servers only; not exercised against a live
//! panel.
use pelty_provisioning::http::call;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{gone_ok, hostname, md, new_password, num_str};
use pelty_provisioning::{
    cfg_u64, cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError,
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
pub struct VirtFusion;

fn ext_rel(svc: &ServiceCtx) -> String {
    // Stable external relation id: derived from the customer email so one
    // panel user is reused across services.
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(svc.customer_email.to_ascii_lowercase().as_bytes());
    // VirtFusion requires an integer extRelationId; use 48 bits.
    let n = u64::from_be_bytes([0, 0, h[0], h[1], h[2], h[3], h[4], h[5]]);
    n.to_string()
}

fn num_id(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
        Ok(id)
    } else {
        Err(ProvisionError::Config("invalid remote id".into()))
    }
}

impl ProvisioningModule for VirtFusion {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "virtfusion",
            name: "VirtFusion",
            server_fields: vec![],
            product_fields: vec![
                fld!("package_id", "number", true),
                fld!("hypervisor_group_id", "number", true),
                fld!("os_template_id", "number", false),
                fld!("ipv4", "number", false, 1),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["boot", "shutdown", "restart", "poweroff", "change_password"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(server, Method::GET, "/api/v1/connect", J, None)
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
            let rel = ext_rel(svc);
            let uid = match call(
                server,
                Method::GET,
                &format!("/api/v1/users/{rel}/byExtRelation"),
                J,
                None,
            )
            .await
            {
                Ok(r) => r.pointer("/data/id").and_then(Value::as_u64),
                Err(ProvisionError::Rejected { status: 404, .. }) => None,
                Err(e) => return Err(e),
            };
            let uid = match uid {
                Some(u) => u,
                None => {
                    let r = call(server, Method::POST, "/api/v1/users", J, Some(&json!({ "name": display_name(svc), "email": svc.customer_email, "extRelationId": rel.parse::<u64>().unwrap_or(0), "sendMail": false }))).await?;
                    r.pointer("/data/id")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            ProvisionError::Transient("VirtFusion returned no user id".into())
                        })?
                }
            };
            let body = json!({ "packageId": cfg_u64(c, "package_id")?, "userId": uid, "hypervisorId": cfg_u64(c, "hypervisor_group_id")?, "ipv4": cfg_u64_or(c, "ipv4", 1)? });
            let r = call(server, Method::POST, "/api/v1/servers", J, Some(&body)).await?;
            let id = r
                .pointer("/data/id")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    ProvisionError::Transient("VirtFusion returned no server id".into())
                })?;
            let host = hostname(svc);
            if let Some(os) = c.get("os_template_id").filter(|v| !v.is_null() && *v != "") {
                let os = num_str(&json!({ "o": os }), "o")?;
                call(server, Method::POST, &format!("/api/v1/servers/{id}/build"), J, Some(&json!({ "operatingSystemId": os.parse::<u64>().unwrap_or(0), "name": host, "hostname": host, "email": true }))).await?;
            }
            Ok(Created {
                remote_id: id.to_string(),
                module_data: json!({ "user_id": uid, "ext_relation_id": rel, "hostname": host }),
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
                &format!("/api/v1/servers/{}/suspend", num_id(svc)?),
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
                &format!("/api/v1/servers/{}/unsuspend", num_id(svc)?),
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
                    &format!("/api/v1/servers/{}?delay=0", num_id(svc)?),
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
            call(
                server,
                Method::PUT,
                &format!(
                    "/api/v1/servers/{}/package/{}",
                    num_id(svc)?,
                    cfg_u64(&svc.product_config, "package_id")?
                ),
                J,
                None,
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
            let rel = safe_seg(md(svc, "ext_relation_id")?)?;
            let r = call(
                server,
                Method::POST,
                &format!(
                    "/api/v1/users/{rel}/serverAuthenticationTokens/{}",
                    num_id(svc)?
                ),
                J,
                None,
            )
            .await?;
            let ep = r
                .pointer("/data/authentication/endpoint_complete")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProvisionError::Transient("VirtFusion returned no auth endpoint".into())
                })?;
            Ok(Some(format!(
                "{}{}",
                server.base_url.trim_end_matches('/'),
                ep
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
            let id = num_id(svc)?;
            match action {
                "boot" | "shutdown" | "restart" | "poweroff" => call(
                    server,
                    Method::POST,
                    &format!("/api/v1/servers/{id}/power/{action}"),
                    J,
                    None,
                )
                .await
                .map(|_| json!({ "ok": true })),
                "change_password" => {
                    let _ = new_password(params)?;
                    let r = call(
                        server,
                        Method::POST,
                        &format!("/api/v1/servers/{id}/resetPassword"),
                        J,
                        Some(&json!({ "user": "root", "sendMail": false })),
                    )
                    .await?;
                    Ok(json!({ "password": r.pointer("/data/expectedPassword") }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
