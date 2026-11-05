//! Cloud VPS providers: Hetzner Cloud, DigitalOcean, Vultr and Linode
//! (Akamai). All use a bearer API token. Suspend powers the instance off,
//! unsuspend powers it on, terminate deletes it, change package resizes to
//! the product's plan / server type.
use pelty_provisioning::http::call;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{gone_ok, hostname};
use pelty_provisioning::{
    cfg_str, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct HetznerCloud;

const J: &str = "application/json";

fn rid(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(ProvisionError::Config("invalid remote id".into()));
    }
    Ok(id)
}

fn id_of(v: &Value, ptr: &str) -> ProvisionResult<String> {
    match v.pointer(ptr) {
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(ProvisionError::Transient(
            "provider returned no instance id".into(),
        )),
    }
}

impl HetznerCloud {
    fn base(&self, id: &str) -> String {
        format!("/v1/servers/{id}")
    }

    /// Power / lifecycle action by generic name.
    async fn act(
        &self,
        server: &ServerConn,
        id: &str,
        what: &str,
        extra: Value,
    ) -> ProvisionResult<Value> {
        let b = self.base(id);
        {
            let a = match what {
                "off" => "poweroff",
                "on" => "poweron",
                "shutdown" => "shutdown",
                other => other,
            };
            call(
                server,
                Method::POST,
                &format!("{b}/actions/{a}"),
                J,
                Some(&extra),
            )
            .await
        }
    }
}

impl ProvisioningModule for HetznerCloud {
    fn info(&self) -> ModuleInfo {
        let (id, name) = ("hetzner_cloud", "Hetzner Cloud");
        ModuleInfo {
            id,
            name,
            server_fields: vec![],
            product_fields: vec![
                fld!("plan", "text", true),
                fld!("region", "text", true),
                fld!("image", "text", true),
                fld!("ssh_keys", "json", false),
                fld!("user_data", "textarea", false),
            ],
            actions: vec![],
        }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &[
            "start",
            "stop",
            "reboot",
            "rebuild",
            "reset_password",
            "console",
        ]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        let path = "/v1/locations";
        Box::pin(async move { call(server, Method::GET, path, J, None).await.map(|_| ()) })
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
            let (plan, region, image) = (
                cfg_str(c, "plan")?,
                cfg_str(c, "region")?,
                cfg_str(c, "image")?,
            );
            let name = hostname(svc);
            let keys = c
                .get("ssh_keys")
                .filter(|v| v.is_array())
                .cloned()
                .unwrap_or(json!([]));
            let user_data = c.get("user_data").and_then(Value::as_str).unwrap_or("");
            let (path, body, ptr) = (
                "/v1/servers",
                json!({ "name": name, "server_type": plan, "location": region, "image": image, "ssh_keys": keys, "user_data": user_data, "labels": { "pelty_service": svc.service_id } }),
                "/server/id",
            );
            let r = call(server, Method::POST, path, J, Some(&body)).await?;
            let id = id_of(&r, ptr)?;
            let pw = r.get("root_password").cloned();
            Ok(Created {
                remote_id: id,
                module_data: json!({ "hostname": name, "initial_password": pw }),
            })
        })
    }

    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            self.act(server, rid(svc)?, "off", json!({}))
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
            self.act(server, rid(svc)?, "on", json!({}))
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
            gone_ok(call(server, Method::DELETE, &self.base(rid(svc)?), J, None).await)
        })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let id = rid(svc)?;
            let plan = cfg_str(&svc.product_config, "plan")?;
            {
                self.act(server, id, "off", json!({})).await?;
                self.act(
                    server,
                    id,
                    "change_type",
                    json!({ "server_type": plan, "upgrade_disk": false }),
                )
                .await?;
                self.act(server, id, "on", json!({})).await.map(|_| ())
            }
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
            let id = rid(svc)?;
            match action {
                "start" => self
                    .act(server, id, "on", json!({}))
                    .await
                    .map(|_| json!({ "ok": true })),
                "stop" => self
                    .act(server, id, "shutdown", json!({}))
                    .await
                    .map(|_| json!({ "ok": true })),
                "reboot" => self
                    .act(server, id, "reboot", json!({}))
                    .await
                    .map(|_| json!({ "ok": true })),
                "rebuild" => {
                    let image = params
                        .get("image")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or(cfg_str(&svc.product_config, "image")?.to_string());
                    let body = json!({ "image": image });
                    let r = self.act(server, id, "rebuild", body.clone()).await?;
                    Ok(
                        json!({ "ok": true, "password": r.get("root_password").or_else(|| body.get("root_pass")) }),
                    )
                }
                "reset_password" => {
                    let r = self.act(server, id, "reset_password", json!({})).await?;
                    Ok(json!({ "password": r.get("root_password") }))
                }
                "console" => {
                    let r = self.act(server, id, "request_console", json!({})).await?;
                    Ok(json!({ "url": r.get("wss_url"), "password": r.get("password") }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }

    fn usage<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move {
            let r = call(server, Method::GET, &self.base(rid(svc)?), J, None).await?;
            let s = json!({ "status": r.pointer("/server/status"), "ipv4": r.pointer("/server/public_net/ipv4/ip"), "traffic_out": r.pointer("/server/outgoing_traffic"), "traffic_in": r.pointer("/server/ingoing_traffic") });
            Ok(Some(s))
        })
    }
}
