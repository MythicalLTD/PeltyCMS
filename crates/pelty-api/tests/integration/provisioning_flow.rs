//! Provisioning end to end against an in-process mock Pterodactyl panel
//! (never a real panel): server CRUD, key secrecy, approve → create job,
//! suspend → job, permanent failure → dead + failed status.
use crate::common;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use common::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<(String, String, String)>>>;
#[derive(Clone, Default)]
struct Mock {
    log: Log,
    fail_suspend: Arc<Mutex<bool>>,
}

async fn panel(
    State(m): State<Mock>,
    method: Method,
    uri: Uri,
    h: HeaderMap,
    _b: Bytes,
) -> axum::response::Response {
    let path = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let auth = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    m.log
        .lock()
        .unwrap()
        .push((method.to_string(), path.clone(), auth.clone()));
    if auth != "Bearer ptla_test_key" {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"errors": [{"detail": "bad key"}]})),
        )
            .into_response();
    }
    let ok = |st: u16, v: Value| (StatusCode::from_u16(st).unwrap(), axum::Json(v)).into_response();
    match (method.as_str(), path.as_str()) {
        ("GET", p) if p.starts_with("/api/application/users?") => ok(200, json!({"data": []})),
        ("POST", "/api/application/users") => ok(201, json!({"attributes": {"id": 42}})),
        ("POST", "/api/application/servers") => {
            ok(201, json!({"attributes": {"id": 77, "identifier": "abc"}}))
        }
        ("POST", "/api/application/servers/77/suspend") => {
            if *m.fail_suspend.lock().unwrap() {
                ok(422, json!({"errors": [{"detail": "nope"}]}))
            } else {
                StatusCode::NO_CONTENT.into_response()
            }
        }
        ("GET", p) if p.starts_with("/api/application/locations") => ok(200, json!({"data": []})),
        _ => ok(404, json!({"errors": [{"detail": "no route"}]})),
    }
}

/// Runs this service's provisioning jobs only (never other suites' leftovers).
async fn drain(st: &pelty_api::AppState, sid: &str) -> String {
    drain_jobs(st, pelty_database::provisioning::JOB_KIND, sid).await;
    sqlx::query_scalar("SELECT provision_status FROM services WHERE id = $1::uuid")
        .bind(sid)
        .fetch_one(&st.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn provisioning_servers_jobs_and_failures() {
    let m = Mock::default();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let r = axum::Router::new().fallback(panel).with_state(m.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });

    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;

    let mods = call(
        &app,
        "GET",
        "/api/v1/admin/provisioning/modules",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(mods.status, StatusCode::OK);
    let ids: Vec<&str> = mods.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"pterodactyl") && ids.contains(&"featherpanel"),
        "{ids:?}"
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/servers", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let body = json!({"name": "Node A", "module": "pterodactyl", "base_url": format!("{url}/"), "api_key": "ptla_wrong"});
    let bad = call(
        &app,
        "POST",
        "/api/v1/admin/servers",
        Some(json!({"name": "x", "module": "nope", "base_url": url, "api_key": "k"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let c = call(
        &app,
        "POST",
        "/api/v1/admin/servers",
        Some(body),
        &admin.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    let srv = c.body["id"].as_str().unwrap().to_string();
    assert!(!c.body.to_string().contains("ptla_"), "key never returned");
    assert_eq!(c.body["base_url"], url.as_str(), "trailing slash trimmed");
    let sealed: Vec<u8> =
        sqlx::query_scalar("SELECT api_key_sealed FROM servers WHERE id = $1::uuid")
            .bind(&srv)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert!(
        !String::from_utf8_lossy(&sealed).contains("ptla_wrong"),
        "sealed at rest"
    );

    let t = call(
        &app,
        "POST",
        &format!("/api/v1/admin/servers/{srv}/test"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(t.body, json!({"ok": false, "error": "error.server_auth"}));
    // Update without api_key keeps it; with one rotates it.
    let u = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/servers/{srv}"),
        Some(json!({"name": "Node A", "module": "pterodactyl", "base_url": url})),
        &admin.h(),
    )
    .await;
    assert_eq!(u.status, StatusCode::OK, "{:?}", u.body);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/servers/{srv}/test"),
            None,
            &admin.h()
        )
        .await
        .body["ok"],
        false
    );
    call(&app, "PUT", &format!("/api/v1/admin/servers/{srv}"), Some(json!({"name": "Node A", "module": "pterodactyl", "base_url": url, "api_key": "ptla_test_key"})), &admin.h()).await;
    let t = call(
        &app,
        "POST",
        &format!("/api/v1/admin/servers/{srv}/test"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(t.body, json!({"ok": true}));

    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let mm = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/products/{pid}/module"),
        Some(json!({"module": "featherpanel", "server_id": srv})),
        &admin.h(),
    )
    .await;
    assert_eq!(mm.body["error"]["code"], "error.server_module_mismatch");
    let cfg = json!({"egg": 5, "location": 1, "docker_image": "img", "startup": "run", "memory": 1024, "disk": 2048, "cpu": 100});
    let pm = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/products/{pid}/module"),
        Some(json!({"module": "pterodactyl", "server_id": srv, "config": cfg})),
        &admin.h(),
    )
    .await;
    assert_eq!(pm.status, StatusCode::OK, "{:?}", pm.body);

    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["id"]
        .as_str()
        .or(o.body["order"]["id"].as_str())
        .unwrap()
        .to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    let sid = ap.body["services"][0].as_str().unwrap().to_string();
    assert_eq!(drain(&st, &sid).await, "ok");
    let state = call(
        &app,
        "GET",
        &format!("/api/v1/admin/services/{sid}/provisioning"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(state.body["state"]["remote_id"], "77", "{:?}", state.body);
    assert_eq!(state.body["state"]["server_id"], srv.as_str());
    assert_eq!(state.body["jobs"][0]["action"], "create");
    assert!(m
        .log
        .lock()
        .unwrap()
        .iter()
        .any(|(me, p, _)| me == "POST" && p == "/api/application/servers"));
    // In use → cannot delete the server.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/servers/{srv}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Billing suspend → suspend job on the panel.
    let s = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/action"),
        Some(json!({"action": "suspend", "reason": "test"})),
        &admin.h(),
    )
    .await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    assert_eq!(drain(&st, &sid).await, "ok");
    assert!(m
        .log
        .lock()
        .unwrap()
        .iter()
        .any(|(_, p, _)| p == "/api/application/servers/77/suspend"));

    // A 4xx rejection is permanent: job dead after one attempt, service failed.
    *m.fail_suspend.lock().unwrap() = true;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/module"),
        Some(json!({"action": "suspend"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let job = r.body["job_id"].as_i64().unwrap();
    assert_eq!(drain(&st, &sid).await, "failed");
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM jobs WHERE id = $1")
            .bind(job)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempts), ("dead", 1));
    let err: String =
        sqlx::query_scalar("SELECT provision_error FROM services WHERE id = $1::uuid")
            .bind(&sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert!(err.contains("422"), "{err}");
    let bad = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/module"),
        Some(json!({"action": "explode"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}
