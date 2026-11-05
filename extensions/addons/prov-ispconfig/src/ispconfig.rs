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
use pelty_provisioning::util::{domain, enc, gen_password, md, short_username, str_or};
use pelty_provisioning::{
    fld, BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
pub struct IspConfig;

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

async fn isp(server: &ServerConn, func: &str, mut body: Value) -> ProvisionResult<Value> {
    let login = call_raw(
        server,
        Method::POST,
        "/remote/json.php?login",
        &[("accept", JSON)],
        Some((
            JSON,
            json!({ "username": user(server, "remote"), "password": server.api_key }).to_string(),
        )),
    )
    .await?;
    let sid = login
        .get("response")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProvisionError::Rejected {
            status: 401,
            message: "ISPConfig login failed".into(),
        })?;
    body["session_id"] = json!(sid);
    let r = call_raw(
        server,
        Method::POST,
        &format!("/remote/json.php?{}", enc(func)),
        &[("accept", JSON)],
        Some((JSON, body.to_string())),
    )
    .await?;
    if r.get("code").and_then(Value::as_str) != Some("ok") {
        return Err(err(r
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("ISPConfig call failed")
            .to_string()));
    }
    Ok(r.get("response").cloned().unwrap_or(Value::Null))
}

async fn isp_set_active(
    server: &ServerConn,
    svc: &ServiceCtx,
    active: &str,
) -> ProvisionResult<()> {
    let id = md(svc, "domain_id")?;
    let client = md(svc, "client_id")?;
    let mut rec = isp(server, "sites_web_domain_get", json!({ "primary_id": id })).await?;
    rec["active"] = json!(active);
    isp(
        server,
        "sites_web_domain_update",
        json!({ "client_id": client, "primary_id": id, "params": rec }),
    )
    .await
    .map(|_| ())
}

impl IspConfig {
    fn id(&self) -> &'static str {
        "ispconfig"
    }
    fn account(&self, svc: &ServiceCtx) -> String {
        svc.module_data
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| short_username(&svc.customer_email, &svc.service_id))
    }
}

impl ProvisioningModule for IspConfig {
    fn info(&self) -> ModuleInfo {
        let (name, server_fields, product_fields): (&'static str, Vec<FieldSpec>, Vec<FieldSpec>) = (
            "ISPConfig",
            vec![fld!("username", "text", true, "remote")],
            vec![
                fld!("server_id", "number", true, 1),
                fld!("template_master", "number", false),
                fld!("hd_quota", "number", false),
                fld!("traffic_quota", "number", false),
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
            isp(
                server,
                "server_get_serverid_by_ip",
                json!({ "ipaddress": "127.0.0.1" }),
            )
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
                let sid = pelty_provisioning::cfg_u64(c, "server_id")?;
                let client = isp(server, "client_add", json!({ "reseller_id": 0, "params": { "company_name": "", "contact_name": format!("{} {}", svc.customer_first_name, svc.customer_last_name).trim(), "email": email, "username": u, "password": pw, "usertheme": "default", "default_webserver": sid, "template_master": c.get("template_master").cloned().unwrap_or(json!(0)), "language": "en" } })).await?;
                let domain_id = isp(server, "sites_web_domain_add", json!({ "client_id": client, "params": { "server_id": sid, "domain": dom, "type": "vhost", "vhost_type": "name", "hd_quota": c.get("hd_quota").cloned().unwrap_or(json!(-1)), "traffic_quota": c.get("traffic_quota").cloned().unwrap_or(json!(-1)), "active": "y", "php": "php-fpm", "document_root": "", "system_user": "", "system_group": "" } })).await?;
                data["client_id"] = json!(client.to_string().trim_matches('"'));
                data["domain_id"] = json!(domain_id.to_string().trim_matches('"'));
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
        Box::pin(async move { isp_set_active(server, svc, "n").await })
    }

    fn unsuspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { isp_set_active(server, svc, "y").await })
    }

    fn terminate<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            {
                isp(
                    server,
                    "sites_web_domain_delete",
                    json!({ "primary_id": md(svc, "domain_id")? }),
                )
                .await?;
                isp(
                    server,
                    "client_delete",
                    json!({ "client_id": md(svc, "client_id")? }),
                )
                .await
                .map(|_| ())
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
            {
                let mut rec = isp(
                    server,
                    "sites_web_domain_get",
                    json!({ "primary_id": md(svc, "domain_id")? }),
                )
                .await?;
                for k in ["hd_quota", "traffic_quota"] {
                    if let Some(v) = c.get(k) {
                        rec[k] = v.clone();
                    }
                }
                isp(server, "sites_web_domain_update", json!({ "client_id": md(svc, "client_id")?, "primary_id": md(svc, "domain_id")?, "params": rec })).await.map(|_| ())
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
