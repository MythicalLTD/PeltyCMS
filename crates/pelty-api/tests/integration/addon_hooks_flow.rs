//! Hook power with the `fixture-hooks` addon: per-listener deadlines that
//! really interrupt the wasm store, call statistics and the circuit
//! breaker, filters, core-function mixins and the call context.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

pub async fn install(app: &axum::Router, st: &pelty_api::AppState, admin: &Sess, dir: &str, krate: &str, id: &str) {
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(id).execute(&st.pool).await.unwrap();
    sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1").bind(id).execute(&st.pool).await.unwrap();
    pelty_api::addons::accept_package(st, &fixture_zip(dir, krate), true).await.expect("fixture package");
    let base = format!("/api/v1/admin/addons/{id}");
    let r = call(app, "GET", &base, None, &admin.h()).await;
    let dangerous: Vec<String> = serde_json::from_value(r.body["capabilities"]["dangerous"].clone()).unwrap_or_default();
    let r = call(app, "POST", &format!("{base}/install"), Some(json!({ "approve": dangerous })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(app, "POST", &format!("{base}/enable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
}

async fn health(app: &axum::Router, admin: &Sess) -> Value {
    let r = call(app, "GET", "/api/v1/admin/addons/integration-hooks/health", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["stats"].clone()
}

#[tokio::test]
async fn hook_deadline_stats_and_breaker() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    install(&app, &st, &admin, "fixture-hooks", "pelty-addon-fixture-hooks", "integration-hooks").await;
    let r = call(&app, "POST", "/api/v1/admin/addons/integration-hooks/health/reset", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    // A spinning hook is interrupted at its 150 ms deadline (the sandbox
    // allows 90 s) and, being fail-open, the request goes through.
    let t = std::time::Instant::now();
    let r = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"city": "Spin"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "hook was not interrupted: {:?}", t.elapsed());
    let h = health(&app, &admin).await;
    assert_eq!((h["calls"].as_u64(), h["failures"].as_u64(), h["timeouts"].as_u64()), (Some(1), Some(1), Some(1)), "{h}");
    assert!(h["last_error"].as_str().unwrap_or("").contains("timed out"), "{h}");

    // A good call resets the consecutive count.
    let r = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"city": "Cluj"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(health(&app, &admin).await["consecutive_failures"], 0);

    // Five failures in a row trip the breaker; the next call is skipped.
    for _ in 0..5 {
        call(&app, "PUT", "/api/v1/client/profile", Some(json!({"city": "Spin"})), &alice.h()).await;
    }
    let h = health(&app, &admin).await;
    assert!(h["tripped_until"].is_i64(), "{h}");
    let t = std::time::Instant::now();
    let r = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"city": "Spin"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(t.elapsed() < std::time::Duration::from_millis(140), "breaker should skip the hook: {:?}", t.elapsed());
    assert_eq!(health(&app, &admin).await["skipped"], 1);
    let r = call(&app, "GET", "/api/v1/admin/addons/health", None, &admin.h()).await;
    assert!(r.body["items"].as_array().unwrap().iter().any(|x| x["addon"] == "integration-hooks"), "{:?}", r.body);
    assert_eq!(call(&app, "POST", "/api/v1/admin/addons/integration-hooks/health/reset", None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    assert!(health(&app, &admin).await["tripped_until"].is_null());
    // Same addon install (tests sharing an addon id must not run in parallel).
    catalog_filters(&app, &admin).await;
    ui_overrides(&app, &admin, &alice).await;
    filter_points(&app, &st, &admin, &alice).await;
    // Last: removes the integration-hooks install.
    ticket_pre_events(&app, &st, &alice).await;
}

/// Addon-defined filter points over WIT (ABI 0.4): `integration-hooks`
/// applies `integration-hooks:greeting`; `integration-points` rewrites it.
/// Both subscribe to their own points, which the host must skip (no
/// self-recursion, no `[self!]`).
async fn filter_points(app: &axum::Router, st: &pelty_api::AppState, admin: &Sess, alice: &Sess) {
    let r = call(app, "GET", "/api/v1/addons/integration-hooks/greeting", None, &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["text"], "hello", "no other subscriber yet, own one skipped: {:?}", r.body);
    install(app, st, admin, "fixture-points", "pelty-addon-fixture-points", "integration-points").await;
    let r = call(app, "GET", "/api/v1/addons/integration-hooks/greeting", None, &alice.h()).await;
    assert_eq!(r.body["text"], "hello world (loop n=0)", "{:?}", r.body);
    sqlx::query("DELETE FROM installed_addons WHERE id = 'integration-points'").execute(&st.pool).await.unwrap();
}

/// `catalog.public` filter: hiding works; an unapproved price change makes
/// the host drop that addon's whole output.
async fn catalog_filters(app: &axum::Router, admin: &Sess) {
    let retire = |id: String| async move {
        let r = call(app, "PATCH", &format!("/api/v1/admin/catalog/products/{id}"), Some(json!({"status": "retired"})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    };
    // An interrupted earlier run can leave a `pricey-fixture` product behind,
    // which (by design) makes the host drop the fixture's whole output.
    let r = call(app, "GET", "/api/v1/catalog", None, &[]).await;
    for p in r.body["products"].as_array().cloned().unwrap_or_default() {
        let name = p["name"].as_str().unwrap_or("");
        if ["hidden-fixture ", "pricey-fixture ", "shown "].iter().any(|x| name.starts_with(x)) {
            retire(p["id"].as_str().unwrap_or("").to_string()).await;
        }
    }
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let g = call(app, "POST", "/api/v1/admin/catalog/groups", Some(json!({"name": "Filters", "slug": format!("flt{tag}")})), &admin.h()).await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let group = g.body["id"].as_str().unwrap().to_string();
    let product = |name: String| {
        let group = group.clone();
        async move {
            let slug = format!("p{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
            let p = call(app, "POST", "/api/v1/admin/catalog/products", Some(json!({"group_id": group, "name": name, "slug": slug})), &admin.h()).await;
            assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
            let id = p.body["id"].as_str().unwrap().to_string();
            let r = call(app, "PUT", &format!("/api/v1/admin/catalog/products/{id}/prices"), Some(json!({"prices": [{"currency": "EUR", "cycle": "monthly", "amount": "5.00"}]})), &admin.h()).await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            id
        }
    };
    let names = |b: &Value| -> Vec<String> { b["products"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str().map(String::from)).collect() };
    let hidden = product(format!("hidden-fixture {tag}")).await;
    let shown = product(format!("shown {tag}")).await;
    let r = call(app, "GET", "/api/v1/catalog", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let n = names(&r.body);
    assert!(n.contains(&format!("shown {tag}")) && !n.contains(&format!("hidden-fixture {tag}")), "{n:?}");

    let pricey = product(format!("pricey-fixture {tag}")).await;
    let r = call(app, "GET", "/api/v1/catalog", None, &[]).await;
    assert!(names(&r.body).contains(&format!("hidden-fixture {tag}")), "output with a money change must be dropped");
    assert!(!r.body.to_string().contains("999999"), "price change without money.modify leaked");
    // Products can't be deleted; retiring takes them out of the public catalog.
    for id in [hidden, shown, pricey] {
        retire(id).await;
    }
}

/// `ui.overrides` (with `ui: ["override"]`, approved as `ui.override`):
/// the client sees the page override and the hidden nav path.
async fn ui_overrides(app: &axum::Router, admin: &Sess, alice: &Sess) {
    let r = call(app, "GET", "/api/v1/admin/addons", None, &admin.h()).await;
    let me = r.body["items"].as_array().and_then(|v| v.iter().find(|a| a["id"] == "integration-hooks").cloned()).unwrap_or_default();
    let danger = me["capabilities"]["dangerous"].to_string();
    assert!(danger.contains("ui.override") && danger.contains("filters"), "{me}");
    let c = call(app, "GET", "/api/v1/plugins/ui", None, &alice.h()).await.body;
    let pages = c["overrides"]["pages"].as_array().cloned().unwrap_or_default();
    assert!(pages.iter().any(|o| o["plugin"] == "integration-hooks" && o["route"] == "client.hooks-fixture" && o["page"] == "override"), "{c}");
    assert!(c["overrides"]["hide_nav"].as_array().unwrap().iter().any(|h| h["path"] == "/hooks-fixture-hidden"), "{c}");
}

/// Pre-events on support tickets: cancel, rewrite subject/priority, redact replies.
async fn ticket_pre_events(app: &axum::Router, st: &pelty_api::AppState, alice: &Sess) {
    const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";
    let (app, alice) = (app.clone(), alice);

    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "buy spam-fixture now", "body": "x"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.addon_hook_cancelled", "{:?}", r.body);

    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "triage: Server down", "body": "pw is hunter2", "priority": "low"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ticket"]["subject"], "[triaged] Server down");
    assert_eq!(r.body["ticket"]["priority"], "urgent");
    let id = r.body["ticket"]["id"].as_str().unwrap().to_string();

    // Core-function mixin (`fn ticket.open`, around): input rewritten before
    // the ticket is stored, output marked after (state carried over).
    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "fnmix: Billing question", "body": "x"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ticket"]["subject"], "[fn] Billing question ✓", "{:?}", r.body);
    let fid = r.body["ticket"]["id"].as_str().unwrap().to_string();
    let g = call(&app, "GET", &format!("/api/v1/client/tickets/{fid}"), None, &alice.h()).await;
    assert_eq!(g.body["ticket"]["subject"], "[fn] Billing question", "stored subject is the rewritten input");
    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "plain one", "body": "x"})), &alice.h()).await;
    assert_eq!(r.body["ticket"]["subject"], "plain one");

    let r = call(&app, "POST", &format!("/api/v1/client/tickets/{id}/reply"), Some(json!({"body": "again: hunter2"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "GET", &format!("/api/v1/client/tickets/{id}"), None, &alice.h()).await;
    let bodies: Vec<&str> = r.body["messages"].as_array().unwrap().iter().filter_map(|m| m["body"].as_str()).collect();
    assert!(bodies.contains(&"again: [redacted]"), "{bodies:?}");
    assert!(!bodies.iter().any(|b| b.contains("hunter2") && b.starts_with("again")), "{bodies:?}");
    sqlx::query("DELETE FROM installed_addons WHERE id = 'integration-hooks'").execute(&st.pool).await.unwrap();
}
