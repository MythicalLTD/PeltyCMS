//! ABI 0.3 with real compiled fixtures: the four event phases on a core
//! request, custom addon events across two addons (priority, modify,
//! cancel, ignore_cancelled + uncancel, after/finished delivery), mixins
//! (around, after, return, schema guard, kill switch) and the UI paths
//! (declarative page, slot, iframe page + asset).
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn install(
    app: &axum::Router,
    st: &pelty_api::AppState,
    admin: &Sess,
    dir: &str,
    krate: &str,
    id: &str,
) {
    sqlx::query("DELETE FROM installed_addons WHERE id = $1")
        .bind(id)
        .execute(&st.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1")
        .bind(id)
        .execute(&st.pool)
        .await
        .unwrap();
    pelty_api::addons::accept_package(st, &fixture_zip(dir, krate), true)
        .await
        .expect("fixture package");
    let base = format!("/api/v1/admin/addons/{id}");
    let r = call(app, "GET", "/api/v1/admin/addons", None, &admin.h()).await;
    let e = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .cloned()
        .expect("listed");
    let dangerous: Vec<String> =
        serde_json::from_value(e["capabilities"]["dangerous"].clone()).unwrap();
    if !dangerous.is_empty() {
        let r = call(app, "POST", &format!("{base}/install"), None, &admin.h()).await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{id} must need approval: {:?}",
            r.body
        );
    }
    let r = call(
        app,
        "POST",
        &format!("{base}/install"),
        Some(json!({ "approve": dangerous })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call(app, "POST", &format!("{base}/enable"), None, &admin.h())
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

async fn log(app: &axum::Router, s: &Sess) -> std::collections::HashMap<String, u64> {
    let r = call(
        app,
        "GET",
        "/api/v1/addons/integration-lifecycle/log",
        None,
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["key"].as_str().unwrap().to_string(),
                x["value"].as_u64().unwrap(),
            )
        })
        .collect()
}

async fn wait_for(
    app: &axum::Router,
    s: &Sess,
    key: &str,
    want: u64,
) -> std::collections::HashMap<String, u64> {
    for _ in 0..100 {
        let l = log(app, s).await;
        if l.get(key).copied().unwrap_or(0) >= want {
            return l;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("{key} never reached {want}: {:?}", log(app, s).await);
}

#[tokio::test]
async fn lifecycle_events_and_mixins() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let _ = pelty_api::addon_events::set_mixins_enabled(&st, "integration-lifecycle", true).await;
    install(
        &app,
        &st,
        &admin,
        "fixture-lifecycle",
        "pelty-addon-fixture-lifecycle",
        "integration-lifecycle",
    )
    .await;
    install(
        &app,
        &st,
        &admin,
        "fixture-listener",
        "pelty-addon-fixture-listener",
        "integration-listener",
    )
    .await;

    // Mixins and custom hooks are dangerous: listed for approval.
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/addons/integration-lifecycle",
        None,
        &admin.h(),
    )
    .await;
    let d = r.body["capabilities"]["dangerous"].to_string();
    for want in ["mixins", "mixins.return", "hooks.any", "ui.iframe"] {
        assert!(d.contains(&format!("\"{want}\"")), "{want} in {d}");
    }

    // Request lifecycle: before cancels; otherwise during/after/finished run.
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"city": "Blocked"})),
        &alice.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("error.addon_hook_cancelled")),
        "{:?}",
        r.body
    );
    wait_for(&app, &alice, "finished:cancelled", 1).await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"city": "Cluj", "organization": "acme", "last_name": "Pop"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // Observe-only phases run in separate background tasks: finished may
    // become visible before after, so wait for both rather than assuming order.
    wait_for(&app, &alice, "finished:ok", 1).await;
    let l = wait_for(&app, &alice, "phase:after", 1).await;
    for k in [
        "phase:before",
        "phase:during",
        "phase:after",
        "mixin:around:before",
        "mixin:around:after",
    ] {
        assert!(l[k] >= 1, "{k}: {l:?}");
    }
    // around/before rewrote the input (organization upper-cased) and its
    // state reached the after half.
    let org: String = sqlx::query_scalar("SELECT c.organization FROM customers c JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1").bind(&alice.email).fetch_one(&st.pool).await.unwrap();
    assert_eq!(org, "ACME");
    let raw = call(
        &app,
        "GET",
        "/api/v1/addons/integration-lifecycle/log",
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(raw.body["around_state"], json!("{\"seen\":\"around\"}"));

    // after mixin edits the response within its schema; return mixin
    // replaces it; DB is untouched.
    let p = call(&app, "GET", "/api/v1/client/profile", None, &alice.h()).await;
    assert_eq!(p.status, StatusCode::OK);
    assert_eq!(p.body["city"], "Cluj (mixin)", "{:?}", p.body);
    assert_eq!(p.body["last_name"], "Returned");
    // A mixin that adds a key is ignored (schema guard).
    let p2 = call(
        &app,
        "GET",
        "/api/v1/client/profile?break=1",
        None,
        &alice.h(),
    )
    .await;
    assert!(p2.body.get("evil").is_none(), "{:?}", p2.body);
    assert_eq!(p2.body["last_name"], "Returned");

    // Kill switch: mixins off → raw response.
    let r = call(&app, "GET", "/api/v1/admin/addons/mixins", None, &admin.h()).await;
    assert!(
        r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["addon"] == "integration-lifecycle" && i["enabled"] == true),
        "{:?}",
        r.body
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/integration-lifecycle/mixins",
            Some(json!({"enabled": false})),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/integration-lifecycle/mixins",
            Some(json!({"enabled": false})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let p = call(&app, "GET", "/api/v1/client/profile", None, &alice.h()).await;
    assert_eq!(
        (p.body["city"].clone(), p.body["last_name"].clone()),
        (json!("Cluj"), json!("Pop")),
        "{:?}",
        p.body
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/integration-lifecycle/mixins",
            Some(json!({"enabled": true})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // Custom events across addons.
    let greet = "/api/v1/addons/integration-lifecycle/greet";
    let r = call(
        &app,
        "POST",
        greet,
        Some(json!({"name": "bob"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["cancelled"].clone(),
            r.body["payload"]["name"].clone()
        ),
        (json!(false), json!("bob!")),
        "listener modified: {:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        greet,
        Some(json!({"name": "nope"})),
        &alice.h(),
    )
    .await;
    assert_eq!(
        (r.body["cancelled"].clone(), r.body["reason"].clone()),
        (json!(true), json!("not welcome")),
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        greet,
        Some(json!({"name": "rescue"})),
        &alice.h(),
    )
    .await;
    assert_eq!(
        r.body["cancelled"],
        json!(false),
        "uncancelled by an ignore_cancelled listener: {:?}",
        r.body
    );
    // Emitting another addon's event is refused.
    let r = call(
        &app,
        "POST",
        "/api/v1/addons/integration-lifecycle/greet-other",
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    assert!(
        r.body["error"]
            .as_str()
            .is_some_and(|e| e.contains("capability not granted")),
        "{:?}",
        r.body
    );
    // after: not for the cancelled one; finished: all three.
    let mut ok = false;
    for _ in 0..100 {
        let r = call(
            &app,
            "GET",
            "/api/v1/addons/integration-listener/log",
            None,
            &admin.h(),
        )
        .await;
        if r.body["after"] == 2 && r.body["finished"] == 3 && r.body["before"] == 3 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        ok,
        "{:?}",
        call(
            &app,
            "GET",
            "/api/v1/addons/integration-listener/log",
            None,
            &admin.h()
        )
        .await
        .body
    );

    // UI: declarative page, slot and iframe page all exposed to clients.
    let c: Value = call(&app, "GET", "/api/v1/plugins/ui", None, &alice.h())
        .await
        .body;
    let mine = |k: &str| {
        c[k].as_array()
            .unwrap()
            .iter()
            .filter(|p| p["plugin"] == "integration-lifecycle")
            .cloned()
            .collect::<Vec<_>>()
    };
    let pages = mine("pages");
    assert!(pages
        .iter()
        .any(|p| p["id"] == "log" && p["body"]["type"] == "stack"));
    assert!(pages.iter().any(|p| p["id"] == "frame"
        && p["frame"]["src"] == "/api/v1/addons/integration-lifecycle/assets/ui.html"));
    let slots: Vec<Value> = mine("slots").iter().map(|s| s["slot"].clone()).collect();
    assert_eq!(
        slots,
        vec![
            json!("client.invoices.toolbar"),
            json!("client.dashboard.after")
        ]
    );
    let (code, h, _) = raw_get(
        &app,
        "/api/v1/addons/integration-lifecycle/assets/ui.html",
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(h["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("sandbox allow-scripts"));

    // Disabling removes hooks and mixins at once.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/addons/integration-lifecycle/disable",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let p = call(&app, "GET", "/api/v1/client/profile", None, &alice.h()).await;
    assert_eq!(p.body["city"], "Cluj");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/client/profile",
            Some(json!({"city": "Blocked"})),
            &alice.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let _ = call(
        &app,
        "POST",
        "/api/v1/admin/addons/integration-listener/disable",
        None,
        &admin.h(),
    )
    .await;
}
async fn raw_get(
    app: &axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    use tower::ServiceExt;
    let mut b = axum::http::Request::builder().uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let r = app
        .clone()
        .oneshot(b.body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();
    let (p, body) = r.into_parts();
    (
        p.status,
        p.headers,
        axum::body::to_bytes(body, usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
}
