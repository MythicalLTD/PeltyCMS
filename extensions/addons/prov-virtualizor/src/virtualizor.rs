//! VPS control panels: Virtualizor (admin API, `index.php?act=…&api=json`)
//! and SolusVM v1 (admin API, `/api/admin/command.php`, `rdtype=json`).
//! Credentials: the server "API key" field holds `key:pass` for Virtualizor
//! and `id:key` for SolusVM.
use pelty_provisioning::http::call_raw;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::{form, gen_password, hostname, new_password, num_str, str_or};
use pelty_provisioning::{
    cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError, ProvisionResult,
    ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

const FORM: &str = "application/x-www-form-urlencoded";

fn split_key(server: &ServerConn) -> ProvisionResult<(&str, &str)> {
    server.api_key.split_once(':').ok_or_else(|| {
        ProvisionError::Config(
            "API key must be `key:pass` (Virtualizor) or `id:key` (SolusVM)".into(),
        )
    })
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
pub struct Virtualizor;

async fn vz(
    server: &ServerConn,
    act: &str,
    query: &[(&str, &str)],
    post: &[(&str, &str)],
) -> ProvisionResult<Value> {
    let (key, pass) = split_key(server)?;
    let mut q = vec![
        ("act", act),
        ("api", "json"),
        ("adminapikey", key),
        ("adminapipass", pass),
    ];
    q.extend_from_slice(query);
    let body = if post.is_empty() {
        None
    } else {
        Some((FORM, form(post)))
    };
    let r = call_raw(
        server,
        Method::POST,
        &format!("/index.php?{}", form(&q)),
        &[("accept", "application/json")],
        body,
    )
    .await?;
    if let Some(e) = r.get("error").filter(|e| {
        !e.is_null()
            && e.as_array().is_none_or(|a| !a.is_empty())
            && e.as_object().is_none_or(|o| !o.is_empty())
    }) {
        return Err(ProvisionError::Rejected {
            status: 422,
            message: e.to_string().chars().take(300).collect(),
        });
    }
    Ok(r)
}

impl ProvisioningModule for Virtualizor {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "virtualizor",
            name: "Virtualizor",
            server_fields: vec![],
            product_fields: vec![
                fld!("virt", "text", true, "kvm"),
                fld!("osid", "number", true),
                fld!("plid", "number", false),
                fld!("serid", "number", false, 0),
                fld!("ram", "number", true, 1024),
                fld!("space", "number", true, 20),
                fld!("cores", "number", true, 1),
                fld!("bandwidth", "number", false, 0),
                fld!("num_ips", "number", false, 1),
            ],
            actions: vec![],
        }
    }

    fn custom_actions(&self) -> &'static [&'static str] {
        &[
            "start",
            "stop",
            "reboot",
            "poweroff",
            "change_password",
            "console",
        ]
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            vz(server, "vs", &[("page", "1"), ("reslen", "1")], &[])
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
            let (pw, upw, host) = (gen_password(16), gen_password(16), hostname(svc));
            let (osid, ram, space, cores) = (
                num_str(c, "osid")?,
                num_str(c, "ram")?,
                num_str(c, "space")?,
                num_str(c, "cores")?,
            );
            let bw = cfg_u64_or(c, "bandwidth", 0)?.to_string();
            let ips = cfg_u64_or(c, "num_ips", 1)?.to_string();
            let serid = cfg_u64_or(c, "serid", 0)?.to_string();
            let mut post = vec![
                ("addvps", "1"),
                ("virt", str_or(c, "virt", "kvm")),
                ("user_email", svc.customer_email.as_str()),
                ("user_pass", upw.as_str()),
                ("hostname", host.as_str()),
                ("rootpass", pw.as_str()),
                ("osid", osid.as_str()),
                ("ram", ram.as_str()),
                ("space", space.as_str()),
                ("cores", cores.as_str()),
                ("bandwidth", bw.as_str()),
                ("num_ips", ips.as_str()),
                ("serid", serid.as_str()),
            ];
            let plid = c
                .get("plid")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
                .map(|n| n.to_string());
            if let Some(p) = plid.as_deref() {
                post.push(("plid", p));
            }
            let r = vz(server, "addvs", &[], &post).await?;
            let id = r
                .pointer("/done/vpsid")
                .or_else(|| r.pointer("/vs_info/vpsid"))
                .map(|v| v.to_string().trim_matches('"').to_string())
                .filter(|s| !s.is_empty() && s != "null")
                .ok_or_else(|| ProvisionError::Transient("Virtualizor returned no vpsid".into()))?;
            Ok(Created {
                remote_id: id,
                module_data: json!({ "hostname": host, "initial_password": pw, "ip": r.pointer("/vs_info/ips/0").cloned() }),
            })
        })
    }

    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            vz(server, "vs", &[("suspend", num_id(svc)?)], &[])
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
            vz(server, "vs", &[("unsuspend", num_id(svc)?)], &[])
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
            vz(server, "vs", &[("delete", num_id(svc)?)], &[])
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
            let (ram, space, cores) = (
                num_str(c, "ram")?,
                num_str(c, "space")?,
                num_str(c, "cores")?,
            );
            let bw = cfg_u64_or(c, "bandwidth", 0)?.to_string();
            vz(
                server,
                "editvs",
                &[("vpsid", num_id(svc)?)],
                &[
                    ("editvps", "1"),
                    ("ram", &ram),
                    ("space", &space),
                    ("cores", &cores),
                    ("bandwidth", &bw),
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
            let id = num_id(svc)?;
            match action {
                "start" | "stop" | "poweroff" => {
                    vz(server, "vs", &[("action", action), ("vpsid", id)], &[])
                        .await
                        .map(|_| json!({ "ok": true }))
                }
                "reboot" => vz(server, "vs", &[("action", "restart"), ("vpsid", id)], &[])
                    .await
                    .map(|_| json!({ "ok": true })),
                "change_password" => {
                    let pw = new_password(params)?;
                    vz(
                        server,
                        "editvs",
                        &[("vpsid", id)],
                        &[("editvps", "1"), ("rootpass", &pw)],
                    )
                    .await?;
                    Ok(json!({ "password": pw }))
                }
                "console" => {
                    let r = vz(server, "vnc", &[("novnc", id)], &[]).await?;
                    Ok(
                        json!({ "url": r.pointer("/info/novnc_url").or_else(|| r.get("novnc")), "password": r.pointer("/info/password") }),
                    )
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
