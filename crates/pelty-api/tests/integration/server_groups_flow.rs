//! Server groups: automatic server selection (least_services) against an
//! in-process mock Pterodactyl panel; capacity limits; group CRUD.
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

async fn panel(State(m): State<Mock>, method: Method, uri: Uri, h: HeaderMap, _b: Bytes) -> axum::response::Response {
    let path = uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let auth = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    m.log.lock().unwrap().push((method.to_string(), path.clone(), auth.clone()));
    if auth != "Bearer ptla_test_key" {
        return (StatusCode::UNAUTHORIZED, axum::Json(json!({"errors": [{"detail": "bad key"}]}))).into_response();
    }
    let ok = |st: u16, v: Value| (StatusCode::from_u16(st).unwrap(), axum::Json(v)).into_response();
    match (method.as_str(), path.as_str()) {
        ("GET", p) if p.starts_with("/api/application/users?") => ok(200, json!({"data": []})),
        ("POST", "/api/application/users") => ok(201, json!({"attributes": {"id": 42}})),
        ("POST", "/api/application/servers") => ok(201, json!({"attributes": {"id": 77, "identifier": "abc"}})),
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
    sqlx::query_scalar("SELECT provision_status FROM services WHERE id = $1::uuid").bind(sid).fetch_one(&st.pool).await.unwrap()
}


#[tokio::test]
async fn server_groups_assign_and_balance() {
    let m = Mock::default();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let r = axum::Router::new().fallback(panel).with_state(m.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;

    assert_eq!(call(&app, "GET", "/api/v1/admin/server-groups", None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", "/api/v1/admin/server-groups", Some(json!({"name": "x", "strategy": "random"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let g = call(&app, "POST", "/api/v1/admin/server-groups", Some(json!({"name": "EU nodes", "strategy": "least_services"})), &admin.h()).await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let gid = g.body["id"].as_str().unwrap().to_string();
    let mut srvs = vec![];
    for (n, max) in [("G1", 1), ("G2", 5)] {
        let c = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": n, "module": "pterodactyl", "base_url": url, "api_key": "ptla_test_key", "max_services": max})), &admin.h()).await;
        assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
        srvs.push(c.body["id"].as_str().unwrap().to_string());
    }
    let mem = call(&app, "PUT", &format!("/api/v1/admin/server-groups/{gid}/servers"), Some(json!({"servers": srvs})), &admin.h()).await;
    assert_eq!(mem.body["servers"].as_array().unwrap().len(), 2, "{:?}", mem.body);

    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let cfg = json!({"egg": 5, "location": 1, "docker_image": "img", "startup": "run", "memory": 1024, "disk": 2048, "cpu": 100});
    let pm = call(&app, "PUT", &format!("/api/v1/admin/products/{pid}/module"), Some(json!({"module": "pterodactyl", "config": cfg})), &admin.h()).await;
    assert_eq!(pm.status, StatusCode::OK, "{:?}", pm.body);
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/products/{pid}/server-group"), Some(json!({"group_id": uuid::Uuid::new_v4()})), &admin.h()).await.status, StatusCode::NOT_FOUND);
    let pg = call(&app, "PUT", &format!("/api/v1/admin/products/{pid}/server-group"), Some(json!({"group_id": gid})), &admin.h()).await;
    assert_eq!(pg.status, StatusCode::OK, "{:?}", pg.body);

    let mut placed = vec![];
    for _ in 0..3 {
        let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]})), &alice.h()).await;
        assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
        let order_id = o.body["id"].as_str().or(o.body["order"]["id"].as_str()).unwrap().to_string();
        let ap = call(&app, "POST", &format!("/api/v1/admin/orders/{order_id}/approve"), None, &admin.h()).await;
        let sid = ap.body["services"][0].as_str().unwrap().to_string();
        assert_eq!(drain(&st, &sid).await, "ok");
        let srv: uuid::Uuid = sqlx::query_scalar("SELECT server_id FROM services WHERE id = $1::uuid").bind(&sid).fetch_one(&st.pool).await.unwrap();
        placed.push(srv.to_string());
    }
    // least_services: G1 (0) first, then G2, then G1 is full (max 1) → G2.
    assert_eq!(placed, vec![srvs[0].clone(), srvs[1].clone(), srvs[1].clone()]);
    let list = call(&app, "GET", "/api/v1/admin/server-groups", None, &admin.h()).await;
    let row = list.body["items"].as_array().unwrap().iter().find(|g| g["id"] == gid.as_str()).cloned().unwrap();
    assert_eq!((row["servers"].as_i64(), row["products"].as_i64()), (Some(2), Some(1)));
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/server-groups/{gid}"), Some(json!({"name": "EU", "strategy": "fill"})), &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/server-groups/{gid}"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let pg: Option<uuid::Uuid> = sqlx::query_scalar("SELECT server_group_id FROM products WHERE id = $1::uuid").bind(&pid).fetch_one(&st.pool).await.unwrap();
    assert!(pg.is_none(), "group delete unbinds products");
}
