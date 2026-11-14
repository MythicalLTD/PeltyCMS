//! #418: built-in placeholder rows (`source = 'builtin'`, `manifest_hash =
//! 'builtin'`) left by migration 0220 must not block the packaged addons.
use crate::common::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use pelty_database::addon_registry as reg;
use serde_json::{json, Value};
use tower::ServiceExt;

const MIGRATION: &str =
    include_str!("../../../../migrations/0371_drop_fresh_builtin_placeholders.sql");

async fn upload(app: &Router, admin: &Sess, bytes: Vec<u8>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/addons/upload")
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A fresh database (no products, no invoices) ends up with no built-in
/// placeholder rows, so nothing blocks installing the packages.
#[tokio::test]
async fn fresh_database_has_no_builtin_placeholders() {
    let (_, s) = app().await;
    let db = "peltycms_fresh_418";
    let drop = format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)");
    sqlx::query(sqlx::AssertSqlSafe(drop.clone()))
        .execute(&s.pool)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db}")))
        .execute(&s.pool)
        .await
        .unwrap();
    let opts = s.pool.connect_options().as_ref().clone().database(db);
    let fresh = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await
        .unwrap();
    pelty_database::migrate(&fresh)
        .await
        .expect("migrate fresh");
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM installed_addons WHERE kind IN ('provisioning','gateway') ORDER BY id",
    )
    .fetch_all(&fresh)
    .await
    .unwrap();
    assert!(ids.is_empty(), "fresh install pre-registered {ids:?}");
    // Re-running the cleanup is harmless and only touches placeholders.
    sqlx::query(
        "INSERT INTO installed_addons (id, kind, engine, version, enabled, manifest_hash)
         VALUES ('gateway-manual','gateway','manual','1.0.0',true,'builtin')",
    )
    .execute(&fresh)
    .await
    .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(MIGRATION))
        .execute(&fresh)
        .await
        .unwrap();
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM installed_addons WHERE id = 'gateway-manual'")
            .fetch_one(&fresh)
            .await
            .unwrap();
    assert_eq!(left, 0);
    fresh.close().await;
    sqlx::query(sqlx::AssertSqlSafe(drop))
        .execute(&s.pool)
        .await
        .unwrap();

    // An installation with products keeps its rows untouched.
    let before: Vec<(String, String, String, bool)> = sqlx::query_as(
        "SELECT id, source, manifest_hash, enabled FROM installed_addons ORDER BY id",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let products: i64 = sqlx::query_scalar("SELECT count(*) FROM products")
        .fetch_one(&s.pool)
        .await
        .unwrap();
    if products > 0 {
        sqlx::raw_sql(sqlx::AssertSqlSafe(MIGRATION))
            .execute(&s.pool)
            .await
            .unwrap();
        let after: Vec<(String, String, String, bool)> = sqlx::query_as(
            "SELECT id, source, manifest_hash, enabled FROM installed_addons ORDER BY id",
        )
        .fetch_all(&s.pool)
        .await
        .unwrap();
        assert_eq!(before, after);
    }
}

/// An existing placeholder row (enabled, no package) is replaced by
/// uploading the packaged addon, then installed and enabled like a fresh one.
#[tokio::test]
async fn upload_replaces_builtin_placeholder() {
    let (app, s) = app().await;
    let admin = sess(&app, &s, "admin").await;
    for id in ["provisioning-featherpanel", "gateway-manual"] {
        let zip = std::fs::read(pelty_api::wasm_addons::dist_dir().join(format!("{id}.zip")))
            .expect("bundled addon zip (cargo xtask addons)");
        let original = reg::get(&s.pool, id)
            .await
            .unwrap()
            .expect("installed by the harness");
        // Turn the row into what migration 0220 left on fresh installs.
        sqlx::query(
            "UPDATE installed_addons SET source = 'builtin', manifest_hash = 'builtin', version = '1.0.0', enabled = true WHERE id = $1",
        )
        .bind(id)
        .execute(&s.pool)
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(pelty_api::addons::addons_dir().join(id));
        pelty_api::wasm_addons::sync(&s).await.unwrap();
        if id == "provisioning-featherpanel" {
            assert!(s.provisioning.get("featherpanel").is_none());
        }

        let (st, b) = upload(&app, &admin, zip.clone()).await;
        assert_eq!(st, StatusCode::OK, "{id}: {b}");
        assert_eq!(
            b["status"], "available",
            "{id}: placeholder reads as not installed"
        );
        let row = reg::get(&s.pool, id).await.unwrap().unwrap();
        assert!(!row.enabled, "{id}: placeholder parked disabled");
        assert_eq!(row.manifest_hash, "builtin");
        // A restart must not adopt it behind the admin's back.
        pelty_api::wasm_addons::load_enabled(&s).await.unwrap();
        assert_eq!(
            reg::get(&s.pool, id).await.unwrap().unwrap().source,
            "builtin"
        );

        let approve: Vec<Value> = b["capabilities"]["dangerous"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/addons/{id}/install"),
            Some(json!({ "approve": approve })),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{id}: {:?}", r.body);
        assert_eq!(r.body["status"], "disabled");
        assert_eq!(r.body["integrity_ok"], true);
        let row = reg::get(&s.pool, id).await.unwrap().unwrap();
        assert_eq!(row.source, "disk");
        assert_eq!(row.manifest_hash, b["hash"].as_str().unwrap());
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/addons/{id}/enable"),
            None,
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{id}: {:?}", r.body);
        if id == "provisioning-featherpanel" {
            let r = call(
                &app,
                "GET",
                "/api/v1/admin/provisioning/modules",
                None,
                &admin.h(),
            )
            .await;
            assert!(
                r.body["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["id"] == "featherpanel"),
                "{:?}",
                r.body
            );
        } else {
            assert!(pelty_api::addons::gateway_enabled(&s, "manual")
                .await
                .unwrap());
        }
        // A second install is still refused; real (non-placeholder) rows
        // keep the normal upgrade rules.
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/addons/{id}/install"),
            Some(json!({})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::CONFLICT);
        let (st, b) = upload(&app, &admin, zip).await;
        assert_eq!(st, StatusCode::CONFLICT, "{id}: {b}");
        if !original.enabled {
            reg::set_enabled(&s.pool, id, false).await.unwrap();
        }
    }
}
