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
use pelty_provisioning::util::{domain, form, gen_password, new_password, short_username, str_or};
use pelty_provisioning::{cfg_str, fld, remote_id, BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx};
use pelty_provisioning::http::Method;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct Hestia;

const FORM: &str = "application/x-www-form-urlencoded";

fn err(msg: impl Into<String>) -> ProvisionError {
    ProvisionError::Rejected { status: 422, message: msg.into() }
}

async fn hestia(server: &ServerConn, cmd: &str, args: &[&str]) -> ProvisionResult<()> {
    let names: Vec<String> = (1..=args.len()).map(|i| format!("arg{i}")).collect();
    let mut pairs: Vec<(&str, &str)> = vec![("hash", server.api_key.as_str()), ("returncode", "yes"), ("cmd", cmd)];
    for (n, a) in names.iter().zip(args) {
        pairs.push((n.as_str(), a));
    }
    let r = call_raw(server, Method::POST, "/api/index.php", &[("accept", "text/plain")], Some((FORM, form(&pairs)))).await?;
    let code = match &r {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    };
    if code == "0" {
        Ok(())
    } else {
        Err(err(format!("Hestia {cmd} returned code {code}")))
    }
}

impl Hestia {
    fn id(&self) -> &'static str {
        "hestia"
    }
    fn account(&self, svc: &ServiceCtx) -> String {
        svc.module_data.get("username").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| short_username(&svc.customer_email, &svc.service_id))
    }
}

impl ProvisioningModule for Hestia {
    fn info(&self) -> ModuleInfo {
        let (name, server_fields, product_fields): (&'static str, Vec<FieldSpec>, Vec<FieldSpec>) = ("HestiaCP", vec![], vec![fld!("package", "text", true, "default")]);
        ModuleInfo { id: self.id(), name, server_fields, product_fields, actions: vec![] }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &["change_password"]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            hestia(server, "v-list-sys-info", &[]).await
        })
    }

    fn create<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                return Ok(Created { remote_id: id.clone(), module_data: svc.module_data.clone() });
            }
            let c = &svc.product_config;
            let dom = domain(svc)?;
            let u = self.account(svc);
            let pw = gen_password(16);
            let email = svc.customer_email.as_str();
            let data = json!({ "username": u, "domain": dom, "initial_password": pw });
            {
                    hestia(server, "v-add-user", &[&u, &pw, email, str_or(c, "package", "default")]).await?;
                    hestia(server, "v-add-domain", &[&u, &dom]).await?;
                }
            Ok(Created { remote_id: u, module_data: data })
        })
    }

    fn suspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            hestia(server, "v-suspend-user", &[u]).await
        })
    }

    fn unsuspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            hestia(server, "v-unsuspend-user", &[u]).await
        })
    }

    fn terminate<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            hestia(server, "v-delete-user", &[u]).await
        })
    }

    fn change_package<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            let c = &svc.product_config;
            hestia(server, "v-change-user-package", &[u, cfg_str(c, "package")?]).await
        })
    }

    fn custom<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, action: &'a str, params: &'a Value) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            match action {
                "change_password" => {
                    let pw = new_password(params)?;
                    hestia(server, "v-change-user-password", &[u, &pw]).await?;
                    Ok(json!({ "password": pw }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }

    fn sso_url<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, token: &'a str) -> BoxFuture<'a, ProvisionResult<Option<String>>> {
        Box::pin(async move {
            Ok(pelty_provisioning::sso::template_url(server, svc, token))
        })
    }
}
