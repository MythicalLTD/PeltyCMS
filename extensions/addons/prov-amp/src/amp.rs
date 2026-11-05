//! CubeCoders AMP (game server panel). JSON API `POST /API/<Module>/<Method>`
//! with a session from `Core/Login`. Instances are deployed from an ADS
//! deployment template and identified by their `InstanceName`.
use pelty_provisioning::http::call_raw;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{gen_password, short, short_username, str_or};
use pelty_provisioning::{
    cfg_u64, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Default)]
pub struct Amp;

async fn amp(server: &ServerConn, method: &str, mut body: Value) -> ProvisionResult<Value> {
    let h = [("accept", "application/json")];
    let login = call_raw(server, Method::POST, "/API/Core/Login", &h, Some(("application/json", json!({ "username": str_or(&server.config, "username", "admin"), "password": server.api_key, "token": "", "rememberMe": false }).to_string()))).await?;
    let sid = login
        .get("sessionID")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProvisionError::Rejected {
            status: 401,
            message: "AMP login failed".into(),
        })?;
    body["SESSIONID"] = json!(sid);
    let r = call_raw(
        server,
        Method::POST,
        &format!("/API/{method}"),
        &h,
        Some(("application/json", body.to_string())),
    )
    .await?;
    // ActionResult shape: {"Status": bool, "Reason": "..."}; others return data.
    if r.get("Status").and_then(Value::as_bool) == Some(false) {
        return Err(ProvisionError::Rejected {
            status: 422,
            message: r
                .get("Reason")
                .and_then(Value::as_str)
                .unwrap_or("AMP call failed")
                .to_string(),
        });
    }
    Ok(r)
}

fn name(svc: &ServiceCtx) -> ProvisionResult<String> {
    let n = remote_id(svc)?;
    if n.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(n.to_string())
    } else {
        Err(ProvisionError::Config("invalid instance name".into()))
    }
}

impl ProvisioningModule for Amp {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "amp",
            name: "CubeCoders AMP",
            server_fields: vec![fld!("username", "text", true, "admin")],
            product_fields: vec![
                fld!("template_id", "number", true),
                fld!("post_create", "number", false, 2),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "restart"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            amp(server, "ADSModule/GetInstances", json!({}))
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
            let user = short_username(&svc.customer_email, &svc.service_id);
            let pw = gen_password(16);
            let tag = format!("pelty-{}", short(svc));
            let r = amp(server, "ADSModule/DeployTemplate", json!({ "TemplateID": cfg_u64(c, "template_id")?, "NewUsername": user, "NewPassword": pw, "NewEmail": svc.customer_email, "RequiredTags": [], "Tag": tag, "FriendlyName": tag, "Secret": svc.service_id, "PostCreate": pelty_provisioning::cfg_u64_or(c, "post_create", 2)?, "ExtraProvisionSettings": {} })).await?;
            let inst = r
                .pointer("/Result/InstanceName")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(tag.clone());
            Ok(Created {
                remote_id: inst,
                module_data: json!({ "username": user, "initial_password": pw, "tag": tag }),
            })
        })
    }
    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            amp(
                server,
                "ADSModule/StopInstance",
                json!({ "InstanceName": name(svc)? }),
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
            amp(
                server,
                "ADSModule/StartInstance",
                json!({ "InstanceName": name(svc)? }),
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
            let n = name(svc)?;
            let _ = amp(
                server,
                "ADSModule/StopInstance",
                json!({ "InstanceName": n }),
            )
            .await;
            amp(
                server,
                "ADSModule/DeleteInstance",
                json!({ "InstanceName": n }),
            )
            .await
            .map(|_| ())
        })
    }
    fn change_package<'a>(
        &'a self,
        _server: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            Err(ProvisionError::Config(
                "AMP has no resize API; change the template and reinstall".into(),
            ))
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
            let m = match action {
                "start" => "ADSModule/StartInstance",
                "stop" => "ADSModule/StopInstance",
                "restart" => "ADSModule/RestartInstance",
                _ => return Err(ProvisionError::Config(format!("unknown action {action}"))),
            };
            amp(server, m, json!({ "InstanceName": name(svc)? }))
                .await
                .map(|_| json!({ "ok": true }))
        })
    }
}
