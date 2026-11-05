//! New provisioning modules against in-process mock servers (never a real
//! panel or cloud). Each test asserts the exact requests sent.
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Router;
use pelty_provisioning::{run, run_custom, ProvisionError, ServerConn, ServiceCtx};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
struct Req {
    method: String,
    path: String,
    body: String,
    headers: HeaderMap,
}

#[derive(Clone, Default)]
struct Mock {
    log: Arc<Mutex<Vec<Req>>>,
    routes: Arc<Vec<(String, String, u16, Value)>>,
}

async fn handler(
    State(m): State<Mock>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let path = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    m.log.lock().unwrap().push(Req {
        method: method.to_string(),
        path: path.clone(),
        body: String::from_utf8_lossy(&body).into_owned(),
        headers,
    });
    for (rm, rp, st, rb) in m.routes.iter() {
        if rm == method.as_str() && path.starts_with(rp.as_str()) {
            return (StatusCode::from_u16(*st).unwrap(), axum::Json(rb.clone())).into_response();
        }
    }
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({"message": "no route"})),
    )
        .into_response()
}

async fn start(routes: Vec<(&str, &str, u16, Value)>) -> (Mock, String) {
    let m = Mock {
        log: Default::default(),
        routes: Arc::new(
            routes
                .into_iter()
                .map(|(a, b, c, d)| (a.to_string(), b.to_string(), c, d))
                .collect(),
        ),
    };
    let app = Router::new().fallback(handler).with_state(m.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (m, format!("http://{addr}"))
}

fn conn(url: &str, key: &str, config: Value) -> ServerConn {
    ServerConn {
        base_url: url.into(),
        api_key: key.into(),
        config,
    }
}

fn svc(config: Value, remote: Option<&str>, data: Value) -> ServiceCtx {
    ServiceCtx {
        service_id: "0b7a5f0e-1111-2222-3333-444455556666".into(),
        customer_email: "alice@example.com".into(),
        customer_first_name: "Alice".into(),
        customer_last_name: "Doe".into(),
        product_config: config,
        custom_fields: json!({ "domain": "alice.example" }),
        remote_id: remote.map(str::to_string),
        module_data: data,
    }
}

fn reqs(m: &Mock) -> Vec<Req> {
    m.log.lock().unwrap().clone()
}

fn find<'a>(r: &'a [Req], method: &str, prefix: &str) -> &'a Req {
    r.iter()
        .find(|x| x.method == method && x.path.starts_with(prefix))
        .unwrap_or_else(|| {
            panic!(
                "no {method} {prefix} in {:?}",
                r.iter()
                    .map(|x| format!("{} {}", x.method, x.path))
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn registry_lists_all_new_modules_with_actions() {
    let reg = pelty_addon_tests_provisioning::builtins();
    let ids: Vec<&str> = reg.list().iter().map(|m| m.id).collect();
    for id in [
        "proxmox",
        "cpanel",
        "plesk",
        "directadmin",
        "cyberpanel",
        "hestia",
        "ispconfig",
        "hetzner_cloud",
        "digitalocean",
        "vultr",
        "linode",
        "virtualizor",
        "solusvm",
        "amp",
        "docker",
        "openstack",
        "ssl_reseller",
        "licensing",
        "pterodactyl",
        "pelican",
    ] {
        assert!(ids.contains(&id), "missing {id}");
    }
    let px = reg.list().into_iter().find(|m| m.id == "proxmox").unwrap();
    assert!(px.actions.contains(&"console"));
    for m in reg.list() {
        for f in m.server_fields.iter().chain(m.product_fields.iter()) {
            assert!(
                f.label.starts_with("provisioning.field."),
                "{} {}",
                m.id,
                f.key
            );
        }
    }
}

#[tokio::test]
async fn proxmox_lxc_lifecycle_and_console() {
    let (m, url) = start(vec![
        (
            "GET",
            "/api2/json/version",
            200,
            json!({"data": {"version": "8.2"}}),
        ),
        (
            "GET",
            "/api2/json/cluster/nextid",
            200,
            json!({"data": "105"}),
        ),
        (
            "POST",
            "/api2/json/nodes/pve1/lxc/105/status",
            200,
            json!({"data": "UPID:x"}),
        ),
        (
            "POST",
            "/api2/json/nodes/pve1/lxc/105/vncproxy",
            200,
            json!({"data": {"port": "5900", "ticket": "PVEVNC:abc"}}),
        ),
        (
            "POST",
            "/api2/json/nodes/pve1/lxc",
            200,
            json!({"data": "UPID:create"}),
        ),
        (
            "PUT",
            "/api2/json/nodes/pve1/lxc/105/",
            200,
            json!({"data": null}),
        ),
        (
            "DELETE",
            "/api2/json/nodes/pve1/lxc/105",
            200,
            json!({"data": "UPID:del"}),
        ),
        (
            "GET",
            "/api2/json/nodes/pve1/lxc/105/status/current",
            200,
            json!({"data": {"status": "running", "cpu": 0.1, "mem": 100, "maxmem": 1000}}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("proxmox").unwrap();
    let c = conn(&url, "root@pam!pelty=secret-uuid", json!({"node": "pve1"}));
    p.test_connection(&c).await.unwrap();
    let cfg = json!({"type": "lxc", "ostemplate": "local:vztmpl/debian-12.tar.zst", "storage": "local-lvm", "cores": 2, "memory": "2048", "disk": 20});
    let created = run(p.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created.remote_id, "105");
    assert_eq!(created.module_data["node"], "pve1");
    assert_eq!(created.module_data["type"], "lxc");
    let r = reqs(&m);
    let mk = find(&r, "POST", "/api2/json/nodes/pve1/lxc");
    assert_eq!(
        mk.headers["authorization"],
        "PVEAPIToken=root@pam!pelty=secret-uuid"
    );
    let b: Value = serde_json::from_str(&mk.body).unwrap();
    assert_eq!(b["vmid"], 105);
    assert_eq!(b["rootfs"], "local-lvm:20");
    assert_eq!(b["memory"], 2048);
    assert_eq!(b["cores"], 2);
    assert_eq!(b["hostname"], "alice.example");
    let s = svc(cfg.clone(), Some("105"), created.module_data.clone());
    run(p.as_ref(), "suspend", &c, &s).await.unwrap();
    run(p.as_ref(), "unsuspend", &c, &s).await.unwrap();
    let out = run_custom(p.as_ref(), "console", &c, &s, &json!({}))
        .await
        .unwrap();
    assert!(
        out["url"].as_str().unwrap().contains("console=lxc")
            && out["url"].as_str().unwrap().contains("vmid=105")
    );
    let usage = p.usage(&c, &s).await.unwrap().unwrap();
    assert_eq!(usage["status"], "running");
    run(p.as_ref(), "terminate", &c, &s).await.unwrap();
    let r = reqs(&m);
    assert!(r
        .iter()
        .any(|x| x.path == "/api2/json/nodes/pve1/lxc/105/status/stop"));
    assert!(r
        .iter()
        .any(|x| x.path == "/api2/json/nodes/pve1/lxc/105/status/start"));
    assert!(r.iter().any(
        |x| x.method == "DELETE" && x.path.starts_with("/api2/json/nodes/pve1/lxc/105?purge=1")
    ));
    // unknown / undeclared custom action is refused before any request
    let n = reqs(&m).len();
    assert!(matches!(
        run_custom(p.as_ref(), "rm_rf", &c, &s, &json!({})).await,
        Err(ProvisionError::Config(_))
    ));
    assert_eq!(reqs(&m).len(), n);
}

#[tokio::test]
async fn proxmox_qemu_clone_resize_and_package_change() {
    let (m, url) = start(vec![
        (
            "GET",
            "/api2/json/cluster/nextid",
            200,
            json!({"data": 200}),
        ),
        (
            "POST",
            "/api2/json/nodes/pve/qemu/9000/clone",
            200,
            json!({"data": "UPID"}),
        ),
        (
            "PUT",
            "/api2/json/nodes/pve/qemu/200/",
            200,
            json!({"data": null}),
        ),
        (
            "POST",
            "/api2/json/nodes/pve/qemu/200/status/start",
            200,
            json!({"data": "UPID"}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("proxmox").unwrap();
    let c = conn(&url, "t", json!({}));
    let cfg = json!({"type": "qemu", "template_vmid": 9000, "storage": "ceph", "cores": 4, "memory": 4096, "disk": 80});
    let created = run(p.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created.remote_id, "200");
    let r = reqs(&m);
    let clone: Value =
        serde_json::from_str(&find(&r, "POST", "/api2/json/nodes/pve/qemu/9000/clone").body)
            .unwrap();
    assert_eq!(clone["newid"], 200);
    let resize: Value =
        serde_json::from_str(&find(&r, "PUT", "/api2/json/nodes/pve/qemu/200/resize").body)
            .unwrap();
    assert_eq!(resize["size"], "80G");
    let mut bigger = cfg.clone();
    bigger["memory"] = json!(8192);
    bigger["disk"] = json!(160);
    run(
        p.as_ref(),
        "change_package",
        &c,
        &svc(bigger, Some("200"), created.module_data),
    )
    .await
    .unwrap();
    let r = reqs(&m);
    let cfgs: Vec<Value> = r
        .iter()
        .filter(|x| x.method == "PUT" && x.path.ends_with("/config"))
        .map(|x| serde_json::from_str(&x.body).unwrap())
        .collect();
    assert_eq!(cfgs.last().unwrap()["memory"], 8192);
    assert!(r
        .iter()
        .filter(|x| x.path.ends_with("/resize"))
        .any(|x| x.body.contains("160G")));
    // a non-numeric vmid is refused (no path injection)
    assert!(matches!(
        run(
            p.as_ref(),
            "suspend",
            &c,
            &svc(cfg, Some("1/../../x"), json!({}))
        )
        .await,
        Err(ProvisionError::Config(_))
    ));
}

#[tokio::test]
async fn cpanel_whm_lifecycle_sso_and_errors() {
    let ok = json!({"metadata": {"result": 1, "reason": "OK"}, "data": {}});
    let (m, url) = start(vec![
        ("GET", "/json-api/version", 200, ok.clone()),
        ("GET", "/json-api/createacct", 200, ok.clone()),
        ("GET", "/json-api/suspendacct", 200, ok.clone()),
        ("GET", "/json-api/unsuspendacct", 200, ok.clone()),
        ("GET", "/json-api/changepackage", 200, json!({"metadata": {"result": 0, "reason": "Package does not exist"}})),
        ("GET", "/json-api/passwd", 200, ok.clone()),
        ("GET", "/json-api/create_user_session", 200, json!({"metadata": {"result": 1}, "data": {"url": "https://h:2083/cpsess1/login/?session=x"}})),
        ("GET", "/json-api/removeacct", 200, ok.clone()),
    ]).await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("cpanel").unwrap();
    let c = conn(&url, "TOKEN123", json!({"username": "root"}));
    p.test_connection(&c).await.unwrap();
    let cfg = json!({"package": "starter"});
    let created = run(p.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created.remote_id, "alic0b7a");
    let r = reqs(&m);
    let ca = find(&r, "GET", "/json-api/createacct");
    assert_eq!(ca.headers["authorization"], "whm root:TOKEN123");
    assert!(
        ca.path.contains("username=alic0b7a")
            && ca.path.contains("domain=alice.example")
            && ca.path.contains("plan=starter")
            && ca.path.contains("contactemail=alice%40example.com"),
        "{}",
        ca.path
    );
    let s = svc(cfg, Some("alic0b7a"), created.module_data);
    run(p.as_ref(), "suspend", &c, &s).await.unwrap();
    run(p.as_ref(), "unsuspend", &c, &s).await.unwrap();
    let e = run(p.as_ref(), "change_package", &c, &s).await.unwrap_err();
    assert!(
        matches!(e, ProvisionError::Rejected { .. })
            && e.to_string().contains("Package does not exist"),
        "{e}"
    );
    let pw = run_custom(
        p.as_ref(),
        "change_password",
        &c,
        &s,
        &json!({"password": "N3w-Passw0rd!"}),
    )
    .await
    .unwrap();
    assert_eq!(pw["password"], "N3w-Passw0rd!");
    assert!(run_custom(
        p.as_ref(),
        "change_password",
        &c,
        &s,
        &json!({"password": "short"})
    )
    .await
    .is_err());
    let sso = p.sso_url(&c, &s, "tok").await.unwrap().unwrap();
    assert!(sso.contains("cpsess1"));
    run(p.as_ref(), "terminate", &c, &s).await.unwrap();
    assert!(reqs(&m)
        .iter()
        .any(|x| x.path.starts_with("/json-api/removeacct") && x.path.contains("user=alic0b7a")));
}

#[tokio::test]
async fn plesk_creates_client_and_subscription() {
    let (m, url) = start(vec![
        ("GET", "/api/v2/server", 200, json!({"platform": "Unix"})),
        (
            "POST",
            "/api/v2/clients",
            201,
            json!({"id": 12, "guid": "g"}),
        ),
        (
            "POST",
            "/api/v2/domains",
            201,
            json!({"id": 34, "guid": "d"}),
        ),
        (
            "POST",
            "/api/v2/cli/subscription/call",
            200,
            json!({"code": 0, "stdout": "ok", "stderr": ""}),
        ),
        ("POST", "/api/v2/cli/customer/call", 200, json!({"code": 0})),
        ("DELETE", "/api/v2/domains/34", 200, json!({})),
        (
            "DELETE",
            "/api/v2/clients/12",
            404,
            json!({"message": "gone"}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("plesk").unwrap();
    let c = conn(&url, "plesk-key", json!({}));
    p.test_connection(&c).await.unwrap();
    let created = run(
        p.as_ref(),
        "create",
        &c,
        &svc(json!({"plan": "Unlimited"}), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(created.module_data["client_id"], 12);
    assert_eq!(created.module_data["domain_id"], 34);
    let r = reqs(&m);
    assert_eq!(
        find(&r, "POST", "/api/v2/clients").headers["x-api-key"],
        "plesk-key"
    );
    let d: Value = serde_json::from_str(&find(&r, "POST", "/api/v2/domains").body).unwrap();
    assert_eq!(d["owner_client"]["id"], 12);
    assert_eq!(d["plan"]["name"], "Unlimited");
    let s = svc(
        json!({"plan": "Pro"}),
        Some(&created.remote_id),
        created.module_data,
    );
    run(p.as_ref(), "suspend", &c, &s).await.unwrap();
    run(p.as_ref(), "change_package", &c, &s).await.unwrap();
    let r = reqs(&m);
    let cli: Vec<&Req> = r
        .iter()
        .filter(|x| x.path == "/api/v2/cli/subscription/call")
        .collect();
    assert!(cli[0].body.contains("--webspace-off") && cli[0].body.contains("alice.example"));
    assert!(cli[1].body.contains("--switch-subscription") && cli[1].body.contains("Pro"));
    // client already deleted (404) still counts as terminated
    run(p.as_ref(), "terminate", &c, &s).await.unwrap();
}

#[tokio::test]
async fn directadmin_cyberpanel_hestia_ispconfig() {
    let (m, url) = start(vec![
        (
            "POST",
            "/CMD_API_ACCOUNT_USER",
            200,
            json!({"error": "0", "text": "User created"}),
        ),
        ("POST", "/CMD_API_SELECT_USERS", 200, json!({"error": "0"})),
        (
            "POST",
            "/CMD_API_MODIFY_USER",
            200,
            json!({"error": "1", "text": "bad package", "details": "Package xl does not exist"}),
        ),
        (
            "POST",
            "/api/createWebsite",
            200,
            json!({"createWebSiteStatus": 1}),
        ),
        (
            "POST",
            "/api/submitWebsiteStatus",
            200,
            json!({"websiteStatus": 1}),
        ),
        (
            "POST",
            "/api/deleteWebsite",
            200,
            json!({"websiteDeleteStatus": 0, "error_message": "nope"}),
        ),
        ("POST", "/api/index.php", 200, json!(0)),
        (
            "POST",
            "/remote/json.php?login",
            200,
            json!({"code": "ok", "response": "sess-1"}),
        ),
        (
            "POST",
            "/remote/json.php?client_add",
            200,
            json!({"code": "ok", "response": 7}),
        ),
        (
            "POST",
            "/remote/json.php?sites_web_domain_add",
            200,
            json!({"code": "ok", "response": 9}),
        ),
        (
            "POST",
            "/remote/json.php?sites_web_domain_get",
            200,
            json!({"code": "ok", "response": {"domain": "alice.example", "active": "y"}}),
        ),
        (
            "POST",
            "/remote/json.php?sites_web_domain_update",
            200,
            json!({"code": "ok", "response": 1}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    // DirectAdmin (basic auth, form body)
    let da = reg.get("directadmin").unwrap();
    let c = conn(&url, "loginkey", json!({"username": "admin"}));
    let cfg = json!({"package": "basic", "ip": "10.0.0.5"});
    let created = run(
        da.as_ref(),
        "create",
        &c,
        &svc(cfg.clone(), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    let r = reqs(&m);
    let a = find(&r, "POST", "/CMD_API_ACCOUNT_USER");
    assert_eq!(a.headers["authorization"], "Basic YWRtaW46bG9naW5rZXk=");
    assert!(
        a.body.contains("action=create")
            && a.body.contains("package=basic")
            && a.body.contains("ip=10.0.0.5")
    );
    let s = svc(
        json!({"package": "xl"}),
        Some(&created.remote_id),
        created.module_data,
    );
    run(da.as_ref(), "suspend", &c, &s).await.unwrap();
    assert!(reqs(&m)
        .iter()
        .any(|x| x.path.starts_with("/CMD_API_SELECT_USERS") && x.body.contains("dosuspend=yes")));
    let e = run(da.as_ref(), "change_package", &c, &s)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("Package xl does not exist"), "{e}");
    // CyberPanel
    let cp = reg.get("cyberpanel").unwrap();
    let cc = conn(&url, "adminpw", json!({}));
    let made = run(cp.as_ref(), "create", &cc, &svc(json!({}), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    let b: Value =
        serde_json::from_str(&find(&reqs(&m), "POST", "/api/createWebsite").body).unwrap();
    assert_eq!(b["adminPass"], "adminpw");
    assert_eq!(b["domainName"], "alice.example");
    let s = svc(json!({}), Some(&made.remote_id), made.module_data);
    run(cp.as_ref(), "suspend", &cc, &s).await.unwrap();
    assert!(run(cp.as_ref(), "terminate", &cc, &s)
        .await
        .unwrap_err()
        .to_string()
        .contains("nope"));
    // Hestia
    let h = reg.get("hestia").unwrap();
    let hc = conn(&url, "access:secret", json!({}));
    run(
        h.as_ref(),
        "create",
        &hc,
        &svc(json!({"package": "default"}), None, json!({})),
    )
    .await
    .unwrap();
    let hb: Vec<String> = reqs(&m)
        .iter()
        .filter(|x| x.path == "/api/index.php")
        .map(|x| x.body.clone())
        .collect();
    assert!(
        hb[0].contains("cmd=v-add-user")
            && hb[0].contains("hash=access%3Asecret")
            && hb[0].contains("returncode=yes")
    );
    assert!(hb[1].contains("cmd=v-add-domain") && hb[1].contains("arg2=alice.example"));
    // ISPConfig
    let i = reg.get("ispconfig").unwrap();
    let ic = conn(&url, "rpw", json!({"username": "remote"}));
    let made = run(
        i.as_ref(),
        "create",
        &ic,
        &svc(json!({"server_id": 1}), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(made.module_data["client_id"], "7");
    assert_eq!(made.module_data["domain_id"], "9");
    let s = svc(
        json!({"server_id": 1}),
        Some(&made.remote_id),
        made.module_data,
    );
    run(i.as_ref(), "suspend", &ic, &s).await.unwrap();
    let up: Value = serde_json::from_str(
        &find(
            &reqs(&m),
            "POST",
            "/remote/json.php?sites_web_domain_update",
        )
        .body,
    )
    .unwrap();
    assert_eq!(up["params"]["active"], "n");
    assert_eq!(up["session_id"], "sess-1");
}

#[tokio::test]
async fn cloud_providers_full_lifecycle() {
    let (m, url) = start(vec![
        // Hetzner
        (
            "POST",
            "/v1/servers/41/actions/request_console",
            201,
            json!({"wss_url": "wss://console", "password": "vnc"}),
        ),
        (
            "POST",
            "/v1/servers/41/actions/reset_password",
            201,
            json!({"root_password": "newroot"}),
        ),
        (
            "POST",
            "/v1/servers/41/actions",
            201,
            json!({"action": {"id": 1}}),
        ),
        (
            "POST",
            "/v1/servers",
            201,
            json!({"server": {"id": 41}, "root_password": "r00t"}),
        ),
        ("DELETE", "/v1/servers/41", 200, json!({})),
        (
            "GET",
            "/v1/servers/41",
            200,
            json!({"server": {"status": "running", "public_net": {"ipv4": {"ip": "1.2.3.4"}}}}),
        ),
        // DigitalOcean
        (
            "POST",
            "/v2/droplets/52/actions",
            201,
            json!({"action": {"id": 2}}),
        ),
        ("POST", "/v2/droplets", 202, json!({"droplet": {"id": 52}})),
        (
            "DELETE",
            "/v2/droplets/52",
            404,
            json!({"message": "not found"}),
        ),
        // Vultr
        (
            "POST",
            "/v2/instances",
            202,
            json!({"instance": {"id": "ab-cd", "default_password": "vp"}}),
        ),
        ("PATCH", "/v2/instances/ab-cd", 202, json!({})),
        // Linode
        ("POST", "/v4/linode/instances/63/", 200, json!({})),
        ("POST", "/v4/linode/instances", 200, json!({"id": 63})),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let c = conn(&url, "cloudtoken", json!({}));
    let cfg = json!({"plan": "cx22", "region": "fsn1", "image": "debian-12"});
    // Hetzner
    let h = reg.get("hetzner_cloud").unwrap();
    let made = run(h.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(made.remote_id, "41");
    assert_eq!(made.module_data["initial_password"], "r00t");
    let r = reqs(&m);
    let cr = find(&r, "POST", "/v1/servers");
    assert_eq!(cr.headers["authorization"], "Bearer cloudtoken");
    let b: Value = serde_json::from_str(&cr.body).unwrap();
    assert_eq!(
        (
            b["server_type"].as_str(),
            b["location"].as_str(),
            b["image"].as_str()
        ),
        (Some("cx22"), Some("fsn1"), Some("debian-12"))
    );
    let s = svc(
        json!({"plan": "cx32", "region": "fsn1", "image": "debian-12"}),
        Some("41"),
        made.module_data,
    );
    run(h.as_ref(), "suspend", &c, &s).await.unwrap();
    run(h.as_ref(), "change_package", &c, &s).await.unwrap();
    let paths: Vec<String> = reqs(&m).iter().map(|x| x.path.clone()).collect();
    assert!(paths.contains(&"/v1/servers/41/actions/poweroff".to_string()));
    assert!(paths.contains(&"/v1/servers/41/actions/change_type".to_string()));
    assert!(paths.contains(&"/v1/servers/41/actions/poweron".to_string()));
    assert_eq!(
        run_custom(h.as_ref(), "console", &c, &s, &json!({}))
            .await
            .unwrap()["url"],
        "wss://console"
    );
    assert_eq!(
        run_custom(h.as_ref(), "reset_password", &c, &s, &json!({}))
            .await
            .unwrap()["password"],
        "newroot"
    );
    assert_eq!(h.usage(&c, &s).await.unwrap().unwrap()["ipv4"], "1.2.3.4");
    run(h.as_ref(), "terminate", &c, &s).await.unwrap();
    // DigitalOcean
    let d = reg.get("digitalocean").unwrap();
    let made = run(d.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    let s = svc(cfg.clone(), Some(&made.remote_id), made.module_data);
    run(d.as_ref(), "suspend", &c, &s).await.unwrap();
    run_custom(d.as_ref(), "reboot", &c, &s, &json!({}))
        .await
        .unwrap();
    let acts: Vec<Value> = reqs(&m)
        .iter()
        .filter(|x| x.path == "/v2/droplets/52/actions")
        .map(|x| serde_json::from_str(&x.body).unwrap())
        .collect();
    assert_eq!(acts[0]["type"], "power_off");
    assert_eq!(acts[1]["type"], "reboot");
    run(d.as_ref(), "terminate", &c, &s).await.unwrap(); // 404 = already gone
                                                         // Vultr: numeric image → os_id
    let v = reg.get("vultr").unwrap();
    let made = run(
        v.as_ref(),
        "create",
        &c,
        &svc(
            json!({"plan": "vc2-1c-1gb", "region": "fra", "image": "2136"}),
            None,
            json!({}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(made.remote_id, "ab-cd");
    let vb: Value = serde_json::from_str(&find(&reqs(&m), "POST", "/v2/instances").body).unwrap();
    assert_eq!(vb["os_id"], 2136);
    run(
        v.as_ref(),
        "change_package",
        &c,
        &svc(json!({"plan": "vc2-2c-4gb"}), Some("ab-cd"), json!({})),
    )
    .await
    .unwrap();
    assert!(find(&reqs(&m), "PATCH", "/v2/instances/ab-cd")
        .body
        .contains("vc2-2c-4gb"));
    // Linode
    let l = reg.get("linode").unwrap();
    let made = run(l.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(made.remote_id, "63");
    assert!(made.module_data["initial_password"].as_str().unwrap().len() >= 24);
    let s = svc(cfg, Some("63"), made.module_data);
    run(l.as_ref(), "suspend", &c, &s).await.unwrap();
    run(l.as_ref(), "change_package", &c, &s).await.unwrap();
    let paths: Vec<String> = reqs(&m).iter().map(|x| x.path.clone()).collect();
    assert!(paths.contains(&"/v4/linode/instances/63/shutdown".to_string()));
    assert!(paths.contains(&"/v4/linode/instances/63/resize".to_string()));
    // a remote id with a slash is refused
    assert!(matches!(
        run(
            l.as_ref(),
            "terminate",
            &c,
            &svc(json!({}), Some("63/../x"), json!({}))
        )
        .await,
        Err(ProvisionError::Config(_))
    ));
}

#[tokio::test]
async fn virtualizor_and_solusvm() {
    let (m, url) = start(vec![
        (
            "POST",
            "/index.php?act=addvs",
            200,
            json!({"done": {"vpsid": 88}, "vs_info": {"vpsid": 88, "ips": ["5.6.7.8"]}}),
        ),
        ("POST", "/index.php?act=vs", 200, json!({"done": 1})),
        ("POST", "/index.php?act=editvs", 200, json!({"done": 1})),
        (
            "POST",
            "/index.php?act=vnc",
            200,
            json!({"info": {"novnc_url": "https://vnc", "password": "p"}}),
        ),
        (
            "POST",
            "/api/admin/command.php",
            200,
            json!({"status": "success", "vserverid": "17", "mainipaddress": "9.9.9.9"}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let v = reg.get("virtualizor").unwrap();
    let c = conn(&url, "akey:apass", json!({}));
    let cfg = json!({"virt": "kvm", "osid": 100, "ram": 2048, "space": 40, "cores": 2});
    let made = run(v.as_ref(), "create", &c, &svc(cfg.clone(), None, json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(made.remote_id, "88");
    let r = reqs(&m);
    let a = find(&r, "POST", "/index.php?act=addvs");
    assert!(a.path.contains("adminapikey=akey") && a.path.contains("adminapipass=apass"));
    assert!(
        a.body.contains("ram=2048")
            && a.body.contains("osid=100")
            && a.body.contains("user_email=alice%40example.com")
    );
    let s = svc(cfg, Some("88"), made.module_data);
    run(v.as_ref(), "suspend", &c, &s).await.unwrap();
    run(v.as_ref(), "change_package", &c, &s).await.unwrap();
    assert_eq!(
        run_custom(v.as_ref(), "console", &c, &s, &json!({}))
            .await
            .unwrap()["url"],
        "https://vnc"
    );
    assert!(reqs(&m).iter().any(|x| x.path.contains("suspend=88")));
    // SolusVM
    let sv = reg.get("solusvm").unwrap();
    let sc = conn(&url, "apiid:apikey", json!({}));
    let made = run(
        sv.as_ref(),
        "create",
        &sc,
        &svc(
            json!({"plan": "vps1", "template": "debian-12"}),
            None,
            json!({}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(made.remote_id, "17");
    let bodies: Vec<String> = reqs(&m)
        .iter()
        .filter(|x| x.path == "/api/admin/command.php")
        .map(|x| x.body.clone())
        .collect();
    assert!(bodies[0].contains("action=client-create"));
    assert!(
        bodies[1].contains("action=vserver-create")
            && bodies[1].contains("plan=vps1")
            && bodies[1].contains("id=apiid")
            && bodies[1].contains("rdtype=json")
    );
    run(
        sv.as_ref(),
        "terminate",
        &sc,
        &svc(json!({}), Some("17"), json!({})),
    )
    .await
    .unwrap();
    assert!(reqs(&m)
        .iter()
        .any(|x| x.body.contains("action=vserver-terminate") && x.body.contains("vserverid=17")));
    // bad key format
    assert!(matches!(
        v.test_connection(&conn(&url, "nocolon", json!({}))).await,
        Err(ProvisionError::Config(_))
    ));
}

#[tokio::test]
async fn amp_docker_openstack_ssl() {
    let (m, url) = start(vec![
        (
            "POST",
            "/API/Core/Login",
            200,
            json!({"success": true, "sessionID": "S1"}),
        ),
        (
            "POST",
            "/API/ADSModule/DeployTemplate",
            200,
            json!({"Status": true, "Result": {"InstanceName": "Minecraft01"}}),
        ),
        (
            "POST",
            "/API/ADSModule/StopInstance",
            200,
            json!({"Status": true}),
        ),
        (
            "POST",
            "/API/ADSModule/StartInstance",
            200,
            json!({"Status": false, "Reason": "Instance missing"}),
        ),
        (
            "POST",
            "/v1.43/containers/create",
            201,
            json!({"Id": "abc123def"}),
        ),
        ("POST", "/v1.43/containers/abc123def/", 204, Value::Null),
        ("DELETE", "/v1.43/containers/abc123def", 204, Value::Null),
        ("POST", "/servers/1b2c-3d/action", 202, Value::Null),
        (
            "POST",
            "/servers/1b2c-3d/remote-consoles",
            200,
            json!({"remote_console": {"url": "https://nova/vnc"}}),
        ),
        (
            "POST",
            "/servers",
            202,
            json!({"server": {"id": "1b2c-3d", "adminPass": "ap"}}),
        ),
        ("POST", "/api/v1/orders/77/resend", 200, json!({})),
        (
            "GET",
            "/api/v1/orders/77",
            200,
            json!({"status": "pending_validation", "dcv": {"method": "email"}}),
        ),
        (
            "POST",
            "/api/v1/orders",
            201,
            json!({"id": 77, "status": "pending_validation"}),
        ),
        ("POST", "/api/v1/orders/77/cancel", 200, json!({})),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let c = conn(&url, "k", json!({}));
    // AMP
    let a = reg.get("amp").unwrap();
    let made = run(
        a.as_ref(),
        "create",
        &c,
        &svc(json!({"template_id": 3}), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(made.remote_id, "Minecraft01");
    let dep: Value =
        serde_json::from_str(&find(&reqs(&m), "POST", "/API/ADSModule/DeployTemplate").body)
            .unwrap();
    assert_eq!(dep["SESSIONID"], "S1");
    assert_eq!(dep["TemplateID"], 3);
    let s = svc(json!({}), Some("Minecraft01"), json!({}));
    run(a.as_ref(), "suspend", &c, &s).await.unwrap();
    assert!(run(a.as_ref(), "unsuspend", &c, &s)
        .await
        .unwrap_err()
        .to_string()
        .contains("Instance missing"));
    // Docker
    let d = reg.get("docker").unwrap();
    let made = run(d.as_ref(), "create", &c, &svc(json!({"image": "nginx:1.27", "memory": 256, "cpu": 50, "port": 80, "environment": {"A": "1"}}), None, json!({}))).await.unwrap().unwrap();
    assert_eq!(made.remote_id, "abc123def");
    let cb: Value =
        serde_json::from_str(&find(&reqs(&m), "POST", "/v1.43/containers/create").body).unwrap();
    assert_eq!(cb["HostConfig"]["Memory"], 256u64 * 1024 * 1024);
    assert_eq!(cb["HostConfig"]["NanoCpus"], 500_000_000u64);
    assert_eq!(cb["Env"][0], "A=1");
    let s = svc(
        json!({"image": "nginx", "memory": 512}),
        Some("abc123def"),
        json!({}),
    );
    run(d.as_ref(), "change_package", &c, &s).await.unwrap();
    run(d.as_ref(), "terminate", &c, &s).await.unwrap();
    assert!(reqs(&m)
        .iter()
        .any(|x| x.path == "/v1.43/containers/abc123def/start"));
    // OpenStack
    let o = reg.get("openstack").unwrap();
    let made = run(
        o.as_ref(),
        "create",
        &c,
        &svc(
            json!({"flavor": "m1.small", "image": "img-1"}),
            None,
            json!({}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(made.remote_id, "1b2c-3d");
    assert_eq!(
        find(&reqs(&m), "POST", "/servers").headers["x-auth-token"],
        "k"
    );
    let s = svc(json!({"flavor": "m1.large"}), Some("1b2c-3d"), json!({}));
    run(o.as_ref(), "suspend", &c, &s).await.unwrap();
    run(o.as_ref(), "change_package", &c, &s).await.unwrap();
    let acts: Vec<String> = reqs(&m)
        .iter()
        .filter(|x| x.path == "/servers/1b2c-3d/action")
        .map(|x| x.body.clone())
        .collect();
    assert!(acts[0].contains("suspend") && acts[1].contains("m1.large"));
    assert_eq!(
        run_custom(o.as_ref(), "console", &c, &s, &json!({}))
            .await
            .unwrap()["url"],
        "https://nova/vnc"
    );
    // SSL reseller
    let ssl = reg.get("ssl_reseller").unwrap();
    let made = run(
        ssl.as_ref(),
        "create",
        &c,
        &svc(json!({"ssl_product": "positivessl"}), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(made.remote_id, "77");
    let ob: Value = serde_json::from_str(&find(&reqs(&m), "POST", "/api/v1/orders").body).unwrap();
    assert_eq!(ob["approver_email"], "admin@alice.example");
    let s = svc(json!({}), Some("77"), made.module_data);
    run(ssl.as_ref(), "suspend", &c, &s).await.unwrap(); // no-op, no request
    let st = run_custom(ssl.as_ref(), "status", &c, &s, &json!({}))
        .await
        .unwrap();
    assert_eq!(st["module_data_patch"]["status"], "pending_validation");
    run_custom(ssl.as_ref(), "resend_validation", &c, &s, &json!({}))
        .await
        .unwrap();
    assert!(
        run_custom(ssl.as_ref(), "reissue", &c, &s, &json!({"csr": "nope"}))
            .await
            .is_err()
    );
    run(ssl.as_ref(), "terminate", &c, &s).await.unwrap();
    let mut csvc = svc(json!({"ssl_product": "x"}), None, json!({}));
    csvc.custom_fields["csr"] = json!("not a csr");
    assert!(matches!(
        run(ssl.as_ref(), "create", &c, &csvc).await,
        Err(ProvisionError::Config(_))
    ));
}

#[tokio::test]
async fn licensing_mints_key_and_reissue_clears_binding() {
    let reg = pelty_addon_tests_provisioning::builtins();
    let l = reg.get("licensing").unwrap();
    let c = conn(
        "http://127.0.0.1:1",
        "a-very-long-signing-secret",
        json!({}),
    );
    l.test_connection(&c).await.unwrap();
    assert!(l
        .test_connection(&conn("http://127.0.0.1:1", "short", json!({})))
        .await
        .is_err());
    let made = run(
        l.as_ref(),
        "create",
        &c,
        &svc(json!({"key_prefix": "app"}), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(made.remote_id.starts_with("APP-"));
    let out = run_custom(
        l.as_ref(),
        "reissue",
        &c,
        &svc(
            json!({}),
            Some(&made.remote_id),
            json!({"bound_domain": "a.com"}),
        ),
        &json!({}),
    )
    .await
    .unwrap();
    assert!(out["module_data_patch"]["bound_domain"].is_null());
}
