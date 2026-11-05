//! WASM port of `pelty-provisioning/tests/mock_vps_panels.rs`: identical behaviour
//! tests, but every module runs as its compiled addon inside the sandbox.
//! Convoy / VirtFusion / Enhance against in-process mock servers (never a
//! live panel). Each test asserts the exact requests sent.
mod common;
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
        .unwrap_or_else(|| panic!("no {method} {prefix}"))
}
#[tokio::test(flavor = "multi_thread")]
async fn convoy_lifecycle() {
    let (m, url) = start(vec![
        ("GET", "/api/application/users", 200, json!({"data": []})),
        (
            "POST",
            "/api/application/users/7/generate-sso-token",
            200,
            json!({"data": {"token": "tok"}}),
        ),
        (
            "POST",
            "/api/application/users",
            201,
            json!({"data": {"id": 7}}),
        ),
        (
            "POST",
            "/api/application/servers/abc-1/settings/suspend",
            204,
            Value::Null,
        ),
        (
            "POST",
            "/api/application/servers",
            201,
            json!({"data": {"uuid": "abc-1"}}),
        ),
        (
            "PATCH",
            "/api/application/servers/abc-1/settings/build",
            200,
            json!({}),
        ),
        (
            "DELETE",
            "/api/application/servers/abc-1",
            404,
            json!({"message": "gone"}),
        ),
        ("PATCH", "/api/client/servers/abc-1/state", 204, Value::Null),
    ])
    .await;
    let reg = common::wasm_registry();
    let mo = reg.get("convoy").unwrap();
    let s = conn(&url, "k", json!({}));
    let cfg =
        json!({"node_id": 1, "template_uuid": "t-1", "cpu": 2, "memory": 2048, "disk": 10240});
    let c = run(
        mo.as_ref(),
        "create",
        &s,
        &svc(cfg.clone(), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(c.remote_id, "abc-1");
    let r = reqs(&m);
    let b: Value =
        serde_json::from_str(&find(&r, "POST", "/api/application/servers").body).unwrap();
    assert_eq!(b["user_id"], 7);
    assert_eq!(b["limits"]["memory"], 2048u64 * 1024 * 1024);
    assert_eq!(
        find(&r, "POST", "/api/application/servers").headers["authorization"],
        "Bearer k"
    );
    let sv = svc(cfg, Some("abc-1"), c.module_data.clone());
    run(mo.as_ref(), "suspend", &s, &sv).await.unwrap();
    run(mo.as_ref(), "change_package", &s, &sv).await.unwrap();
    run(mo.as_ref(), "terminate", &s, &sv).await.unwrap();
    run_custom(mo.as_ref(), "restart", &s, &sv, &json!({}))
        .await
        .unwrap();
    let u = mo.sso_url(&s, &sv, "x").await.unwrap().unwrap();
    assert_eq!(u, format!("{url}/authenticate?token=tok"));
    let bad = svc(json!({}), Some("../x"), json!({}));
    assert!(matches!(
        run(mo.as_ref(), "suspend", &s, &bad).await,
        Err(ProvisionError::Config(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn virtfusion_lifecycle() {
    let (m, url) = start(vec![
        ("GET", "/api/v1/users/", 404, json!({"msg": "not found"})),
        (
            "POST",
            "/api/v1/users/",
            200,
            json!({"data": {"authentication": {"endpoint_complete": "/token/xyz"}}}),
        ),
        ("POST", "/api/v1/users", 201, json!({"data": {"id": 9}})),
        ("POST", "/api/v1/servers/55/build", 200, json!({})),
        ("POST", "/api/v1/servers/55/suspend", 204, Value::Null),
        ("POST", "/api/v1/servers/55/power/restart", 200, json!({})),
        ("POST", "/api/v1/servers", 201, json!({"data": {"id": 55}})),
        ("PUT", "/api/v1/servers/55/package/3", 200, json!({})),
        ("DELETE", "/api/v1/servers/55", 204, Value::Null),
    ])
    .await;
    let reg = common::wasm_registry();
    let mo = reg.get("virtfusion").unwrap();
    let s = conn(&url, "k", json!({}));
    let cfg = json!({"package_id": 3, "hypervisor_group_id": 2, "os_template_id": 11});
    let c = run(
        mo.as_ref(),
        "create",
        &s,
        &svc(cfg.clone(), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(c.remote_id, "55");
    let r = reqs(&m);
    let b: Value = serde_json::from_str(&find(&r, "POST", "/api/v1/servers").body).unwrap();
    assert_eq!(
        b,
        json!({"packageId": 3, "userId": 9, "hypervisorId": 2, "ipv4": 1})
    );
    let bb: Value =
        serde_json::from_str(&find(&r, "POST", "/api/v1/servers/55/build").body).unwrap();
    assert_eq!(bb["operatingSystemId"], 11);
    let sv = svc(cfg, Some("55"), c.module_data.clone());
    run(mo.as_ref(), "suspend", &s, &sv).await.unwrap();
    run(mo.as_ref(), "change_package", &s, &sv).await.unwrap();
    run(mo.as_ref(), "terminate", &s, &sv).await.unwrap();
    assert!(find(&reqs(&m), "DELETE", "/api/v1/servers/55")
        .path
        .ends_with("delay=0"));
    run_custom(mo.as_ref(), "restart", &s, &sv, &json!({}))
        .await
        .unwrap();
    assert_eq!(
        mo.sso_url(&s, &sv, "x").await.unwrap().unwrap(),
        format!("{url}/token/xyz")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn enhance_lifecycle() {
    let (m, url) = start(vec![
        (
            "POST",
            "/api/orgs/master/customers/c1/subscriptions",
            201,
            json!({"id": 44}),
        ),
        (
            "POST",
            "/api/orgs/master/customers",
            201,
            json!({"id": "c1"}),
        ),
        ("POST", "/api/logins", 201, json!({"id": "l1"})),
        ("POST", "/api/orgs/c1/members", 201, json!({"id": "m1"})),
        ("POST", "/api/orgs/c1/websites", 201, json!({"id": "w1"})),
        ("PATCH", "/api/orgs/c1/subscriptions/44", 204, Value::Null),
        (
            "GET",
            "/api/orgs/c1/members/m1/sso",
            200,
            json!("https://panel/sso/abc"),
        ),
        ("DELETE", "/api/orgs/c1", 204, Value::Null),
    ])
    .await;
    let reg = common::wasm_registry();
    let mo = reg.get("enhance").unwrap();
    let s = conn(&url, "k", json!({"org_id": "master"}));
    let cfg = json!({"plan_id": 5});
    let c = run(
        mo.as_ref(),
        "create",
        &s,
        &svc(cfg.clone(), None, json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(c.remote_id, "c1");
    assert_eq!(c.module_data["subscription_id"], "44");
    let r = reqs(&m);
    let w: Value = serde_json::from_str(&find(&r, "POST", "/api/orgs/c1/websites").body).unwrap();
    assert_eq!(w, json!({"domain": "alice.example", "subscriptionId": 44}));
    let sv = svc(cfg, Some("c1"), c.module_data.clone());
    run(mo.as_ref(), "suspend", &s, &sv).await.unwrap();
    let p: Value =
        serde_json::from_str(&find(&reqs(&m), "PATCH", "/api/orgs/c1/subscriptions/44").body)
            .unwrap();
    assert_eq!(p, json!({"isSuspended": true}));
    run(mo.as_ref(), "terminate", &s, &sv).await.unwrap();
    assert_eq!(
        mo.sso_url(&s, &sv, "x").await.unwrap().unwrap(),
        "https://panel/sso/abc"
    );
    let no_org = conn(&url, "k", json!({}));
    assert!(matches!(
        mo.test_connection(&no_org).await,
        Err(ProvisionError::Config(_))
    ));
}
