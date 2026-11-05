//! Zip upload + registry (marketplace) install, against a fresh DB and an
//! in-process registry server (no real marketplace is contacted).
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use pelty_extensions::addon_pkg::sha256_hex;
use serde_json::{json, Value};
use std::io::Write;
use tower::ServiceExt;

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, b) in entries {
        w.start_file(*n, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn package(id: &str, version: &str, sql: &str) -> Vec<u8> {
    let m = json!({"id": id, "name": "Uploaded", "version": version, "kind": "provisioning", "engine": "webhook",
        "permissions": ["network.outbound"], "migrations": ["m/0001.sql"], "files": {"m/0001.sql": sha256_hex(sql.as_bytes())}});
    zip_of(&[
        ("addon.json", &serde_json::to_vec(&m).unwrap()),
        ("m/0001.sql", sql.as_bytes()),
    ])
}

async fn upload(app: &axum::Router, s: &Sess, uri: &str, body: Vec<u8>) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/zip");
    for (k, v) in s.h() {
        b = b.header(k, v);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let st = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn zip_upload_and_registry_install() {
    // Shared test binary: other suites read PELTY_ADDONS_DIR concurrently, so
    // use the common addons dir and only touch this test's own addon ids.
    let (app, st) = app().await;
    let dir = pelty_api::addons::addons_dir();
    for id in ["upl-demo", "reg-demo"] {
        let _ = std::fs::remove_dir_all(dir.join(id));
        sqlx::query("DELETE FROM installed_addons WHERE id = $1")
            .bind(id)
            .execute(&st.pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "DROP TABLE IF EXISTS addon_{}_log",
            id.replace('-', "_")
        ))
        .execute(&st.pool)
        .await
        .unwrap();
    }
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let sql = "CREATE TABLE addon_upl_demo_log (id int);";
    let up = "/api/v1/admin/addons/upload";

    assert_eq!(
        upload(&app, &alice, up, package("upl-demo", "1.0.0", sql))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    // Hostile packages.
    let (c, b) = upload(&app, &admin, up, b"garbage".to_vec()).await;
    assert_eq!(
        (c, b["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("error.addon_zip"))
    );
    let m = serde_json::to_vec(&json!({"id": "upl-demo", "name": "x", "version": "1", "kind": "plugin", "engine": "x", "files": {}})).unwrap();
    let (_, b) = upload(
        &app,
        &admin,
        up,
        zip_of(&[("addon.json", &m), ("../../etc/passwd", b"x")]),
    )
    .await;
    assert_eq!(b["error"]["code"], "error.addon_unsafe_path");
    let (_, b) = upload(
        &app,
        &admin,
        up,
        zip_of(&[("addon.json", &m), ("evil.sh", b"x")]),
    )
    .await;
    assert_eq!(b["error"]["code"], "error.addon_integrity");
    let (_, b) = upload(
        &app,
        &admin,
        up,
        package("upl-demo", "1.0.0", "DROP TABLE users;"),
    )
    .await;
    assert_eq!(b["error"]["code"], "error.addon_migration");
    let (_, b) = upload(
        &app,
        &admin,
        up,
        package(
            "provisioning-webhook",
            "1.0.0",
            "CREATE TABLE addon_provisioning_webhook_x (id int);",
        ),
    )
    .await;
    assert_eq!(b["error"]["code"], "error.addon_installed");
    let (c, _) = upload(&app, &admin, up, vec![0u8; 65 * 1024 * 1024]).await;
    assert_eq!(c, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        !dir.join("upl-demo").exists(),
        "nothing extracted from rejected packages"
    );

    // Valid upload → available, then installable (migration runs).
    let (c, b) = upload(&app, &admin, up, package("upl-demo", "1.0.0", sql)).await;
    assert_eq!(c, StatusCode::OK, "{b:?}");
    assert_eq!(b["status"], "available");
    assert!(dir.join("upl-demo/m/0001.sql").exists());
    assert_eq!(
        upload(&app, &admin, up, package("upl-demo", "1.0.0", sql))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/addons/upl-demo/install",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // Upgrade in place (same migrations) keeps it installed and enable re-verifies.
    let (c, b) = upload(
        &app,
        &admin,
        &format!("{up}?replace=true"),
        package("upl-demo", "1.1.0", sql),
    )
    .await;
    assert_eq!(c, StatusCode::OK, "{b:?}");
    assert_eq!(b["installed_version"], "1.1.0");
    assert_eq!(b["integrity_ok"], true);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/upl-demo/enable",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        upload(
            &app,
            &admin,
            &format!("{up}?replace=true"),
            package("upl-demo", "1.2.0", sql)
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "enabled addons must be disabled first"
    );

    // Registry: unset → 409; then an in-process index.
    sqlx::query("DELETE FROM system_settings WHERE key = 'addons.registry'")
        .execute(&st.pool)
        .await
        .ok();
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/addons/registry",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let good = package(
        "reg-demo",
        "2.0.0",
        "CREATE TABLE addon_reg_demo_log (id int);",
    );
    let good_sha = sha256_hex(&good);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let idx = json!({"addons": [
        {"id": "reg-demo", "name": "Reg", "version": "2.0.0", "kind": "provisioning", "download_url": format!("{base}/reg.zip"), "sha256": good_sha},
        {"id": "reg-bad", "name": "Bad", "version": "1", "kind": "provisioning", "download_url": format!("{base}/reg.zip"), "sha256": "0".repeat(64)},
        {"id": "BAD ID", "name": "x", "version": "1", "kind": "plugin", "download_url": "x", "sha256": "y"}
    ]});
    let g2 = good.clone();
    let reg_app = axum::Router::new()
        .route(
            "/index.json",
            axum::routing::get(move || {
                let i = idx.clone();
                async move { axum::Json(i) }
            }),
        )
        .route(
            "/reg.zip",
            axum::routing::get(move || {
                let g = g2.clone();
                async move { g }
            }),
        );
    tokio::spawn(async move { axum::serve(l, reg_app).await.unwrap() });
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/addons/registry/settings",
            Some(json!({"url": "ftp://x"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/addons/registry/settings",
            Some(json!({"url": "http://u:p@x/"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/addons/registry/settings",
        Some(json!({"url": format!("{base}/index.json")})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/addons/registry",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = r.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "invalid entries dropped");
    assert_eq!(items[0]["installed"], false);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/addons/registry/reg-bad/install",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.addon_integrity",
        "sha256 mismatch refused"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/registry/nope/install",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/addons/registry/reg-demo/install",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["version"], "2.0.0");
    assert!(dir.join("reg-demo/addon.json").exists());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action IN ('addon.uploaded','addon.downloaded') AND target IN ('upl-demo','reg-demo')").fetch_one(&st.pool).await.unwrap();
    assert!(n >= 3, "audited: {n}");
    for id in ["upl-demo", "reg-demo"] {
        let _ = std::fs::remove_dir_all(dir.join(id));
    }
}
