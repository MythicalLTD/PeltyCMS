//! WASM addon extension points with real compiled fixtures:
//! pre-event hooks (cancel / modify / priority / fail-closed / timeout),
//! addon HTTP routes (public / client / admin, host auth + CSRF) and the
//! declarative UI index.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn install(app: &axum::Router, st: &pelty_api::AppState, admin: &Sess, dir: &str, krate: &str, id: &str) {
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(id).execute(&st.pool).await.unwrap();
    sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1").bind(id).execute(&st.pool).await.unwrap();
    pelty_api::addons::accept_package(st, &fixture_zip(dir, krate), true).await.expect("fixture package");
    let base = format!("/api/v1/admin/addons/{id}");
    let m = pelty_extensions::addon_pkg::inspect(&fixture_zip(dir, krate), pelty_api::addons::PKG_LIMITS).unwrap().manifest;
    let body = if m["capabilities"]["ui"].as_array().is_some_and(|u| u.iter().any(|x| x == "iframe")) {
        // Iframe UI needs an explicit admin approval.
        let r = call(app, "POST", &format!("{base}/install"), None, &admin.h()).await;
        assert_eq!((r.status, r.body["error"]["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("error.addon_iframe_approval")), "{:?}", r.body);
        Some(json!({"approve_iframe": true}))
    } else {
        None
    };
    let r = call(app, "POST", &format!("{base}/install"), body, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(app, "POST", &format!("{base}/enable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn hooks_routes_and_ui() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    install(&app, &st, &admin, "fixture-ext", "pelty-addon-fixture-ext", "integration-extdemo").await;
    install(&app, &st, &admin, "fixture-events", "pelty-addon-fixture-events", "integration-eventlog").await;

    // login.before: cancel by addon; others still log in.
    let blocked = format!("x{}@blocked.test", uuid::Uuid::new_v4().simple());
    st.auth.register(pelty_auth::RegisterInput { email: &blocked, password: "Very-Long-Pass-123", display_name: "B" }, "customer", None).await.unwrap();
    let r = call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": blocked, "password": "Very-Long-Pass-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.addon_hook_cancelled");
    let _bob = sess(&app, &st, "customer").await;

    // client.before_register: payload modified (display name upper-cased).
    let e = format!("r{}@example.com", uuid::Uuid::new_v4().simple());
    let r = call(&app, "POST", "/api/v1/auth/register", Some(json!({"email": e, "password": "Very-Long-Pass-123", "display_name": "mixed Case"})), &[]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let dn: String = sqlx::query_scalar("SELECT display_name FROM users WHERE email = $1").bind(&e).fetch_one(&st.pool).await.unwrap();
    assert_eq!(dn, "MIXED CASE");

    // order.before_place: continue; error is fail-closed; endless loop times out.
    let item = json!([{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]);
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": item})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let cid = o.body["order"]["customer_id"].as_str().unwrap().to_string();
    for coupon in ["BOOM", "SLOW"] {
        let t = std::time::Instant::now();
        let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": item, "coupon": coupon})), &alice.h()).await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{coupon}: {:?}", r.body);
        assert_eq!(r.body["error"]["code"], "error.addon_hook_failed");
        assert!(t.elapsed() < std::time::Duration::from_secs(4), "{coupon} took {:?}", t.elapsed());
    }

    // invoice.before_create: two addons in priority order; amounts untouched.
    let r = call(&app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "items": [{"description": "Setup", "quantity": "1", "unit_price": "25.00", "tax_rate": "0"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let s = r.body.to_string();
    assert!(s.contains("[x] Setup (2)"), "{s}");
    assert!(s.contains("25.00"), "amount kept: {s}");
    assert!(!s.contains("0.01"), "{s}");

    // Routes: public, params, query.
    let r = call(&app, "GET", "/api/v1/addons/integration-extdemo/ping?q=hi", None, &[]).await;
    assert_eq!((r.status, r.body.clone()), (StatusCode::OK, json!({"pong": true, "q": "hi"})));
    let r = call(&app, "GET", "/api/v1/addons/integration-extdemo/items/42", None, &[]).await;
    assert_eq!(r.body, json!({"id": "42"}));
    assert_eq!(call(&app, "GET", "/api/v1/addons/integration-extdemo/nope", None, &[]).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "DELETE", "/api/v1/addons/integration-extdemo/ping", None, &[]).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/v1/addons/integration-missing/ping", None, &[]).await.status, StatusCode::NOT_FOUND);

    // Client route: auth + CSRF by the host; per-user data in addon kv.
    let notes = "/api/v1/addons/integration-extdemo/notes";
    assert_eq!(call(&app, "GET", notes, None, &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "POST", notes, Some(json!({"text": "a"})), &[("cookie", alice.cookie.as_str())]).await.status, StatusCode::FORBIDDEN, "csrf required");
    assert_eq!(call(&app, "POST", notes, Some(json!({"text": "hello"})), &alice.h()).await.status, StatusCode::CREATED);
    assert_eq!(call(&app, "POST", notes, Some(json!({"text": ""})), &alice.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", notes, None, &alice.h()).await.body, json!([{"text": "hello"}]));
    assert_eq!(call(&app, "GET", notes, None, &_bob_h(&app, &st).await.h()).await.body, json!([]));

    // Admin route: customers are refused, admins with the permission served.
    let stats = "/api/v1/addons/integration-extdemo/stats";
    assert_eq!(call(&app, "GET", stats, None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "GET", stats, None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body[0]["value"].as_u64().unwrap() >= 3, "{:?}", r.body);

    // UI: addon pages/slots join the plugin UI index (same renderer);
    // admin pages need admin.access; strings served as plugin.<id>.*.
    let a: Value = call(&app, "GET", "/api/v1/plugins/ui", None, &admin.h()).await.body;
    let mine = |v: &Value, k: &str| v[k].as_array().unwrap().iter().filter(|p| p["plugin"] == "integration-extdemo").cloned().collect::<Vec<_>>();
    let ids: Vec<String> = mine(&a, "pages").iter().map(|p| p["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, vec!["stats", "notes", "panel"], "{a}");
    assert_eq!(mine(&a, "slots").len(), 5);
    let c: Value = call(&app, "GET", "/api/v1/plugins/ui", None, &alice.h()).await.body;
    let ids: Vec<String> = mine(&c, "pages").iter().map(|p| p["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, vec!["notes", "panel"]);
    let slots: Vec<Value> = mine(&c, "slots").iter().map(|s| s["slot"].clone()).collect();
    assert_eq!(slots, vec![json!("client.dashboard"), json!("client.*.after")]);
    // Iframe page: asset URL + bridge prefixes; frame widget inside a slot.
    let panel = mine(&c, "pages").into_iter().find(|p| p["id"] == "panel").unwrap();
    assert_eq!(panel["frame"], json!({"src": "/api/v1/addons/integration-extdemo/assets/ui.html", "height": 240}));
    assert_eq!(panel["bridge"], json!(["/api/v1/auth/me"]));
    assert!(mine(&a, "slots").iter().any(|s| s["slot"] == "admin.invoices.toolbar" && s["body"]["type"] == "frame" && s["order"] == 5));

    // Static assets: hashed package files only, locked-down headers.
    let (st_code, h, body) = raw(&app, "/api/v1/addons/integration-extdemo/assets/ui.html", &[]).await;
    assert_eq!(st_code, StatusCode::OK);
    assert!(h["content-type"].to_str().unwrap().starts_with("text/html"));
    let csp = h["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("connect-src 'none'") && csp.contains("sandbox allow-scripts allow-forms") && csp.contains("frame-ancestors 'self'"), "{csp}");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert_eq!(h["x-frame-options"], "SAMEORIGIN", "frameable by the panel only");
    assert!(String::from_utf8_lossy(&body).contains("pelty:request"));
    let (code, h, _) = raw(&app, "/api/v1/addons/integration-extdemo/assets/ui.css", &[]).await;
    assert_eq!((code, h["content-type"].to_str().unwrap()), (StatusCode::OK, "text/css; charset=utf-8"));
    for bad in ["assets/../addon.json", "assets/missing.html", "addon.json", "assets/%2e%2e/addon.json"] {
        let (code, _, _) = raw(&app, &format!("/api/v1/addons/integration-extdemo/{bad}"), &[]).await;
        assert_ne!(code, StatusCode::OK, "{bad}");
    }
    // A tampered file on disk is refused (hash checked on every read).
    let on_disk = pelty_api::addons::addons_dir().join("integration-extdemo/assets/ui.css");
    let orig = std::fs::read(&on_disk).unwrap();
    std::fs::write(&on_disk, b"body{}").unwrap();
    assert_eq!(raw(&app, "/api/v1/addons/integration-extdemo/assets/ui.css", &[]).await.0, StatusCode::NOT_FOUND);
    std::fs::write(&on_disk, orig).unwrap();

    // UI token: signed-in only, bound to addon + user; bridge calls carry it.
    assert_eq!(call(&app, "POST", "/api/v1/addons/integration-extdemo/ui-token", None, &[]).await.status, StatusCode::UNAUTHORIZED);
    let tok = call(&app, "POST", "/api/v1/addons/integration-extdemo/ui-token", None, &alice.h()).await;
    assert_eq!(tok.status, StatusCode::OK, "{:?}", tok.body);
    assert_eq!(tok.body["expires_in"], 300);
    let t = tok.body["token"].as_str().unwrap().to_string();
    let mut h = alice.h().to_vec();
    h.push(("x-pelty-ui-token", t.as_str()));
    assert_eq!(call(&app, "GET", notes, None, &h).await.status, StatusCode::OK);
    let mut hb = admin.h().to_vec();
    hb.push(("x-pelty-ui-token", t.as_str()));
    assert_eq!(call(&app, "GET", stats, None, &hb).await.status, StatusCode::FORBIDDEN, "token of another user");
    assert_eq!(call(&app, "POST", "/api/v1/addons/integration-eventlog/ui-token", None, &alice.h()).await.status, StatusCode::NOT_FOUND, "no ui.iframe capability");
    let ro: Value = call(&app, "GET", "/api/v1/i18n/ro", None, &[]).await.body;
    assert_eq!(ro["plugin.integration-extdemo.notes_title"], "Notițele mele");
    let en: Value = call(&app, "GET", "/api/v1/i18n/en", None, &[]).await.body;
    assert_eq!(en["plugin.integration-extdemo.notes_title"], "My notes");

    // A frame without the ui.iframe capability, or pointing at a file the
    // package does not ship, is rejected at install.
    let pkg = pelty_extensions::addon_pkg::inspect(&fixture_zip("fixture-ext", "pelty-addon-fixture-ext"), pelty_api::addons::PKG_LIMITS).unwrap().manifest;
    let mut no_cap = pkg.clone();
    no_cap["capabilities"]["ui"] = json!([]);
    no_cap["capabilities"]["bridge"] = json!([]);
    assert!(pelty_api::addons::validate_manifest(&st, &serde_json::from_value(no_cap).unwrap()).is_err());
    let mut missing = pkg.clone();
    missing["ui"]["pages"][2]["frame"]["src"] = json!("assets/nope.html");
    assert!(pelty_api::addons::validate_manifest(&st, &serde_json::from_value(missing).unwrap()).is_err());

    // Invalid UI (request to a core endpoint) is rejected at install.
    let mut bad = pelty_extensions::addon_pkg::inspect(&fixture_zip("fixture-ext", "pelty-addon-fixture-ext"), pelty_api::addons::PKG_LIMITS).unwrap().manifest;
    bad["ui"]["pages"][0]["body"] = json!({"type": "button", "action": {"type": "request", "path": "/api/v1/admin/users/1"}});
    let m: pelty_api::addons::Manifest = serde_json::from_value(bad).unwrap();
    assert!(pelty_api::addons::validate_manifest(&st, &m).is_err());

    // Disabled addon: hooks and routes are gone.
    assert_eq!(call(&app, "POST", "/api/v1/admin/addons/integration-extdemo/disable", None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "GET", "/api/v1/addons/integration-extdemo/ping", None, &[]).await.status, StatusCode::NOT_FOUND);
    let r = call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": blocked, "password": "Very-Long-Pass-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let a: Value = call(&app, "GET", "/api/v1/plugins/ui", None, &admin.h()).await.body;
    assert!(mine(&a, "pages").is_empty());
}

async fn raw(app: &axum::Router, uri: &str, headers: &[(&str, &str)]) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    use tower::ServiceExt;
    let mut b = axum::http::Request::builder().uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let r = app.clone().oneshot(b.body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let (p, body) = r.into_parts();
    (p.status, p.headers, axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec())
}

async fn _bob_h(app: &axum::Router, st: &pelty_api::AppState) -> Sess {
    sess(app, st, "customer").await
}
