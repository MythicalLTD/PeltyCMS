//! WASM port of `pelty-provisioning/tests/mock_modules.rs`: identical behaviour
//! tests, but every module runs as its compiled addon inside the sandbox.
//! Pelican, webhook and HTTPS-agent modules against in-process mock servers.
mod common;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Router;
use pelty_provisioning::template::sign;
use pelty_provisioning::{run, ProvisionError, ServerConn, ServiceCtx};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
struct Hit {
    method: String,
    path: String,
    headers: HeaderMap,
    body: String,
}

type Routes = Arc<Vec<(String, String, u16, Value)>>;

#[derive(Clone)]
struct Mock {
    log: Arc<Mutex<Vec<Hit>>>,
    routes: Routes,
}

async fn handler(State(m): State<Mock>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let path = uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    m.log.lock().unwrap().push(Hit { method: method.to_string(), path: path.clone(), headers, body: String::from_utf8_lossy(&body).into_owned() });
    for (rm, rp, st, rb) in m.routes.iter() {
        if rm == method.as_str() && path.starts_with(rp.as_str()) {
            return (StatusCode::from_u16(*st).unwrap(), axum::Json(rb.clone())).into_response();
        }
    }
    (StatusCode::NOT_FOUND, axum::Json(json!({"message": "no route"}))).into_response()
}

async fn start(routes: Vec<(&str, &str, u16, Value)>) -> (Arc<Mutex<Vec<Hit>>>, String) {
    let m = Mock { log: Arc::default(), routes: Arc::new(routes.into_iter().map(|(a, b, c, d)| (a.to_string(), b.to_string(), c, d)).collect()) };
    let app = Router::new().fallback(handler).with_state(m.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (m.log, format!("http://{addr}"))
}

fn svc(config: Value, remote: Option<&str>) -> ServiceCtx {
    ServiceCtx {
        service_id: "0b7a5f0e-1111-2222-3333-444455556666".into(),
        customer_email: "alice@example.com".into(),
        customer_first_name: "Alice".into(),
        customer_last_name: "Doe \"Q\"".into(),
        product_config: config,
        custom_fields: json!({"hostname": "srv one"}),
        remote_id: remote.map(str::to_string),
        module_data: json!({}),
    }
}

fn h(hit: &Hit, k: &str) -> String {
    hit.headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn pelican_full_lifecycle() {
    let (log, url) = start(vec![
        ("GET", "/api/application/users?filter", 200, json!({"data": [{"attributes": {"id": 8}}]})),
        ("POST", "/api/application/servers/12/suspend", 204, Value::Null),
        ("POST", "/api/application/servers/12/unsuspend", 204, Value::Null),
        ("POST", "/api/application/servers", 201, json!({"attributes": {"id": 12, "identifier": "pel1"}})),
        ("GET", "/api/application/servers/12", 200, json!({"attributes": {"id": 12, "allocation": 4}})),
        ("PATCH", "/api/application/servers/12/build", 200, json!({})),
        ("DELETE", "/api/application/servers/12", 404, json!({})),
        ("GET", "/api/application/locations", 200, json!({"data": []})),
    ])
    .await;
    let p = common::wasm_registry().get("pelican").unwrap();
    assert_eq!(p.info().name, "Pelican (Wings)");
    let conn = ServerConn { base_url: url, api_key: "peli_k".into(), config: json!({}) };
    p.test_connection(&conn).await.unwrap();
    let cfg = json!({"egg": 2, "location": 1, "docker_image": "img", "startup": "run", "memory": 1024, "disk": 2048});
    let c = run(p.as_ref(), "create", &conn, &svc(cfg, None)).await.unwrap().unwrap();
    assert_eq!(c.remote_id, "12");
    assert_eq!(c.module_data["user_id"], 8);
    let s = svc(json!({"memory": 2048, "disk": 4096}), Some("12"));
    for a in ["suspend", "unsuspend", "change_package", "terminate"] {
        run(p.as_ref(), a, &conn, &s).await.unwrap_or_else(|e| panic!("{a}: {e}"));
    }
    let log = log.lock().unwrap();
    assert!(log.iter().all(|x| h(x, "accept") == "application/json" && h(x, "authorization") == "Bearer peli_k"));
    assert!(!log.iter().any(|x| x.method == "POST" && x.path == "/api/application/users"), "existing user reused");
    let b: Value = serde_json::from_str(&log.iter().find(|x| x.method == "PATCH").unwrap().body).unwrap();
    assert_eq!(b["allocation"], 4);
    assert_eq!(b["memory"], 2048);
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_templates_and_signs() {
    let (log, url) = start(vec![
        ("POST", "/hooks/create", 200, json!({"data": {"vm": 991}})),
        ("POST", "/hooks/vm/991/suspend", 200, json!({})),
        ("PUT", "/hooks/vm/991/plan", 200, json!({})),
        ("DELETE", "/hooks/vm/991", 404, json!({})),
        ("POST", "/hooks/vm/500/suspend", 502, json!({"message": "gateway"})),
        ("GET", "/ping", 200, json!({})),
    ])
    .await;
    let p = common::wasm_registry().get("webhook").unwrap();
    let server_actions = json!({
        "test": {"path": "/ping"},
        "create": {"path": "/hooks/create", "body": "{\"email\":\"{{email}}\",\"name\":\"{{last_name}}\",\"host\":\"{{custom.hostname}}\",\"ram\":{{config.ram}}}", "remote_id_pointer": "/data/vm"},
        "suspend": {"path": "/hooks/vm/{{remote_id}}/suspend"},
        "terminate": {"method": "DELETE", "path": "/hooks/vm/{{remote_id}}"}
    });
    let conn = ServerConn { base_url: url, api_key: "whsec".into(), config: json!({"actions": server_actions.to_string()}) };
    p.test_connection(&conn).await.unwrap();
    // Product overrides change_package only.
    let product = json!({"ram": 512, "actions": {"change_package": {"method": "put", "path": "/hooks/vm/{{remote_id}}/plan", "body": "{\"ram\":{{config.ram}}}"}}});
    let c = run(p.as_ref(), "create", &conn, &svc(product.clone(), None)).await.unwrap().unwrap();
    assert_eq!(c.remote_id, "991");
    let s = svc(product.clone(), Some("991"));
    for a in ["suspend", "change_package", "terminate"] {
        run(p.as_ref(), a, &conn, &s).await.unwrap_or_else(|e| panic!("{a}: {e}"));
    }
    // Unconfigured action is a config error; 5xx is retryable.
    assert!(matches!(run(p.as_ref(), "unsuspend", &conn, &s).await.unwrap_err(), ProvisionError::Config(_)));
    assert!(run(p.as_ref(), "suspend", &conn, &svc(product, Some("500"))).await.unwrap_err().retryable());

    let log = log.lock().unwrap();
    let create = log.iter().find(|x| x.path == "/hooks/create").unwrap();
    let body: Value = serde_json::from_str(&create.body).expect("templated body is valid JSON despite quotes");
    assert_eq!(body, json!({"email": "alice@example.com", "name": "Doe \"Q\"", "host": "srv one", "ram": 512}));
    assert_eq!(h(create, "x-pelty-signature"), format!("sha256={}", sign("whsec", create.body.as_bytes())));
    assert_eq!(h(create, "authorization"), "Bearer whsec");
    assert_eq!(h(create, "x-pelty-action"), "create");
    let plan = log.iter().find(|x| x.method == "PUT").unwrap();
    assert_eq!(plan.body, "{\"ram\":512}");
}

#[tokio::test(flavor = "multi_thread")]
async fn https_agent_runs_signed_commands() {
    let (log, url) = start(vec![
        ("GET", "/v1/health", 200, json!({"ok": true})),
        ("POST", "/v1/run", 200, json!({"ok": true, "output": "done", "remote_id": "ct-7"})),
    ])
    .await;
    let p = common::wasm_registry().get("https_agent").unwrap();
    let conn = ServerConn { base_url: url, api_key: "agent-secret".into(), config: json!({}) };
    p.test_connection(&conn).await.unwrap();
    let cfg = json!({"create_command": "create-ct --ram {{config.ram}} --owner {{email}}", "suspend_command": "stop {{remote_id}}", "ram": 1024});
    let c = run(p.as_ref(), "create", &conn, &svc(cfg.clone(), None)).await.unwrap().unwrap();
    assert_eq!(c.remote_id, "ct-7");
    run(p.as_ref(), "suspend", &conn, &svc(cfg.clone(), Some("ct-7"))).await.unwrap();
    assert!(matches!(run(p.as_ref(), "terminate", &conn, &svc(cfg, Some("ct-7"))).await.unwrap_err(), ProvisionError::Config(_)));
    let log = log.lock().unwrap();
    let runs: Vec<_> = log.iter().filter(|x| x.path == "/v1/run").collect();
    assert_eq!(runs.len(), 2);
    for r in &runs {
        let ts = h(r, "x-pelty-timestamp");
        assert_eq!(h(r, "x-pelty-signature"), format!("sha256={}", sign("agent-secret", format!("{ts}.{}", r.body).as_bytes())));
        assert!(h(r, "authorization").is_empty(), "secret is never sent in clear");
    }
    let b0: Value = serde_json::from_str(&runs[0].body).unwrap();
    assert_eq!(b0["command"], "create-ct --ram 1024 --owner alice@example.com");
    assert_eq!(b0["action"], "create");
    let b1: Value = serde_json::from_str(&runs[1].body).unwrap();
    assert_eq!(b1["command"], "stop ct-7");
}

#[tokio::test(flavor = "multi_thread")]
async fn https_agent_failure_is_rejected() {
    let (_log, url) = start(vec![("POST", "/v1/run", 200, json!({"ok": false, "output": "no space"}))]).await;
    let p = common::wasm_registry().get("https_agent").unwrap();
    let conn = ServerConn { base_url: url, api_key: "k".into(), config: json!({}) };
    match run(p.as_ref(), "create", &conn, &svc(json!({"create_command": "x"}), None)).await.unwrap_err() {
        ProvisionError::Rejected { status: 422, message } => assert_eq!(message, "no space"),
        other => panic!("{other}"),
    }
}
