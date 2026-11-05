//! VPS control panels: Virtualizor (admin API, `index.php?act=…&api=json`)
//! and SolusVM v1 (admin API, `/api/admin/command.php`, `rdtype=json`).
//! Credentials: the server "API key" field holds `key:pass` for Virtualizor
//! and `id:key` for SolusVM.
use pelty_provisioning::http::call_raw;
use pelty_provisioning::util::{form, gen_password, hostname, new_password, short_username, str_or};
use pelty_provisioning::{cfg_str, cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx};
use pelty_provisioning::http::Method;
use serde_json::{json, Value};

const FORM: &str = "application/x-www-form-urlencoded";

fn split_key(server: &ServerConn) -> ProvisionResult<(&str, &str)> {
    server.api_key.split_once(':').ok_or_else(|| ProvisionError::Config("API key must be `key:pass` (Virtualizor) or `id:key` (SolusVM)".into()))
}

fn num_id(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
        Ok(id)
    } else {
        Err(ProvisionError::Config("invalid remote id".into()))
    }
}

#[derive(Default)]
pub struct SolusVm;

async fn svm(server: &ServerConn, action: &str, args: &[(&str, &str)]) -> ProvisionResult<Value> {
    let (id, key) = split_key(server)?;
    let mut p = vec![("id", id), ("key", key), ("action", action), ("rdtype", "json")];
    p.extend_from_slice(args);
    let r = call_raw(server, Method::POST, "/api/admin/command.php", &[("accept", "application/json")], Some((FORM, form(&p)))).await?;
    if r.get("status").and_then(Value::as_str) != Some("success") {
        return Err(ProvisionError::Rejected { status: 422, message: r.get("statusmsg").and_then(Value::as_str).unwrap_or("SolusVM call failed").to_string() });
    }
    Ok(r)
}

impl ProvisioningModule for SolusVm {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "solusvm",
            name: "SolusVM",
            server_fields: vec![],
            product_fields: vec![fld!("type", "text", true, "kvm"), fld!("node", "text", false), fld!("nodegroup", "text", false), fld!("plan", "text", true), fld!("template", "text", true), fld!("ips", "number", false, 1)],
            actions: vec![],
        }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "reboot", "change_password", "rebuild"]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { svm(server, "node-idlist", &[("type", "kvm")]).await.map(|_| ()) })
    }

    fn create<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                return Ok(Created { remote_id: id.clone(), module_data: svc.module_data.clone() });
            }
            let c = &svc.product_config;
            let user = short_username(&svc.customer_email, &svc.service_id);
            let (upw, pw, host) = (gen_password(16), gen_password(16), hostname(svc));
            let first = if svc.customer_first_name.is_empty() { "Customer" } else { svc.customer_first_name.as_str() };
            let last = if svc.customer_last_name.is_empty() { "User" } else { svc.customer_last_name.as_str() };
            // client-create fails if the client exists; that is fine.
            match svm(server, "client-create", &[("username", &user), ("password", &upw), ("email", &svc.customer_email), ("firstname", first), ("lastname", last)]).await {
                Ok(_) | Err(ProvisionError::Rejected { .. }) => {}
                Err(e) => return Err(e),
            }
            let ips = cfg_u64_or(c, "ips", 1)?.to_string();
            let mut args = vec![("type", str_or(c, "type", "kvm")), ("username", user.as_str()), ("plan", cfg_str(c, "plan")?), ("template", cfg_str(c, "template")?), ("hostname", host.as_str()), ("password", pw.as_str()), ("ips", ips.as_str())];
            if let Some(n) = c.get("node").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                args.push(("node", n));
            }
            if let Some(g) = c.get("nodegroup").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                args.push(("nodegroup", g));
            }
            let r = svm(server, "vserver-create", &args).await?;
            let id = r.get("vserverid").map(|v| v.to_string().trim_matches('"').to_string()).filter(|s| !s.is_empty()).ok_or_else(|| ProvisionError::Transient("SolusVM returned no vserverid".into()))?;
            Ok(Created { remote_id: id, module_data: json!({ "username": user, "hostname": host, "initial_password": pw, "ip": r.get("mainipaddress") }) })
        })
    }

    fn suspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { svm(server, "vserver-suspend", &[("vserverid", num_id(svc)?)]).await.map(|_| ()) })
    }
    fn unsuspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { svm(server, "vserver-unsuspend", &[("vserverid", num_id(svc)?)]).await.map(|_| ()) })
    }
    fn terminate<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { svm(server, "vserver-terminate", &[("vserverid", num_id(svc)?), ("deleteclient", "false")]).await.map(|_| ()) })
    }
    fn change_package<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { svm(server, "vserver-change", &[("vserverid", num_id(svc)?), ("plan", cfg_str(&svc.product_config, "plan")?), ("changehdd", "true")]).await.map(|_| ()) })
    }
    fn custom<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, action: &'a str, params: &'a Value) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let id = num_id(svc)?;
            match action {
                "start" => svm(server, "vserver-boot", &[("vserverid", id)]).await.map(|_| json!({ "ok": true })),
                "stop" => svm(server, "vserver-shutdown", &[("vserverid", id)]).await.map(|_| json!({ "ok": true })),
                "reboot" => svm(server, "vserver-reboot", &[("vserverid", id)]).await.map(|_| json!({ "ok": true })),
                "change_password" => {
                    let pw = new_password(params)?;
                    svm(server, "vserver-rootpassword", &[("vserverid", id), ("rootpassword", &pw)]).await?;
                    Ok(json!({ "password": pw }))
                }
                "rebuild" => {
                    let t = params.get("template").and_then(Value::as_str).map(str::to_string).unwrap_or(cfg_str(&svc.product_config, "template")?.to_string());
                    svm(server, "vserver-rebuild", &[("vserverid", id), ("template", &t)]).await.map(|_| json!({ "ok": true }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
