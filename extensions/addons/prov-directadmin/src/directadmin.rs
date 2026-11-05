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
use base64::Engine;
use pelty_provisioning::http::Method;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct DirectAdmin;

const JSON: &str = "application/json";
const FORM: &str = "application/x-www-form-urlencoded";

fn user(server: &ServerConn, default: &'static str) -> String {
    str_or(&server.config, "username", default).to_string()
}

fn err(msg: impl Into<String>) -> ProvisionError {
    ProvisionError::Rejected { status: 422, message: msg.into() }
}

async fn da(server: &ServerConn, cmd: &str, args: &[(&str, &str)]) -> ProvisionResult<Value> {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user(server, "admin"), server.api_key));
    let auth = format!("Basic {basic}");
    let r = call_raw(server, Method::POST, &format!("/{cmd}?json=yes"), &[("accept", JSON), ("authorization", &auth)], Some((FORM, form(args)))).await?;
    let bad = match r.get("error") {
        Some(Value::String(s)) => s != "0",
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64() != Some(0),
        _ => false,
    };
    if bad {
        let m = r.get("details").or_else(|| r.get("text")).and_then(Value::as_str).unwrap_or("DirectAdmin call failed");
        return Err(err(m.to_string()));
    }
    Ok(r)
}

impl DirectAdmin {
    fn id(&self) -> &'static str {
        "directadmin"
    }
    fn account(&self, svc: &ServiceCtx) -> String {
        svc.module_data.get("username").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| short_username(&svc.customer_email, &svc.service_id))
    }
}

impl ProvisioningModule for DirectAdmin {
    fn info(&self) -> ModuleInfo {
        let (name, server_fields, product_fields): (&'static str, Vec<FieldSpec>, Vec<FieldSpec>) = ("DirectAdmin", vec![fld!("username", "text", true, "admin")], vec![fld!("package", "text", true), fld!("ip", "text", true)]);
        ModuleInfo { id: self.id(), name, server_fields, product_fields, actions: vec![] }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &["change_password"]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            da(server, "CMD_API_SHOW_USERS", &[]).await.map(|_| ())
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
                    da(server, "CMD_API_ACCOUNT_USER", &[("action", "create"), ("add", "Submit"), ("username", &u), ("email", email), ("passwd", &pw), ("passwd2", &pw), ("domain", &dom), ("package", cfg_str(c, "package")?), ("ip", cfg_str(c, "ip")?), ("notify", "no")]).await?;
                }
            Ok(Created { remote_id: u, module_data: data })
        })
    }

    fn suspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            da(server, "CMD_API_SELECT_USERS", &[("dosuspend", "yes"), ("select0", u)]).await.map(|_| ())
        })
    }

    fn unsuspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            da(server, "CMD_API_SELECT_USERS", &[("dounsuspend", "yes"), ("select0", u)]).await.map(|_| ())
        })
    }

    fn terminate<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            da(server, "CMD_API_SELECT_USERS", &[("confirmed", "Confirm"), ("delete", "yes"), ("select0", u)]).await.map(|_| ())
        })
    }

    fn change_package<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            let c = &svc.product_config;
            da(server, "CMD_API_MODIFY_USER", &[("action", "package"), ("user", u), ("package", cfg_str(c, "package")?)]).await.map(|_| ())
        })
    }

    fn custom<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, action: &'a str, params: &'a Value) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            match action {
                "change_password" => {
                    let pw = new_password(params)?;
                    da(server, "CMD_API_USER_PASSWD", &[("username", u), ("passwd", &pw), ("passwd2", &pw)]).await.map(|_| ())?;
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
