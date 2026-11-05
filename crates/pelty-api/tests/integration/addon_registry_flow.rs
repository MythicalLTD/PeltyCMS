use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

/// Test-only engines registered under unique ids so this test never toggles
/// the built-ins other tests (sharing the DB) depend on.
async fn app_with_test_engines() -> (axum::Router, pelty_api::AppState) {
    let (app, st) = app().await;
    static ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    ONCE.get_or_init(|| setup(st.clone())).await;
    (app, st)
}

/// Once per test binary: tests in this file run in parallel.
async fn setup(st: pelty_api::AppState) {
    // Clean slate for the test-only ids (re-runs on the same DB).
    for id in [
        "provisioning-addontestmod",
        "gateway-addontestpay",
        "hello-webhook",
    ] {
        sqlx::query("DELETE FROM addon_migrations WHERE addon_id = $1")
            .bind(id)
            .execute(&st.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM installed_addons WHERE id = $1")
            .bind(id)
            .execute(&st.pool)
            .await
            .unwrap();
    }
    sqlx::query("DROP TABLE IF EXISTS addon_hello_webhook_log")
        .execute(&st.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM payment_gateways WHERE id = 'addontestpay'")
        .execute(&st.pool)
        .await
        .unwrap();
    // WASM fixtures with unique engine ids, uploaded like an admin would.
    for (dir, krate) in [
        ("fixture-prov", "pelty-addon-fixture-prov"),
        ("fixture-pay", "pelty-addon-fixture-pay"),
    ] {
        pelty_api::addons::accept_package(&st, &fixture_zip(dir, krate), true)
            .await
            .expect("fixture package");
    }
    copy_disk_addon("hello-webhook");
    // Source checkouts may use CRLF; package the copied fixture's actual bytes.
    pelty_extensions::addon_pkg::rehash(&pelty_api::addons::addons_dir().join("hello-webhook"))
        .unwrap();
}

#[tokio::test]
async fn provisioning_addon_lifecycle_gates_module_use() {
    let (app, st) = app_with_test_engines().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let (pid, _, _) = seed_product(&app, &admin).await;

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/addons", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let l = call(&app, "GET", "/api/v1/admin/addons", None, &admin.h()).await;
    assert_eq!(l.status, StatusCode::OK);
    let items = l.body["items"].as_array().unwrap();
    let find = |id: &str| items.iter().find(|i| i["id"] == id).cloned().unwrap();
    assert_eq!(find("provisioning-addontestmod")["status"], "available");
    assert_eq!(find("provisioning-pterodactyl")["status"], "enabled");
    assert_eq!(find("gateway-stripe")["kind"], "gateway");

    let uri = format!("/api/v1/admin/products/{pid}/module");
    let set = json!({"module": "addontestmod"});
    // Not installed → cannot be chosen.
    let r = call(&app, "PUT", &uri, Some(set.clone()), &admin.h()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(
        r.body["error"]["code"], "error.addon_not_installed",
        "{:?}",
        r.body
    );

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/addons/provisioning-addontestmod/install",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "disabled");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/provisioning-addontestmod/install",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    // Installed but disabled → still refused.
    assert_eq!(
        call(&app, "PUT", &uri, Some(set.clone()), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/provisioning-addontestmod/enable",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let r = call(&app, "PUT", &uri, Some(set), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // In use → uninstall refused; disable gates it at runtime.
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/addons/provisioning-addontestmod",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/provisioning-addontestmod/disable",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(
        !pelty_api::addons::provisioning_enabled(&st, "addontestmod")
            .await
            .unwrap()
    );
    assert_eq!(
        call(&app, "PUT", &uri, Some(json!({"module": null})), &admin.h())
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/addons/provisioning-addontestmod",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let d = call(
        &app,
        "GET",
        "/api/v1/admin/addons/provisioning-addontestmod",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(d.body["status"], "available");
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'addon.%' AND target = 'provisioning-addontestmod'")
        .fetch_one(&st.pool).await.unwrap();
    assert!(audit >= 4, "audit rows: {audit}");
}

#[tokio::test]
async fn gateway_addon_must_be_installed_to_enable_or_pay() {
    let (app, st) = app_with_test_engines().await;
    let admin = sess(&app, &st, "admin").await;
    let put = |on: bool| json!({"enabled": on, "config": {}});
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/gateways/addontestpay",
        Some(put(true)),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.addon_not_installed");
    assert!(!pelty_api::addons::gateway_enabled(&st, "addontestpay")
        .await
        .unwrap());

    for step in ["install", "enable"] {
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/addons/gateway-addontestpay/{step}"),
            None,
            &admin.h(),
        )
        .await;
        assert!(r.status.is_success(), "{step}: {:?}", r.body);
    }
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/gateways/addontestpay",
        Some(put(true)),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert!(pelty_api::addons::gateway_enabled(&st, "addontestpay")
        .await
        .unwrap());

    // Disabling the addon switches the gateway row off too.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/gateway-addontestpay/disable",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let on: bool =
        sqlx::query_scalar("SELECT enabled FROM payment_gateways WHERE id = 'addontestpay'")
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert!(!on);
    let visible =
        pelty_api::routes::gateways::eligible_for(&st, "EUR", rust_decimal::Decimal::new(1000, 2))
            .await
            .ok()
            .expect("eligible");
    assert!(!visible.iter().any(|g| g["id"] == "addontestpay"));
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/addons/gateway-addontestpay",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn disk_addon_integrity_migrations_and_settings() {
    let (app, st) = app_with_test_engines().await;
    let admin = sess(&app, &st, "admin").await;
    let d = call(
        &app,
        "GET",
        "/api/v1/admin/addons/hello-webhook",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    assert_eq!(d.body["source"], "disk");
    assert_eq!(d.body["settings_schema"].as_array().unwrap().len(), 3);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/addons/hello-webhook/settings",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/addons/hello-webhook/install",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let t: bool = sqlx::query_scalar("SELECT to_regclass('addon_hello_webhook_log') IS NOT NULL")
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert!(t, "addon migration ran");
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM addon_migrations WHERE addon_id = 'hello-webhook'",
    )
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);

    let s = "/api/v1/admin/addons/hello-webhook/settings";
    assert_eq!(
        call(&app, "PUT", s, Some(json!({})), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            s,
            Some(json!({"endpoint": "https://x", "mode": "bogus"})),
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
            s,
            Some(json!({"endpoint": "https://x", "nope": 1})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let ok = call(
        &app,
        "PUT",
        s,
        Some(json!({"endpoint": "https://hooks.example", "verbose": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(
        ok.body["values"],
        json!({"endpoint": "https://hooks.example", "mode": "test", "verbose": true})
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/hello-webhook/enable",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/addons/hello-webhook",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn tampered_disk_addon_is_rejected() {
    let tmp = std::env::temp_dir().join(format!("pelty-addons-{}", std::process::id()));
    let dir = tmp.join("bad-addon");
    std::fs::create_dir_all(dir.join("migrations")).unwrap();
    std::fs::write(
        dir.join("migrations/1.sql"),
        "CREATE TABLE addon_bad_addon_x (id int);",
    )
    .unwrap();
    std::fs::write(
        dir.join("addon.json"),
        serde_json::to_vec(&json!({
            "id": "bad-addon", "name": "Bad", "version": "1", "kind": "plugin", "engine": "bad",
            "migrations": ["migrations/1.sql"], "files": {"migrations/1.sql": "00"}
        }))
        .unwrap(),
    )
    .unwrap();
    let (_a, st) = app().await;
    let e = pelty_api::addons::CatalogEntry {
        manifest: serde_json::from_slice(&std::fs::read(dir.join("addon.json")).unwrap()).unwrap(),
        source: "disk",
        hash: "x".into(),
        dir: Some(dir.clone()),
    };
    pelty_api::addons::validate_manifest(&st, &e.manifest).unwrap();
    assert!(
        pelty_api::addons::verify(&e).is_err(),
        "hash mismatch must be rejected"
    );
    let mut bad_perm = e.manifest.clone();
    bad_perm.permissions = vec!["root".into()];
    assert!(pelty_api::addons::validate_manifest(&st, &bad_perm).is_err());
    let mut bad_engine = e.manifest.clone();
    bad_engine.kind = "gateway".into();
    bad_engine.engine = "nope".into();
    assert!(pelty_api::addons::validate_manifest(&st, &bad_engine).is_err());
    std::fs::remove_dir_all(tmp).ok();
}
