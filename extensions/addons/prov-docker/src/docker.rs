//! Docker host module: one container per service via the Docker Engine
//! API (`/v1.43`). The server URL points at a TLS-terminating proxy or
//! socket proxy in front of the daemon; the API key is sent as a bearer
//! token for that proxy. Suspend = stop, unsuspend = start.
use pelty_provisioning::http::call;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{gone_ok, short};
use pelty_provisioning::{
    cfg_str, cfg_u64, cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError,
    ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

const V: &str = "/v1.43";
const J: &str = "application/json";

#[derive(Default)]
pub struct DockerHost;

fn cid(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(id)
    } else {
        Err(ProvisionError::Config("invalid container id".into()))
    }
}

fn host_config(c: &Value) -> ProvisionResult<Value> {
    let mem = cfg_u64(c, "memory")?;
    let cpus = cfg_u64_or(c, "cpu", 100)?;
    Ok(
        json!({ "Memory": mem * 1024 * 1024, "NanoCpus": cpus * 10_000_000, "PidsLimit": cfg_u64_or(c, "pids", 512)? }),
    )
}

impl ProvisioningModule for DockerHost {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "docker",
            name: "Docker host",
            server_fields: vec![],
            product_fields: vec![
                fld!("image", "text", true),
                fld!("memory", "number", true, 512),
                fld!("cpu", "number", false, 100),
                fld!("pids", "number", false, 512),
                fld!("environment", "json", false),
                fld!("port", "number", false),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "restart"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(server, Method::GET, &format!("{V}/version"), J, None)
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
            let env: Vec<String> = c
                .get("environment")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| {
                            format!(
                                "{k}={}",
                                v.as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| v.to_string())
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut hc = host_config(c)?;
            let mut body = json!({ "Image": cfg_str(c, "image")?, "Env": env, "Labels": { "pelty.service": svc.service_id } });
            if let Some(p) = c.get("port").and_then(|p| {
                p.as_u64()
                    .or_else(|| p.as_str().and_then(|s| s.parse().ok()))
            }) {
                body["ExposedPorts"] = json!({ format!("{p}/tcp"): {} });
                hc["PortBindings"] = json!({ format!("{p}/tcp"): [{ "HostPort": "" }] });
            }
            body["HostConfig"] = hc;
            let name = format!("pelty-{}", short(svc));
            let r = call(
                server,
                Method::POST,
                &format!("{V}/containers/create?name={name}"),
                J,
                Some(&body),
            )
            .await?;
            let id = r
                .get("Id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProvisionError::Transient("docker returned no container id".into()))?
                .to_string();
            call(
                server,
                Method::POST,
                &format!("{V}/containers/{id}/start"),
                J,
                None,
            )
            .await?;
            Ok(Created {
                remote_id: id,
                module_data: json!({ "name": name }),
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
                &format!("{V}/containers/{}/stop", cid(svc)?),
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
                &format!("{V}/containers/{}/start", cid(svc)?),
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
                    &format!("{V}/containers/{}?force=true&v=true", cid(svc)?),
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
                Method::POST,
                &format!("{V}/containers/{}/update", cid(svc)?),
                J,
                Some(&host_config(&svc.product_config)?),
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
            if !["start", "stop", "restart"].contains(&action) {
                return Err(ProvisionError::Config(format!("unknown action {action}")));
            }
            call(
                server,
                Method::POST,
                &format!("{V}/containers/{}/{action}", cid(svc)?),
                J,
                None,
            )
            .await
            .map(|_| json!({ "ok": true }))
        })
    }
    fn usage<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move {
            let r = call(
                server,
                Method::GET,
                &format!("{V}/containers/{}/json", cid(svc)?),
                J,
                None,
            )
            .await?;
            Ok(Some(
                json!({ "status": r.pointer("/State/Status"), "started_at": r.pointer("/State/StartedAt") }),
            ))
        })
    }
}
