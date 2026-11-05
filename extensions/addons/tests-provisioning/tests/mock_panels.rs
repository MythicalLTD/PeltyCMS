//! Modules against in-process mock panels (never a real panel).
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Router;
use pelty_provisioning::{run, ProvisionError, ServerConn, ServiceCtx};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<(String, String, Value, String)>>>;
type Routes = Arc<Mutex<Vec<(String, String, u16, Value)>>>;

#[derive(Clone, Default)]
struct Mock {
    log: Log,
    /// (method, path prefix) -> (status, body)
    routes: Routes,
}

async fn handler(
    State(m): State<Mock>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let path = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let b: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    m.log
        .lock()
        .unwrap()
        .push((method.to_string(), path.clone(), b, auth));
    let routes = m.routes.lock().unwrap().clone();
    for (rm, rp, st, rb) in routes {
        if rm == method.as_str() && path.starts_with(&rp) {
            return (StatusCode::from_u16(st).unwrap(), axum::Json(rb)).into_response();
        }
    }
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({"errors": [{"detail": "no route"}]})),
    )
        .into_response()
}

async fn start(routes: Vec<(&str, &str, u16, Value)>) -> (Mock, String) {
    let m = Mock::default();
    *m.routes.lock().unwrap() = routes
        .into_iter()
        .map(|(a, b, c, d)| (a.to_string(), b.to_string(), c, d))
        .collect();
    let app = Router::new().fallback(handler).with_state(m.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (m, format!("http://{addr}"))
}

fn svc(config: Value, remote: Option<&str>) -> ServiceCtx {
    ServiceCtx {
        service_id: "0b7a5f0e-1111-2222-3333-444455556666".into(),
        customer_email: "alice@example.com".into(),
        customer_first_name: "Alice".into(),
        customer_last_name: "Doe".into(),
        product_config: config,
        custom_fields: json!({}),
        remote_id: remote.map(str::to_string),
        module_data: json!({}),
    }
}

#[tokio::test]
async fn pterodactyl_full_lifecycle() {
    let (m, url) = start(vec![
        (
            "GET",
            "/api/application/users?filter",
            200,
            json!({"data": []}),
        ),
        (
            "POST",
            "/api/application/users",
            201,
            json!({"attributes": {"id": 42}}),
        ),
        (
            "POST",
            "/api/application/servers/77/suspend",
            204,
            Value::Null,
        ),
        (
            "POST",
            "/api/application/servers/77/unsuspend",
            204,
            Value::Null,
        ),
        (
            "POST",
            "/api/application/servers",
            201,
            json!({"attributes": {"id": 77, "identifier": "abc123"}}),
        ),
        (
            "GET",
            "/api/application/servers/77",
            200,
            json!({"attributes": {"id": 77, "allocation": 9}}),
        ),
        (
            "PATCH",
            "/api/application/servers/77/build",
            200,
            json!({"attributes": {"id": 77}}),
        ),
        ("DELETE", "/api/application/servers/77", 204, Value::Null),
        (
            "GET",
            "/api/application/locations",
            200,
            json!({"data": []}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("pterodactyl").unwrap();
    let conn = ServerConn {
        base_url: url,
        api_key: "ptla_secret".into(),
        config: json!({}),
    };
    p.test_connection(&conn).await.unwrap();
    let cfg = json!({"egg": 5, "location": 1, "docker_image": "ghcr.io/x:y", "startup": "java -jar s.jar", "environment": {"VERSION": "latest"}, "memory": "2048", "disk": 10240, "cpu": 200, "backups": 2});
    let created = run(p.as_ref(), "create", &conn, &svc(cfg.clone(), None))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created.remote_id, "77");
    assert_eq!(created.module_data["user_id"], 42);
    {
        let log = m.log.lock().unwrap();
        assert!(log.iter().all(|(_, _, _, a)| a == "Bearer ptla_secret"));
        let (_, _, body, _) = log
            .iter()
            .find(|(me, p, _, _)| me == "POST" && p == "/api/application/servers")
            .unwrap();
        assert_eq!(body["user"], 42);
        assert_eq!(body["limits"]["memory"], 2048);
        assert_eq!(body["feature_limits"]["backups"], 2);
        assert_eq!(body["deploy"]["locations"], json!([1]));
        assert_eq!(body["environment"]["VERSION"], "latest");
        let (_, _, ub, _) = log
            .iter()
            .find(|(me, p, _, _)| me == "POST" && p == "/api/application/users")
            .unwrap();
        assert_eq!(ub["email"], "alice@example.com");
    }
    // Retried create with a known remote id makes no new server.
    let n = m.log.lock().unwrap().len();
    run(p.as_ref(), "create", &conn, &svc(cfg.clone(), Some("77")))
        .await
        .unwrap();
    assert_eq!(m.log.lock().unwrap().len(), n);

    let s = svc(json!({"memory": 4096, "disk": 20480}), Some("77"));
    for a in ["suspend", "unsuspend", "change_package", "terminate"] {
        run(p.as_ref(), a, &conn, &s)
            .await
            .unwrap_or_else(|e| panic!("{a}: {e}"));
    }
    let log = m.log.lock().unwrap();
    let (_, _, build, _) = log.iter().find(|(me, _, _, _)| me == "PATCH").unwrap();
    assert_eq!(build["allocation"], 9);
    assert_eq!(build["memory"], 4096);
}

#[tokio::test]
async fn pterodactyl_errors_are_classified() {
    let (_m, url) = start(vec![
        (
            "POST",
            "/api/application/servers/1/suspend",
            503,
            json!({"errors": [{"detail": "down"}]}),
        ),
        (
            "POST",
            "/api/application/servers/2/suspend",
            422,
            json!({"errors": [{"detail": "bad"}]}),
        ),
        ("DELETE", "/api/application/servers/3", 404, json!({})),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("pterodactyl").unwrap();
    let conn = ServerConn {
        base_url: url,
        api_key: "k".into(),
        config: json!({}),
    };
    let e = run(p.as_ref(), "suspend", &conn, &svc(json!({}), Some("1")))
        .await
        .unwrap_err();
    assert!(e.retryable(), "{e}");
    match run(p.as_ref(), "suspend", &conn, &svc(json!({}), Some("2")))
        .await
        .unwrap_err()
    {
        ProvisionError::Rejected {
            status: 422,
            message,
        } => assert_eq!(message, "bad"),
        other => panic!("{other}"),
    }
    // Terminating something already gone is fine.
    run(p.as_ref(), "terminate", &conn, &svc(json!({}), Some("3")))
        .await
        .unwrap();
    // Missing package config is a config error, not a network call.
    let e = run(p.as_ref(), "create", &conn, &svc(json!({"egg": 1}), None))
        .await
        .unwrap_err();
    assert!(
        matches!(e, ProvisionError::Config(_)) || !e.retryable(),
        "{e}"
    );
    let bad = ServerConn {
        base_url: "ftp://x".into(),
        api_key: "k".into(),
        config: json!({}),
    };
    assert!(matches!(
        p.test_connection(&bad).await.unwrap_err(),
        ProvisionError::Config(_)
    ));
}

#[tokio::test]
async fn featherpanel_full_lifecycle() {
    let (m, url) = start(vec![
        (
            "GET",
            "/api/admin/users?search=",
            200,
            json!({"success": true, "data": {"users": [{"id": 5, "email": "ALICE@example.com"}]}}),
        ),
        (
            "PUT",
            "/api/admin/servers",
            201,
            json!({"success": true, "data": {"server_id": 31}}),
        ),
        (
            "POST",
            "/api/admin/servers/31/suspend",
            200,
            json!({"success": true}),
        ),
        (
            "POST",
            "/api/admin/servers/31/unsuspend",
            200,
            json!({"success": true}),
        ),
        (
            "PATCH",
            "/api/admin/servers/31",
            200,
            json!({"success": true}),
        ),
        (
            "DELETE",
            "/api/admin/servers/31",
            200,
            json!({"success": true}),
        ),
        (
            "GET",
            "/api/admin/servers",
            200,
            json!({"success": true, "data": {}}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("featherpanel").unwrap();
    let conn = ServerConn {
        base_url: format!("{url}/"),
        api_key: "fp_key".into(),
        config: json!({}),
    };
    p.test_connection(&conn).await.unwrap();
    let cfg = json!({"node_id": 1, "allocation_id": 3, "realms_id": 2, "spell_id": 7, "image": "img", "startup": "run", "memory": 1024, "disk": 4096, "variables": {"A": "1"}});
    let c = run(p.as_ref(), "create", &conn, &svc(cfg, None))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(c.remote_id, "31");
    {
        let log = m.log.lock().unwrap();
        assert!(
            !log.iter()
                .any(|(me, p, _, _)| me == "PUT" && p == "/api/admin/users"),
            "existing user reused (case-insensitive email)"
        );
        let (_, _, body, auth) = log
            .iter()
            .find(|(me, p, _, _)| me == "PUT" && p == "/api/admin/servers")
            .unwrap();
        assert_eq!(auth, "Bearer fp_key");
        assert_eq!(body["owner_id"], 5);
        assert_eq!(body["spell_id"], 7);
        assert_eq!(body["io"], 500);
        assert_eq!(body["variables"]["A"], "1");
    }
    let s = svc(json!({"memory": 2048, "disk": 8192}), Some("31"));
    for a in ["suspend", "unsuspend", "change_package", "terminate"] {
        run(p.as_ref(), a, &conn, &s)
            .await
            .unwrap_or_else(|e| panic!("{a}: {e}"));
    }
    let log = m.log.lock().unwrap();
    let (_, _, patch, _) = log.iter().find(|(me, _, _, _)| me == "PATCH").unwrap();
    assert_eq!(patch["memory"], 2048);
}

#[tokio::test]
async fn featherpanel_creates_missing_user() {
    let (m, url) = start(vec![
        (
            "GET",
            "/api/admin/users?search=",
            200,
            json!({"success": true, "data": {"users": []}}),
        ),
        (
            "PUT",
            "/api/admin/users",
            201,
            json!({"success": true, "data": {"user_id": 99}}),
        ),
        (
            "PUT",
            "/api/admin/servers",
            201,
            json!({"success": true, "data": {"server_id": 1}}),
        ),
    ])
    .await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("featherpanel").unwrap();
    let conn = ServerConn {
        base_url: url,
        api_key: "k".into(),
        config: json!({}),
    };
    let cfg = json!({"node_id": 1, "allocation_id": 3, "realms_id": 2, "spell_id": 7, "image": "img", "startup": "run", "memory": 1024, "disk": 4096});
    run(p.as_ref(), "create", &conn, &svc(cfg, None))
        .await
        .unwrap();
    let log = m.log.lock().unwrap();
    let (_, _, u, _) = log
        .iter()
        .find(|(me, p, _, _)| me == "PUT" && p == "/api/admin/users")
        .unwrap();
    assert_eq!(u["username"], "alice_0b7a5f");
    assert!(u["password"].as_str().unwrap().len() >= 32);
    let (_, _, s, _) = log
        .iter()
        .find(|(me, p, _, _)| me == "PUT" && p == "/api/admin/servers")
        .unwrap();
    assert_eq!(s["owner_id"], 99);
}

#[tokio::test]
async fn pterodactyl_reinstall_status_and_usage() {
    let (m, url) = start(vec![
        ("POST", "/api/application/servers/77/reinstall", 204, Value::Null),
        ("GET", "/api/application/servers/77", 200, json!({"attributes": {"id": 77, "identifier": "abc", "status": null, "suspended": false, "limits": {"memory": 2048, "disk": 1000, "cpu": 100}, "feature_limits": {"databases": 2, "allocations": 1, "backups": 3}}})),
    ]).await;
    let reg = pelty_addon_tests_provisioning::builtins();
    let p = reg.get("pterodactyl").unwrap();
    let conn = ServerConn {
        base_url: url,
        api_key: "ptla_secret".into(),
        config: json!({}),
    };
    let s = svc(json!({}), Some("77"));
    assert!(reg
        .list()
        .iter()
        .find(|i| i.id == "pterodactyl")
        .unwrap()
        .actions
        .contains(&"reinstall"));
    pelty_provisioning::run_custom(p.as_ref(), "reinstall", &conn, &s, &json!({}))
        .await
        .unwrap();
    let st = pelty_provisioning::run_custom(p.as_ref(), "status", &conn, &s, &json!({}))
        .await
        .unwrap();
    assert_eq!(st["module_data_patch"]["identifier"], "abc");
    assert!(
        pelty_provisioning::run_custom(p.as_ref(), "nuke", &conn, &s, &json!({}))
            .await
            .is_err()
    );
    let u = p.usage(&conn, &s).await.unwrap().unwrap();
    assert_eq!(u["memory_mb"], 2048);
    assert_eq!(u["backups"], 3);
    assert!(m
        .log
        .lock()
        .unwrap()
        .iter()
        .any(|(me, p, _, _)| me == "POST" && p.ends_with("/reinstall")));
}
