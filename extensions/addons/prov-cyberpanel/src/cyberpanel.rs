//! Shared-hosting control panels: cPanel/WHM, Plesk, DirectAdmin,
//! CyberPanel, HestiaCP and ISPConfig. Each one creates an account (and the
//! primary domain from the service's `domain` custom field) on a package /
//! plan named in the product settings.
//!
//! Auth per panel (the server "API key" field):
//! * cPanel/WHM, API token; header `Authorization: whm <user>:<token>`
//!   (user from server setting `username`, default `root`).
//! * Plesk, REST API key, header `X-API-Key`.
//! * DirectAdmin, login key; HTTP Basic `<username>:<key>`, `?json=yes`.
//! * CyberPanel, admin password; JSON `adminUser`/`adminPass`.
//! * HestiaCP, `access_key:secret_key` hash, form API with `returncode`.
//! * ISPConfig, remote user password; JSON remote API with session login.
use pelty_provisioning::http::call_raw;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{domain, gen_password, md, short_username, str_or};
use pelty_provisioning::{
    cfg_str, fld, BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct CyberPanel;

const JSON: &str = "application/json";

fn user(server: &ServerConn, default: &'static str) -> String {
    str_or(&server.config, "username", default).to_string()
}

fn err(msg: impl Into<String>) -> ProvisionError {
    ProvisionError::Rejected {
        status: 422,
        message: msg.into(),
    }
}

async fn cyber(
    server: &ServerConn,
    func: &str,
    mut body: Value,
    ok_key: &str,
) -> ProvisionResult<Value> {
    body["adminUser"] = json!(user(server, "admin"));
    body["adminPass"] = json!(server.api_key);
    let r = call_raw(
        server,
        Method::POST,
        &format!("/api/{func}"),
        &[("accept", JSON)],
        Some((JSON, body.to_string())),
    )
    .await?;
    if r.get(ok_key).and_then(Value::as_i64) != Some(1) {
        return Err(err(r
            .get("error_message")
            .and_then(Value::as_str)
            .unwrap_or("CyberPanel call failed")
            .to_string()));
    }
    Ok(r)
}

impl CyberPanel {
    fn id(&self) -> &'static str {
        "cyberpanel"
    }
    fn account(&self, svc: &ServiceCtx) -> String {
        svc.module_data
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| short_username(&svc.customer_email, &svc.service_id))
    }
}

impl ProvisioningModule for CyberPanel {
    fn info(&self) -> ModuleInfo {
        let (name, server_fields, product_fields): (&'static str, Vec<FieldSpec>, Vec<FieldSpec>) = (
            "CyberPanel",
            vec![fld!("username", "text", true, "admin")],
            vec![
                fld!("package", "text", true, "Default"),
                fld!("php", "text", false, "PHP 8.2"),
            ],
        );
        ModuleInfo {
            id: self.id(),
            name,
            server_fields,
            product_fields,
            actions: vec![],
        }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &[]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            cyber(server, "verifyConn", json!({}), "verifyConn")
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
            let dom = domain(svc)?;
            let u = self.account(svc);
            let pw = gen_password(16);
            let email = svc.customer_email.as_str();
            let data = json!({ "username": u, "domain": dom, "initial_password": pw });
            {
                cyber(server, "createWebsite", json!({ "domainName": dom, "ownerEmail": email, "packageName": str_or(c, "package", "Default"), "websiteOwner": u, "ownerPassword": pw, "phpSelection": str_or(c, "php", "PHP 8.2") }), "createWebSiteStatus").await?;
            }
            Ok(Created {
                remote_id: u,
                module_data: data,
            })
        })
    }

    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            cyber(
                server,
                "submitWebsiteStatus",
                json!({ "websiteName": md(svc, "domain")?, "state": "Suspend" }),
                "websiteStatus",
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
            cyber(
                server,
                "submitWebsiteStatus",
                json!({ "websiteName": md(svc, "domain")?, "state": "Activate" }),
                "websiteStatus",
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
            cyber(
                server,
                "deleteWebsite",
                json!({ "domainName": md(svc, "domain")? }),
                "websiteDeleteStatus",
            )
            .await
            .map(|_| ())
        })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let c = &svc.product_config;
            cyber(
                server,
                "changePackageAPI",
                json!({ "websiteName": md(svc, "domain")?, "packageName": cfg_str(c, "package")? }),
                "changePackage",
            )
            .await
            .map(|_| ())
        })
    }
    fn sso_url<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        token: &'a str,
    ) -> BoxFuture<'a, ProvisionResult<Option<String>>> {
        Box::pin(async move { Ok(pelty_provisioning::sso::template_url(server, svc, token)) })
    }
}
