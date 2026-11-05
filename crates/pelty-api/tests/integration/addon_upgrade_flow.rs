//! Generated packages exercise the real upload path without compiling WASM.
use crate::common::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use pelty_api::{
    addons::{self, Manifest},
    AppState,
};
use pelty_database::addon_registry as reg;
use pelty_extensions::addon_pkg::sha256_hex;
use serde_json::{json, Value};
use std::io::Write;
use tower::ServiceExt;

/// Exercise the helper itself; no sleep-based scheduling assertions.
#[tokio::test]
async fn lifecycle_locks_leave_single_connection_pool_usable() {
    let (_, s) = app().await;
    // Lifecycle locking must leave even the smallest supported pool usable.
    let holder_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(s.pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let id = "upgrade-lock-primary";
    let other_id = "upgrade-lock-independent";
    let held = pelty_api::addon_upgrade::lock(&holder_pool, id)
        .await
        .unwrap();
    let row = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reg::get(&holder_pool, "upgrade-lock-no-installed-row"),
    )
    .await
    .expect("lifecycle guard consumed the only application pool slot")
    .unwrap();
    assert!(row.is_none());
    // This timeout only catches a regression to a blocking lock. Contention
    // correctness is established by the explicit conflict, not elapsed time.
    let busy = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        pelty_api::addon_upgrade::lock(&s.pool, id),
    )
    .await
    .expect("contended lifecycle lock waited instead of failing fast");
    assert!(matches!(
        busy,
        Err(pelty_core::AppError::Conflict("error.addon_busy"))
    ));

    let independent = pelty_api::addon_upgrade::lock(&s.pool, other_id)
        .await
        .unwrap();
    let other_busy = pelty_api::addon_upgrade::lock(&s.pool, other_id).await;
    assert!(matches!(
        other_busy,
        Err(pelty_core::AppError::Conflict("error.addon_busy"))
    ));

    // Await the explicit rollback response instead of racing the server's
    // eventual handling of a dropped control connection.
    held.release().await.unwrap();
    let reacquired = pelty_api::addon_upgrade::lock(&s.pool, id).await.unwrap();
    reacquired.release().await.unwrap();
    independent.release().await.unwrap();
    let other_reacquired = pelty_api::addon_upgrade::lock(&s.pool, other_id)
        .await
        .unwrap();
    other_reacquired.release().await.unwrap();
    holder_pool.close().await;
}

const ID: &str = "upgrade-policy-demo";
const SQL: &str = "CREATE TABLE addon_upgrade_policy_demo_log (id int);";

fn manifest(version: &str) -> Value {
    json!({"id": ID, "name": "Upgrade policy", "version": version,
        "kind": "plugin", "engine": "upgrade-policy", "permissions": [],
        "settings": [{"key": "token", "kind": "secret"}],
        "migrations": ["m/0001.sql"], "files": {}})
}

fn package(mut m: Value, sql: &str, payload: &[u8]) -> Vec<u8> {
    let mut files = serde_json::Map::new();
    files.insert("payload.txt".into(), json!(sha256_hex(payload)));
    for name in m["migrations"].as_array().unwrap() {
        files.insert(
            name.as_str().unwrap().into(),
            json!(sha256_hex(sql.as_bytes())),
        );
    }
    m["files"] = Value::Object(files);
    let raw = serde_json::to_vec(&m).unwrap();
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in [("addon.json", raw.as_slice()), ("payload.txt", payload)] {
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(bytes).unwrap();
    }
    for name in m["migrations"].as_array().unwrap() {
        w.start_file(
            name.as_str().unwrap(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w.write_all(sql.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

async fn upload(app: &Router, admin: &Sess, bytes: Vec<u8>, replace: bool) -> (StatusCode, Value) {
    let uri = if replace {
        "/api/v1/admin/addons/upload?replace=true"
    } else {
        "/api/v1/admin/addons/upload"
    };
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/zip");
    for (key, value) in admin.h() {
        req = req.header(key, value);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn snapshot(s: &AppState) -> Value {
    let dir = addons::addons_dir().join(ID);
    json!({
        "row": reg::get(&s.pool, ID).await.unwrap(),
        "migrations": reg::applied_migration_checksums(&s.pool, ID).await.unwrap(),
        "manifest": std::fs::read(dir.join("addon.json")).ok(),
        "payload": std::fs::read(dir.join("payload.txt")).ok(),
        "sql": std::fs::read(dir.join("m/0001.sql")).ok(),
        "entries": disk_files(&dir),
    })
}

fn disk_files(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

async fn rejected(app: &Router, s: &AppState, admin: &Sess, bytes: Vec<u8>, key: &str) {
    let before = snapshot(s).await;
    let (status, body) = upload(app, admin, bytes, true).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body:?}");
    assert_eq!(body["error"]["code"], key);
    assert_eq!(
        snapshot(s).await,
        before,
        "rejection must not touch disk or registry"
    );
}

#[tokio::test]
async fn installed_upgrade_policy_is_atomic() {
    let (app, s) = app().await;
    let dir = addons::addons_dir().join(ID);
    let _ = std::fs::remove_dir_all(&dir);
    reg::remove(&s.pool, ID).await.unwrap();
    sqlx::query("DROP TABLE IF EXISTS addon_upgrade_policy_demo_log")
        .execute(&s.pool)
        .await
        .unwrap();
    let admin = sess(&app, &s, "admin").await;
    let original = package(manifest("1.0.0"), SQL, b"original");
    assert_eq!(
        upload(&app, &admin, original.clone(), false).await.0,
        StatusCode::OK
    );
    let installed = call(
        &app,
        "POST",
        &format!("/api/v1/admin/addons/{ID}/install"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(installed.status, StatusCode::OK, "{:?}", installed.body);
    reg::set_settings(&s.pool, ID, &json!({"token": "configured"}))
        .await
        .unwrap();

    rejected(
        &app,
        &s,
        &admin,
        package(manifest("0.9.0"), SQL, b"old"),
        "error.addon_upgrade_version",
    )
    .await;
    rejected(
        &app,
        &s,
        &admin,
        package(manifest("1.0.0"), SQL, b"changed"),
        "error.addon_upgrade_version",
    )
    .await;
    rejected(
        &app,
        &s,
        &admin,
        package(manifest("1.0.0+build2"), SQL, b"changed"),
        "error.addon_upgrade_version",
    )
    .await;
    rejected(
        &app,
        &s,
        &admin,
        package(manifest("not-semver"), SQL, b"changed"),
        "error.addon_upgrade_version",
    )
    .await;
    rejected(
        &app,
        &s,
        &admin,
        package(
            manifest("1.1.0"),
            "CREATE TABLE addon_upgrade_policy_demo_changed (id int);",
            b"new",
        ),
        "error.addon_upgrade_migrations",
    )
    .await;
    for migrations in [json!([]), json!(["m/0001.sql", "m/0002.sql"])] {
        let mut m = manifest("1.1.0");
        m["migrations"] = migrations;
        rejected(
            &app,
            &s,
            &admin,
            package(m, SQL, b"new"),
            "error.addon_upgrade_migrations",
        )
        .await;
    }
    for (field, value, key) in [
        ("kind", json!("theme"), "error.addon_upgrade_identity"),
        (
            "engine",
            json!("other-engine"),
            "error.addon_upgrade_identity",
        ),
        (
            "permissions",
            json!(["network.outbound"]),
            "error.addon_upgrade_contract",
        ),
        (
            "capabilities",
            json!({"secrets": ["token"]}),
            "error.addon_upgrade_contract",
        ),
        (
            "ui",
            json!({"pages": [{"iframe": "https://unapproved.example"}]}),
            "error.addon_upgrade_contract",
        ),
        (
            "limits",
            json!({"memory_mib": 128}),
            "error.addon_upgrade_contract",
        ),
        ("settings", json!([]), "error.addon_upgrade_contract"),
        (
            "settings",
            json!([{"key": "token", "kind": "text"}]),
            "error.addon_upgrade_contract",
        ),
        (
            "settings",
            json!([{"key": "token", "kind": "secret"}, {"key": "extra", "kind": "secret"}]),
            "error.addon_upgrade_contract",
        ),
    ] {
        let mut m = manifest("1.1.0");
        m[field] = value;
        rejected(&app, &s, &admin, package(m, SQL, b"new"), key).await;
    }
    let raw = std::fs::read(dir.join("addon.json")).unwrap();
    for broken in [
        None,
        Some(b"invalid json".to_vec()),
        Some(serde_json::to_vec(&manifest("1.0.0")).unwrap()),
    ] {
        match broken {
            Some(bytes) => std::fs::write(dir.join("addon.json"), bytes).unwrap(),
            None => std::fs::remove_file(dir.join("addon.json")).unwrap(),
        }
        rejected(
            &app,
            &s,
            &admin,
            package(manifest("1.1.0"), SQL, b"new"),
            "error.addon_upgrade_manifest",
        )
        .await;
        std::fs::write(dir.join("addon.json"), &raw).unwrap();
    }
    // Same-version identical replay supports retries and bundled integration seeds.
    assert_eq!(upload(&app, &admin, original, true).await.0, StatusCode::OK);
    reg::set_enabled(&s.pool, ID, true).await.unwrap();
    rejected(
        &app,
        &s,
        &admin,
        package(manifest("1.1.0"), SQL, b"new"),
        "error.addon_disable_first",
    )
    .await;
    let incoming: Manifest = serde_json::from_slice(&raw).unwrap();
    pelty_api::addon_upgrade::check(&s, &incoming, true)
        .await
        .unwrap();
    let mut widened = incoming.clone();
    widened.permissions.push("network.outbound".into());
    assert!(
        pelty_api::addon_upgrade::check(&s, &widened, true)
            .await
            .is_err(),
        "restore must not bypass approvals"
    );
    reg::set_enabled(&s.pool, ID, false).await.unwrap();
    let before = reg::get(&s.pool, ID).await.unwrap().unwrap();
    let (status, body) = upload(&app, &admin, package(manifest("1.1.0"), SQL, b"new"), true).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let after = reg::get(&s.pool, ID).await.unwrap().unwrap();
    assert_eq!(after.version, "1.1.0");
    assert_eq!(after.settings, before.settings);
    assert_eq!(after.permissions, before.permissions);
    assert_eq!(after.enabled, before.enabled);
    assert_ne!(after.manifest_hash, before.manifest_hash);
    assert_eq!(std::fs::read(dir.join("payload.txt")).unwrap(), b"new");
    assert_eq!(
        reg::applied_migration_checksums(&s.pool, ID).await.unwrap()["m/0001.sql"],
        sha256_hex(SQL.as_bytes())
    );
    // Trusted restore must still reject a rollback, even for enabled packages.
    let mut rollback = incoming.clone();
    rollback.version = "0.9.0".into();
    assert!(matches!(
        pelty_api::addon_upgrade::check(&s, &rollback, true).await,
        Err(pelty_core::AppError::Conflict(
            "error.addon_upgrade_version"
        ))
    ));
    reg::remove(&s.pool, ID).await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    sqlx::query("DROP TABLE addon_upgrade_policy_demo_log")
        .execute(&s.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn legacy_adoption_is_restore_only() {
    let (_, s) = app().await;
    let id = "provisioning-upgrade-legacy";
    reg::remove(&s.pool, id).await.unwrap();
    reg::install(
        &s.pool,
        &reg::NewAddon {
            id,
            kind: "provisioning",
            engine: "upgrade-legacy",
            version: "legacy",
            source: "builtin",
            settings: &json!({}),
            permissions: &[],
            manifest_hash: "builtin",
        },
        &[],
    )
    .await
    .unwrap();
    let mut m: Manifest = serde_json::from_value(json!({
        "id": id, "name": "Legacy", "version": "1.0.0", "kind": "provisioning",
        "engine": "wasm", "provides": "upgrade-legacy", "abi": "0.2.0",
        "wasm": "addon.wasm", "files": {"addon.wasm": "0".repeat(64)}
    }))
    .unwrap();
    assert!(matches!(
        pelty_api::addon_upgrade::check(&s, &m, false).await,
        Err(pelty_core::AppError::Conflict(
            "error.addon_upgrade_manifest"
        ))
    ));
    reg::set_enabled(&s.pool, id, true).await.unwrap();
    pelty_api::addon_upgrade::check(&s, &m, true).await.unwrap();
    m.provides = Some("other-engine".into());
    assert!(matches!(
        pelty_api::addon_upgrade::check(&s, &m, true).await,
        Err(pelty_core::AppError::Conflict(
            "error.addon_upgrade_identity"
        ))
    ));
    m.provides = Some("upgrade-legacy".into());
    m.migrations.push("m/new.sql".into());
    assert!(matches!(
        pelty_api::addon_upgrade::check(&s, &m, true).await,
        Err(pelty_core::AppError::Conflict(
            "error.addon_upgrade_migrations"
        ))
    ));
    reg::remove(&s.pool, id).await.unwrap();
}
