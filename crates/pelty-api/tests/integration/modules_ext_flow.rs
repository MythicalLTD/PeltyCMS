//! Server health, module log, custom actions, client module panel, failed
//! queue and the public licensing verify endpoint, all in-process, no panel.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const SECRET: &str = "lic-signing-secret-0123456789";

async fn drain(st: &pelty_api::AppState, sid: uuid::Uuid) -> String {
    for _ in 0..50 {
        let _ = pelty_api::worker::run_once(st, "mx-test", 10)
            .await
            .unwrap();
        let s: String = sqlx::query_scalar("SELECT provision_status FROM services WHERE id = $1")
            .bind(sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
        if s != "pending" {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    "pending".into()
}

#[tokio::test]
async fn groups_licensing_custom_actions_and_log() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let other = sess(&app, &st, "customer").await;
    call(&app, "GET", "/api/v1/client/services", None, &cust.h()).await;
    call(&app, "GET", "/api/v1/client/services", None, &other.h()).await;

    // Two licensing "servers" (signing secrets), one too short to pass the test.
    let mk = |name: &str, key: &str| json!({"name": name, "module": "licensing", "base_url": "https://lic.example.com", "api_key": key, "max_services": 1});
    let a = call(
        &app,
        "POST",
        "/api/v1/admin/servers",
        Some(mk("A", SECRET)),
        &admin.h(),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let a = a.body["id"].as_str().unwrap().to_string();
    let b = call(
        &app,
        "POST",
        "/api/v1/admin/servers",
        Some(mk("B", "short")),
        &admin.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Health: connection test persisted.
    let chk = call(
        &app,
        "POST",
        "/api/v1/admin/servers/health/check",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(chk.status, StatusCode::OK, "{:?}", chk.body);
    let h = call(
        &app,
        "GET",
        "/api/v1/admin/servers/health",
        None,
        &admin.h(),
    )
    .await;
    let rows = h.body["data"].as_array().unwrap();
    let ok_of = |id: &str| {
        rows.iter().find(|r| r["server"]["id"] == id).unwrap()["server"]["last_check_ok"].clone()
    };
    assert_eq!(ok_of(&a), json!(true));
    assert_eq!(ok_of(&b), json!(false));
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/servers/health", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let (pid, _, _) = seed_product(&app, &admin).await;
    let pm = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/products/{pid}/module"),
        Some(json!({"module": "licensing", "server_id": a, "config": {"key_prefix": "PC"}})),
        &admin.h(),
    )
    .await;
    assert_eq!(pm.status, StatusCode::OK, "{:?}", pm.body);

    // Service provisioned through the queue onto A.
    let sid: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date)
         SELECT gen_random_uuid(), c.id, $2::uuid, 'active', 'monthly', c.currency, 0, CURRENT_DATE + 30 FROM customers c JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1 RETURNING id",
    ).bind(&cust.email).bind(uuid::Uuid::parse_str(&pid).unwrap()).fetch_one(&st.pool).await.unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/module"),
        Some(json!({"action": "create"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(drain(&st, sid).await, "ok");
    let (srv, key): (Option<uuid::Uuid>, String) =
        sqlx::query_as("SELECT server_id, remote_id FROM services WHERE id = $1")
            .bind(sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(srv.map(|x| x.to_string()), Some(a.clone()));
    assert!(key.starts_with("PC-"), "{key}");

    // Module log has the create.
    let log = call(
        &app,
        "GET",
        &format!("/api/v1/admin/module-log?service_id={sid}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(log.status, StatusCode::OK, "{:?}", log.body);
    assert_eq!(log.body["data"][0]["action"], "create");
    assert_eq!(log.body["data"][0]["ok"], true);

    // A broken service (server of another module) dies at once.
    let c = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": "C", "module": "webhook", "base_url": "https://hook.example.com", "api_key": "k"})), &admin.h()).await.body["id"].as_str().unwrap().to_string();
    let sid2: uuid::Uuid = sqlx::query_scalar("INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, server_id) SELECT gen_random_uuid(), customer_id, product_id, 'active', cycle, currency, 0, $2::uuid FROM services WHERE id = $1 RETURNING id").bind(sid).bind(&c).fetch_one(&st.pool).await.unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid2}/module"),
        Some(json!({"action": "create"})),
        &admin.h(),
    )
    .await;
    assert_eq!(drain(&st, sid2).await, "failed");
    let q = call(&app, "GET", "/api/v1/admin/module-queue", None, &admin.h()).await;
    assert!(
        q.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["service_id"] == sid2.to_string()),
        "{:?}",
        q.body
    );
    let staff_mail = mails_to(&st, &admin.email).await;
    assert!(
        staff_mail.iter().any(|m| m.subject.contains("create")),
        "staff notified"
    );
    let rt = call(
        &app,
        "POST",
        &format!("/api/v1/admin/module-queue/{sid2}/retry"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(rt.status, StatusCode::OK, "{:?}", rt.body);

    // Public verify: binds on first use, signed, then rejects another domain.
    let none: [(&str, &str); 0] = [];
    let v = call(
        &app,
        "POST",
        "/api/v1/licensing/verify",
        Some(json!({"key": key, "domain": "www.Shop.example", "ip": "203.0.113.5"})),
        &none,
    )
    .await;
    assert_eq!(v.status, StatusCode::OK, "{:?}", v.body);
    assert_eq!(v.body["status"], "active", "{:?}", v.body);
    let mut payload = v.body.clone();
    let sig = payload
        .as_object_mut()
        .unwrap()
        .remove("signature")
        .unwrap();
    assert_eq!(
        sig,
        json!(pelty_provisioning::licensing::sign(SECRET, &payload))
    );
    let v2 = call(
        &app,
        "POST",
        "/api/v1/licensing/verify",
        Some(json!({"key": key, "domain": "other.example", "ip": "203.0.113.5"})),
        &none,
    )
    .await;
    assert_eq!(v2.body["status"], "invalid");
    let nf = call(
        &app,
        "POST",
        "/api/v1/licensing/verify",
        Some(json!({"key": "nope"})),
        &none,
    )
    .await;
    assert_eq!(nf.body["status"], "invalid");

    // Client panel: owner sees key + reissue; others 404.
    let p = call(
        &app,
        "GET",
        &format!("/api/v1/client/services/{sid}/module"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert_eq!(p.body["data"]["license_key"], key.as_str());
    assert_eq!(p.body["data"]["bound_domain"], "shop.example");
    assert_eq!(p.body["actions"], json!(["reissue"]));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/services/{sid}/module"),
            None,
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/services/{sid}/module/terminate"),
            Some(json!({})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let ri = call(
        &app,
        "POST",
        &format!("/api/v1/client/services/{sid}/module/reissue"),
        Some(json!({})),
        &cust.h(),
    )
    .await;
    assert_eq!(ri.status, StatusCode::OK, "{:?}", ri.body);
    // Binding cleared → the new domain is accepted now.
    let v3 = call(
        &app,
        "POST",
        "/api/v1/licensing/verify",
        Some(json!({"key": key, "domain": "other.example", "ip": "198.51.100.1"})),
        &none,
    )
    .await;
    assert_eq!(v3.body["status"], "active", "{:?}", v3.body);

    // Admin custom action: undeclared action rejected; declared one logged.
    let bad = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/module/custom"),
        Some(json!({"action": "rm_rf"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = call(
        &app,
        "POST",
        &format!("/api/v1/admin/services/{sid}/module/custom"),
        Some(json!({"action": "reissue"})),
        &admin.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    let us = call(
        &app,
        "GET",
        &format!("/api/v1/admin/services/{sid}/module/usage"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(us.body["actions"], json!(["reissue"]));
    let log = call(
        &app,
        "GET",
        &format!("/api/v1/admin/module-log?service_id={sid}"),
        None,
        &admin.h(),
    )
    .await;
    let srcs: Vec<&str> = log.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["source"].as_str())
        .collect();
    assert!(
        srcs.contains(&"admin") && srcs.contains(&"client") && srcs.contains(&"queue"),
        "{srcs:?}"
    );

    // Suspended: client panel shows no actions, verify reports suspended.
    sqlx::query("UPDATE services SET status = 'suspended' WHERE id = $1")
        .bind(sid)
        .execute(&st.pool)
        .await
        .unwrap();
    let v4 = call(
        &app,
        "POST",
        "/api/v1/licensing/verify",
        Some(json!({"key": key, "domain": "other.example", "ip": "198.51.100.1"})),
        &none,
    )
    .await;
    assert_eq!(v4.body["status"], "suspended");
}
