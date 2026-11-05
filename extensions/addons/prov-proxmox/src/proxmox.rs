//! Proxmox VE module (API v2, `/api2/json`), KVM (`qemu`, cloned from a
//! template VM) and LXC (`lxc`, created from an `ostemplate`).
//! Auth: API token `user@realm!tokenid=secret` sent as
//! `Authorization: PVEAPIToken=<token>`. Suspend = stop (as WHMCS modules do),
//! unsuspend = start, terminate = stop + delete with purge.
use pelty_provisioning::http::call_raw;
use pelty_provisioning::util::{gen_password, gone_ok, hostname, md, new_password, str_or};
use pelty_provisioning::{cfg_str, cfg_u64, cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx};
use pelty_provisioning::http::Method;
use serde_json::{json, Value};

#[derive(Default)]
pub struct Proxmox;

async fn pve(server: &ServerConn, method: Method, path: &str, body: Option<Value>) -> ProvisionResult<Value> {
    let auth = format!("PVEAPIToken={}", server.api_key);
    let raw = body.map(|b| ("application/json", b.to_string()));
    let r = call_raw(server, method, &format!("/api2/json{path}"), &[("accept", "application/json"), ("authorization", &auth)], raw).await?;
    Ok(r.get("data").cloned().unwrap_or(Value::Null))
}

fn kind(svc: &ServiceCtx) -> &str {
    svc.module_data.get("type").and_then(Value::as_str).unwrap_or_else(|| str_or(&svc.product_config, "type", "qemu"))
}

fn node<'a>(server: &'a ServerConn, svc: &'a ServiceCtx) -> &'a str {
    svc.module_data.get("node").and_then(Value::as_str).unwrap_or_else(|| str_or(&server.config, "node", str_or(&svc.product_config, "node", "pve")))
}

fn base(server: &ServerConn, svc: &ServiceCtx) -> ProvisionResult<String> {
    let vmid = remote_id(svc)?;
    if !vmid.chars().all(|c| c.is_ascii_digit()) {
        return Err(ProvisionError::Config("invalid vmid".into()));
    }
    Ok(format!("/nodes/{}/{}/{vmid}", node(server, svc), kind(svc)))
}

/// Resources for create / change package (memory in MB, disk in GB).
fn resources(c: &Value, lxc: bool) -> ProvisionResult<Value> {
    let mut v = json!({ "cores": cfg_u64_or(c, "cores", 1)?, "memory": cfg_u64(c, "memory")? });
    if lxc {
        v["swap"] = json!(cfg_u64_or(c, "swap", 512)?);
    } else {
        v["sockets"] = json!(cfg_u64_or(c, "sockets", 1)?);
    }
    if let Some(limit) = c.get("cpu_limit").filter(|x| !x.is_null() && *x != "") {
        v["cpulimit"] = limit.clone();
    }
    Ok(v)
}

impl ProvisioningModule for Proxmox {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "proxmox",
            name: "Proxmox VE",
            server_fields: vec![fld!("node", "text", true, "pve")],
            product_fields: vec![
                fld!("type", "text", true, "qemu"),
                fld!("template_vmid", "number", false),
                fld!("ostemplate", "text", false),
                fld!("storage", "text", true, "local-lvm"),
                fld!("bridge", "text", false, "vmbr0"),
                fld!("cores", "number", true, 1),
                fld!("sockets", "number", false, 1),
                fld!("memory", "number", true, 1024),
                fld!("swap", "number", false, 512),
                fld!("disk", "number", true, 20),
                fld!("cpu_limit", "number", false),
                fld!("bandwidth_limit", "number", false),
            ],
            actions: vec![],
        }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &["start", "stop", "reboot", "shutdown", "console", "change_password"]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { pve(server, Method::GET, "/version", None).await.map(|_| ()) })
    }

    fn create<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                return Ok(Created { remote_id: id.clone(), module_data: svc.module_data.clone() });
            }
            let c = &svc.product_config;
            let t = str_or(c, "type", "qemu");
            if t != "qemu" && t != "lxc" {
                return Err(ProvisionError::Config("type must be qemu or lxc".into()));
            }
            let node = str_or(&server.config, "node", str_or(c, "node", "pve")).to_string();
            let vmid = pve(server, Method::GET, "/cluster/nextid", None).await?;
            let vmid: u64 = vmid.as_u64().or_else(|| vmid.as_str().and_then(|s| s.parse().ok())).ok_or_else(|| ProvisionError::Transient("no next vmid".into()))?;
            let storage = str_or(c, "storage", "local-lvm");
            let disk = cfg_u64(c, "disk")?;
            let password = gen_password(16);
            let host = hostname(svc);
            if t == "lxc" {
                let mut body = resources(c, true)?;
                body["vmid"] = json!(vmid);
                body["ostemplate"] = json!(cfg_str(c, "ostemplate")?);
                body["hostname"] = json!(host);
                body["password"] = json!(password);
                body["rootfs"] = json!(format!("{storage}:{disk}"));
                body["net0"] = json!(format!("name=eth0,bridge={},ip=dhcp", str_or(c, "bridge", "vmbr0")));
                body["unprivileged"] = json!(1);
                body["start"] = json!(1);
                pve(server, Method::POST, &format!("/nodes/{node}/lxc"), Some(body)).await?;
            } else {
                let tpl = cfg_u64(c, "template_vmid")?;
                pve(server, Method::POST, &format!("/nodes/{node}/qemu/{tpl}/clone"), Some(json!({ "newid": vmid, "name": host, "full": 1, "storage": storage }))).await?;
                let mut cfg = resources(c, false)?;
                cfg["cipassword"] = json!(password);
                pve(server, Method::PUT, &format!("/nodes/{node}/qemu/{vmid}/config"), Some(cfg)).await?;
                pve(server, Method::PUT, &format!("/nodes/{node}/qemu/{vmid}/resize"), Some(json!({ "disk": "scsi0", "size": format!("{disk}G") }))).await?;
                pve(server, Method::POST, &format!("/nodes/{node}/qemu/{vmid}/status/start"), None).await?;
            }
            Ok(Created { remote_id: vmid.to_string(), module_data: json!({ "node": node, "type": t, "hostname": host, "initial_password": password }) })
        })
    }

    fn suspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { pve(server, Method::POST, &format!("{}/status/stop", base(server, svc)?), None).await.map(|_| ()) })
    }

    fn unsuspend<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { pve(server, Method::POST, &format!("{}/status/start", base(server, svc)?), None).await.map(|_| ()) })
    }

    fn terminate<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let b = base(server, svc)?;
            gone_ok(pve(server, Method::POST, &format!("{b}/status/stop"), None).await)?;
            gone_ok(pve(server, Method::DELETE, &format!("{b}?purge=1&destroy-unreferenced-disks=1"), None).await)
        })
    }

    fn change_package<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let b = base(server, svc)?;
            let lxc = kind(svc) == "lxc";
            pve(server, Method::PUT, &format!("{b}/config"), Some(resources(&svc.product_config, lxc)?)).await?;
            let disk = cfg_u64(&svc.product_config, "disk")?;
            // Proxmox only grows disks; shrinking is refused remotely.
            pve(server, Method::PUT, &format!("{b}/resize"), Some(json!({ "disk": if lxc { "rootfs" } else { "scsi0" }, "size": format!("{disk}G") }))).await.map(|_| ())
        })
    }

    fn custom<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx, action: &'a str, params: &'a Value) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let b = base(server, svc)?;
            match action {
                "start" | "stop" | "reboot" | "shutdown" => pve(server, Method::POST, &format!("{b}/status/{action}"), None).await.map(|_| json!({ "ok": true })),
                "console" => {
                    let t = pve(server, Method::POST, &format!("{b}/vncproxy"), Some(json!({ "websocket": 1 }))).await?;
                    let console = if kind(svc) == "lxc" { "lxc" } else { "kvm" };
                    let url = format!("{}/?console={console}&novnc=1&node={}&vmid={}", server.base_url.trim_end_matches('/'), node(server, svc), remote_id(svc)?);
                    Ok(json!({ "url": url, "port": t.get("port"), "ticket": t.get("ticket") }))
                }
                "change_password" => {
                    let pw = new_password(params)?;
                    if kind(svc) == "lxc" {
                        return Err(ProvisionError::Config("password change for LXC needs the guest agent; use the console".into()));
                    }
                    pve(server, Method::PUT, &format!("{b}/config"), Some(json!({ "cipassword": pw }))).await?;
                    Ok(json!({ "password": pw }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }

    fn usage<'a>(&'a self, server: &'a ServerConn, svc: &'a ServiceCtx) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move {
            let _ = md(svc, "node").ok();
            let s = pve(server, Method::GET, &format!("{}/status/current", base(server, svc)?), None).await?;
            Ok(Some(json!({
                "status": s.get("status"), "cpu": s.get("cpu"), "mem": s.get("mem"), "maxmem": s.get("maxmem"),
                "disk": s.get("disk"), "maxdisk": s.get("maxdisk"), "netin": s.get("netin"), "netout": s.get("netout"), "uptime": s.get("uptime"),
            })))
        })
    }
}
