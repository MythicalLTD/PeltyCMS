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
use pelty_provisioning::util::{domain, form, gen_password, new_password, short_username, str_or};
use pelty_provisioning::{
    cfg_str, fld, remote_id, BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError,
    ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct Cpanel;

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

async fn whm(server: &ServerConn, func: &str, args: &[(&str, &str)]) -> ProvisionResult<Value> {
    let auth = format!("whm {}:{}", user(server, "root"), server.api_key);
    let mut q = vec![("api.version", "1")];
    q.extend_from_slice(args);
    let r = call_raw(
        server,
        Method::GET,
        &format!("/json-api/{func}?{}", form(&q)),
        &[("accept", JSON), ("authorization", &auth)],
        None,
    )
    .await?;
    if r.pointer("/metadata/result").and_then(Value::as_i64) != Some(1) {
        return Err(err(r
            .pointer("/metadata/reason")
            .and_then(Value::as_str)
            .unwrap_or("WHM call failed")
            .to_string()));
    }
    Ok(r.get("data").cloned().unwrap_or(Value::Null))
}

impl Cpanel {
    fn id(&self) -> &'static str {
        "cpanel"
    }
    fn account(&self, svc: &ServiceCtx) -> String {
        svc.module_data
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| short_username(&svc.customer_email, &svc.service_id))
    }
}

impl ProvisioningModule for Cpanel {
    fn info(&self) -> ModuleInfo {
        let (name, server_fields, product_fields): (&'static str, Vec<FieldSpec>, Vec<FieldSpec>) = (
            "cPanel / WHM",
            vec![fld!("username", "text", true, "root")],
            vec![
                fld!("package", "text", true),
                fld!("ip", "text", false),
                fld!("cgi", "text", false),
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
        &["change_password", "login"]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { whm(server, "version", &[]).await.map(|_| ()) })
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
                let pkg = cfg_str(c, "package")?;
                let mut args = vec![
                    ("username", u.as_str()),
                    ("domain", dom.as_str()),
                    ("plan", pkg),
                    ("password", pw.as_str()),
                    ("contactemail", email),
                ];
                if let Some(ip) = c
                    .get("ip")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    args.push(("ip", ip));
                }
                whm(server, "createacct", &args).await?;
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
            let u = remote_id(svc)?;
            whm(
                server,
                "suspendacct",
                &[("user", u), ("reason", "Suspended by billing")],
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
            let u = remote_id(svc)?;
            whm(server, "unsuspendacct", &[("user", u)])
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
            let u = remote_id(svc)?;
            whm(server, "removeacct", &[("user", u)]).await.map(|_| ())
        })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let u = remote_id(svc)?;
            let c = &svc.product_config;
            whm(
                server,
                "changepackage",
                &[("user", u), ("pkg", cfg_str(c, "package")?)],
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
            let u = remote_id(svc)?;
            match action {
                "change_password" => {
                    let pw = new_password(params)?;
                    whm(server, "passwd", &[("user", u), ("password", &pw)])
                        .await
                        .map(|_| ())?;
                    Ok(json!({ "password": pw }))
                }
                "login" => {
                    let d = whm(
                        server,
                        "create_user_session",
                        &[("user", u), ("service", "cpaneld")],
                    )
                    .await?;
                    Ok(json!({ "url": d.get("url") }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }

    fn sso_url<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        token: &'a str,
    ) -> BoxFuture<'a, ProvisionResult<Option<String>>> {
        Box::pin(async move {
            let _ = token;
            let d = whm(
                server,
                "create_user_session",
                &[("user", remote_id(svc)?), ("service", "cpaneld")],
            )
            .await?;
            Ok(d.get("url").and_then(Value::as_str).map(str::to_string))
        })
    }
}
