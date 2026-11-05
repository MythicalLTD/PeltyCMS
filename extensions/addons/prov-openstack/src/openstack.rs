//! OpenStack-lite: Nova compute API with a pre-issued token
//! (application credential token sent as `X-Auth-Token`). The server URL
//! is the compute endpoint (e.g. `https://nova.example:8774/v2.1`).
//! Keystone login and token refresh are not implemented (🟡).
use pelty_provisioning::http::call_raw;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{gone_ok, hostname};
use pelty_provisioning::{
    cfg_str, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Default)]
pub struct OpenStack;

async fn nova(
    server: &ServerConn,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> ProvisionResult<Value> {
    call_raw(
        server,
        method,
        path,
        &[
            ("accept", "application/json"),
            ("x-auth-token", &server.api_key),
            ("openstack-api-version", "compute 2.79"),
        ],
        body.map(|b| ("application/json", b.to_string())),
    )
    .await
}

fn sid(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && !id.is_empty() {
        Ok(id)
    } else {
        Err(ProvisionError::Config("invalid server id".into()))
    }
}

impl ProvisioningModule for OpenStack {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "openstack",
            name: "OpenStack (Nova, token)",
            server_fields: vec![],
            product_fields: vec![
                fld!("flavor", "text", true),
                fld!("image", "text", true),
                fld!("network", "text", false),
                fld!("key_name", "text", false),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "reboot", "console"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            nova(server, Method::GET, "/flavors?limit=1", None)
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
            let mut s = json!({ "name": hostname(svc), "flavorRef": cfg_str(c, "flavor")?, "imageRef": cfg_str(c, "image")?, "metadata": { "pelty_service": svc.service_id } });
            s["networks"] = match c
                .get("network")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
            {
                Some(n) => json!([{ "uuid": n }]),
                None => json!("auto"),
            };
            if let Some(k) = c
                .get("key_name")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
            {
                s["key_name"] = json!(k);
            }
            let r = nova(
                server,
                Method::POST,
                "/servers",
                Some(json!({ "server": s })),
            )
            .await?;
            let id = r
                .pointer("/server/id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProvisionError::Transient("nova returned no server id".into()))?;
            Ok(Created {
                remote_id: id.to_string(),
                module_data: json!({ "initial_password": r.pointer("/server/adminPass") }),
            })
        })
    }
    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            nova(
                server,
                Method::POST,
                &format!("/servers/{}/action", sid(svc)?),
                Some(json!({ "suspend": null })),
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
            nova(
                server,
                Method::POST,
                &format!("/servers/{}/action", sid(svc)?),
                Some(json!({ "resume": null })),
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
                nova(
                    server,
                    Method::DELETE,
                    &format!("/servers/{}", sid(svc)?),
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
            nova(
                server,
                Method::POST,
                &format!("/servers/{}/action", sid(svc)?),
                Some(json!({ "resize": { "flavorRef": cfg_str(&svc.product_config, "flavor")? } })),
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
        _params: &'a Value,
    ) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let p = format!("/servers/{}", sid(svc)?);
            let body = match action {
                "start" => json!({ "os-start": null }),
                "stop" => json!({ "os-stop": null }),
                "reboot" => json!({ "reboot": { "type": "SOFT" } }),
                "console" => {
                    let r = nova(
                        server,
                        Method::POST,
                        &format!("{p}/remote-consoles"),
                        Some(json!({ "remote_console": { "protocol": "vnc", "type": "novnc" } })),
                    )
                    .await?;
                    return Ok(json!({ "url": r.pointer("/remote_console/url") }));
                }
                _ => return Err(ProvisionError::Config(format!("unknown action {action}"))),
            };
            nova(server, Method::POST, &format!("{p}/action"), Some(body))
                .await
                .map(|_| json!({ "ok": true }))
        })
    }
}
