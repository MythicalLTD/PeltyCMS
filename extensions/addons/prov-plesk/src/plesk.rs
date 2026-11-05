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
use pelty_provisioning::util::{domain, gen_password, gone_ok, md, new_password, short_username};
use pelty_provisioning::{
    cfg_str, fld, remote_id, BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError,
    ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct Plesk;

const JSON: &str = "application/json";

fn err(msg: impl Into<String>) -> ProvisionError {
    ProvisionError::Rejected {
        status: 422,
        message: msg.into(),
    }
}

async fn plesk(
    server: &ServerConn,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> ProvisionResult<Value> {
    let raw = body.map(|b| (JSON, b.to_string()));
    call_raw(
        server,
        method,
        &format!("/api/v2{path}"),
        &[("accept", JSON), ("x-api-key", &server.api_key)],
        raw,
    )
    .await
}

async fn plesk_cli(server: &ServerConn, cmd: &str, params: &[&str]) -> ProvisionResult<Value> {
    let r = plesk(
        server,
        Method::POST,
        &format!("/cli/{cmd}/call"),
        Some(json!({ "params": params })),
    )
    .await?;
    if r.get("code").and_then(Value::as_i64).unwrap_or(0) != 0 {
        return Err(err(r
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("plesk cli failed")
            .to_string()));
    }
    Ok(r)
}

impl Plesk {
    fn id(&self) -> &'static str {
        "plesk"
    }
    fn account(&self, svc: &ServiceCtx) -> String {
        svc.module_data
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| short_username(&svc.customer_email, &svc.service_id))
    }
}

impl ProvisioningModule for Plesk {
    fn info(&self) -> ModuleInfo {
        let (name, server_fields, product_fields): (&'static str, Vec<FieldSpec>, Vec<FieldSpec>) = (
            "Plesk",
            vec![],
            vec![fld!("plan", "text", true), fld!("ip", "text", false)],
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
        &["change_password"]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            plesk(server, Method::GET, "/server", None)
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
            let mut data = json!({ "username": u, "domain": dom, "initial_password": pw });
            {
                let name = format!("{} {}", svc.customer_first_name, svc.customer_last_name);
                let name = if name.trim().is_empty() {
                    email.to_string()
                } else {
                    name.trim().to_string()
                };
                let client = plesk(server, Method::POST, "/clients", Some(json!({ "name": name, "login": u, "password": pw, "email": email, "type": "customer" }))).await?;
                let cid = client.get("id").cloned().ok_or_else(|| {
                    ProvisionError::Transient("plesk returned no client id".into())
                })?;
                let mut d = json!({ "name": dom, "hosting_type": "virtual", "hosting_settings": { "ftp_login": u, "ftp_password": pw }, "owner_client": { "id": cid }, "plan": { "name": cfg_str(c, "plan")? } });
                if let Some(ip) = c
                    .get("ip")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    d["ip_addresses"] = json!([ip]);
                }
                let dr = plesk(server, Method::POST, "/domains", Some(d)).await?;
                data["client_id"] = cid;
                data["domain_id"] = dr.get("id").cloned().unwrap_or(Value::Null);
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
            plesk_cli(
                server,
                "subscription",
                &["--webspace-off", md(svc, "domain")?],
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
            plesk_cli(
                server,
                "subscription",
                &["--webspace-on", md(svc, "domain")?],
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
            {
                if let Some(d) = svc.module_data.get("domain_id").filter(|v| !v.is_null()) {
                    gone_ok(
                        plesk(
                            server,
                            Method::DELETE,
                            &format!("/domains/{}", d.to_string().trim_matches('"')),
                            None,
                        )
                        .await,
                    )?;
                }
                match svc.module_data.get("client_id").filter(|v| !v.is_null()) {
                    Some(c) => gone_ok(
                        plesk(
                            server,
                            Method::DELETE,
                            &format!("/clients/{}", c.to_string().trim_matches('"')),
                            None,
                        )
                        .await,
                    ),
                    None => Ok(()),
                }
            }
        })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let c = &svc.product_config;
            plesk_cli(
                server,
                "subscription",
                &[
                    "--switch-subscription",
                    md(svc, "domain")?,
                    "-service-plan",
                    cfg_str(c, "plan")?,
                ],
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
                    plesk_cli(server, "customer", &["--update", u, "-passwd", &pw])
                        .await
                        .map(|_| ())?;
                    Ok(json!({ "password": pw }))
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
        Box::pin(async move { Ok(pelty_provisioning::sso::template_url(server, svc, token)) })
    }
}
