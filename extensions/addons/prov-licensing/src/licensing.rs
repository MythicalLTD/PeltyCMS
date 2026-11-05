//! Licensing module: issues license keys; the verify primitives (`sign`, `decide`) stay in the host.
use pelty_provisioning::licensing::*;
use pelty_provisioning::{
    fld, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule,
    ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Default)]
pub struct Licensing;

impl ProvisioningModule for Licensing {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "licensing",
            name: "Software licensing",
            server_fields: vec![],
            product_fields: vec![
                fld!("key_prefix", "text", false),
                fld!("bind_domain", "text", false, "true"),
                fld!("bind_ip", "text", false, "true"),
                fld!("bind_directory", "text", false, "false"),
            ],
            actions: vec![],
        }
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["reissue"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            if server.api_key.len() < 16 {
                return Err(ProvisionError::Config(
                    "signing secret (API key) must be at least 16 characters".into(),
                ));
            }
            Ok(())
        })
    }
    fn create<'a>(
        &'a self,
        _server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                return Ok(Created {
                    remote_id: id.clone(),
                    module_data: svc.module_data.clone(),
                });
            }
            let key = generate_key(
                svc.product_config
                    .get("key_prefix")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
            Ok(Created {
                remote_id: key,
                module_data: json!({}),
            })
        })
    }
    fn suspend<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn unsuspend<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn terminate<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn change_package<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn custom<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
        action: &'a str,
        _p: &'a Value,
    ) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            match action {
                "reissue" => Ok(
                    json!({ "ok": true, "module_data_patch": { "bound_domain": null, "bound_ip": null, "bound_directory": null } }),
                ),
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
