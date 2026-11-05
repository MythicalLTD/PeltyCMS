//! Opt-in power capabilities end to end with the real `fixture-power`
//! component: install approval, generic `before:*` pre-events (modify,
//! cancel, secret redaction, money.modify with exact decimals + audit),
//! db.read / db.write / own-table SQL through the real host, jobs, the
//! auth provider and the refusals.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

const ID: &str = "integration-powerdemo";

async fn try_power(app: &axum::Router, admin: &Sess, body: Value) -> Value {
    let r = call(app, "POST", &format!("/api/v1/addons/{ID}/try"), Some(body), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

#[tokio::test]
async fn power_capabilities() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(ID).execute(&st.pool).await.unwrap();
    sqlx::query("DROP TABLE IF EXISTS addon_integration_powerdemo_notes").execute(&st.pool).await.unwrap();
    pelty_api::addons::accept_package(&st, &fixture_zip("fixture-power", "pelty-addon-fixture-power"), true).await.expect("fixture package");
    let base = format!("/api/v1/admin/addons/{ID}");

    // The catalogue lists every dangerous capability for the red warning.
    let r = call(&app, "GET", "/api/v1/admin/addons", None, &admin.h()).await;
    let e = r.body["items"].as_array().unwrap().iter().find(|a| a["id"] == ID).cloned().expect("listed");
    let dangerous: Vec<String> = serde_json::from_value(e["capabilities"]["dangerous"].clone()).unwrap();
    for d in ["http.any", "money.modify", "db.read", "db.write", "db.sql", "fs", "exec", "auth.provider", "hooks.any"] {
        assert!(dangerous.iter().any(|x| x == d), "{d} missing in {dangerous:?}");
    }

    // Install without (or with partial) approval: 422.
    for body in [None, Some(json!({"approve": ["db.read"]}))] {
        let r = call(&app, "POST", &format!("{base}/install"), body, &admin.h()).await;
        assert_eq!((r.status, r.body["error"]["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("error.addon_capability_approval")), "{:?}", r.body);
    }
    let r = call(&app, "POST", &format!("{base}/install"), Some(json!({ "approve": dangerous })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", &format!("{base}/enable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'addon.capabilities_approved' AND target = $1").bind(ID).fetch_one(&st.pool).await.unwrap();
    assert!(n >= 1);

    // db.read on a declared entity works; undeclared entity / users refused.
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let cid = o.body["order"]["customer_id"].as_str().unwrap().to_string();
    // checkout.discounts: 10.00 + 5.00 setup, minus the addon's 2.50, audited.
    assert_eq!(o.body["invoice"]["total"].as_str().and_then(|t| t.parse::<f64>().ok()), Some(12.5), "{:?}", o.body);
    let oid_s = o.body["order"]["id"].as_str().unwrap().to_string();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'order.addon_discounts' AND target = $1").bind(&oid_s).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n, 1);
    let line: Option<String> = sqlx::query_scalar("SELECT description FROM invoice_items WHERE invoice_id = $1::uuid AND unit_price < 0").bind(o.body["invoice"]["id"].as_str().unwrap()).fetch_optional(&st.pool).await.unwrap();
    assert_eq!(line.as_deref(), Some("Fixture loyalty, VPS S"));
    let r = try_power(&app, &admin, json!({"power": "db_read", "entity": "clients", "query": {"id": cid}})).await;
    assert_eq!(r["ok"].as_array().map(Vec::len), Some(1), "{r:?}");
    for ent in ["users", "tickets"] {
        let r = try_power(&app, &admin, json!({"power": "db_read", "entity": ent, "query": {}})).await;
        assert!(r["err"].as_str().is_some_and(|e| e.contains("capability not granted") || e.contains("unknown")), "{ent}: {r:?}");
    }

    // db.write through a typed, audited op; undeclared op refused.
    let r = try_power(&app, &admin, json!({"power": "db_write", "op": "client.note", "input": {"customer_id": cid, "text": "from addon"}})).await;
    assert!(r.get("ok").is_some(), "{r:?}");
    let a: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'addon.db_write'").fetch_one(&st.pool).await.unwrap();
    assert!(a >= 1);
    let r = try_power(&app, &admin, json!({"power": "db_write", "op": "ticket.status", "input": {}})).await;
    assert!(r["err"].as_str().is_some_and(|e| e.contains("capability not granted")), "{r:?}");

    // Free SQL only on own addon_<id>_ tables.
    for stmt in ["CREATE TABLE addon_integration_powerdemo_notes (id int, t text)", "INSERT INTO addon_integration_powerdemo_notes VALUES (1, 'a')"] {
        let r = try_power(&app, &admin, json!({"power": "sql", "statement": stmt, "params": []})).await;
        assert!(r.get("ok").is_some(), "{stmt}: {r:?}");
    }
    let r = try_power(&app, &admin, json!({"power": "sql", "statement": "SELECT t FROM addon_integration_powerdemo_notes WHERE id = $1", "params": [1]})).await;
    assert_eq!(r["ok"][0]["t"], "a", "{r:?}");
    for stmt in ["SELECT * FROM users", "DELETE FROM invoices", "SELECT pg_read_file('/etc/passwd')", "DROP TABLE addon_other_x"] {
        let r = try_power(&app, &admin, json!({"power": "sql", "statement": stmt, "params": []})).await;
        assert!(r.get("err").is_some(), "{stmt} must be refused: {r:?}");
    }

    // Jobs go through the job queue.
    let r = try_power(&app, &admin, json!({"power": "job", "op": "tick"})).await;
    assert_eq!(r["ok"], true, "{r:?}");
    let j: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'addon.job' AND payload->>'addon' = $1").bind(ID).fetch_one(&st.pool).await.unwrap();
    assert!(j >= 1);
    let r = try_power(&app, &admin, json!({"power": "job", "op": "nope"})).await;
    assert!(r.get("err").is_some(), "{r:?}");

    // exec: allowlisted program only (or blocked in containers).
    let r = try_power(&app, &admin, json!({"power": "exec", "program": "/bin/echo", "args": ["hi"]})).await;
    if !pelty_wasm::in_container() {
        assert_eq!(r["ok"]["stdout"], "hi\n", "{r:?}");
    }
    let r = try_power(&app, &admin, json!({"power": "exec", "program": "/bin/sh", "args": ["-c", "id"]})).await;
    assert!(r.get("err").is_some(), "{r:?}");

    // http *: private targets refused without http_private.
    let r = try_power(&app, &admin, json!({"power": "fetch", "url": "http://127.0.0.1:55432/"})).await;
    assert!(r.get("err").is_some(), "{r:?}");

    // Generic pre-event on a core route: money.modify doubles unit prices,
    // the description shows the password-like keys were not visible.
    let inv = json!({"customer_id": cid, "currency": "EUR", "items": [{"description": "Setup", "quantity": "1", "unit_price": "25.00", "tax_rate": "0"}]});
    let r = call(&app, "POST", "/api/v1/admin/invoices?pd=1", Some(inv.clone()), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let s = r.body.to_string();
    assert!(s.contains("50.00") && s.contains("[seen pw=false]"), "{s}");
    let m: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'addon.money_modified' AND metadata->>'addon' = $1").bind(ID).fetch_one(&st.pool).await.unwrap();
    assert!(m >= 1);
    // A float money value is not exact: the whole modification is refused.
    let r = call(&app, "POST", "/api/v1/admin/invoices?pd=1&bad=1", Some(inv.clone()), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let s = r.body.to_string();
    assert!(s.contains("25.00") && !s.contains("[seen"), "{s}");
    // Cancel: 409.
    let r = call(&app, "POST", "/api/v1/admin/invoices?pd=1&cancel=1", Some(inv), &admin.h()).await;
    assert_eq!((r.status, r.body["error"]["code"].clone()), (StatusCode::CONFLICT, json!("error.addon_hook_cancelled")));
    // Secrets: the addon overwrites the password, the host restores it.
    let r = call(&app, "POST", "/api/v1/auth/login?pd=1", Some(json!({"email": alice.email, "password": "Very-Long-Pass-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Auth provider: verified external credentials create a customer.
    let r = call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": "alice@ext.test", "password": "open-sesame"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": "alice@ext.test", "password": "wrong"})), &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{:?}", r.body);

    // Disable: the generic hook is gone.
    assert_eq!(call(&app, "POST", &format!("{base}/disable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let r = call(&app, "POST", "/api/v1/admin/invoices?pd=1&cancel=1", Some(json!({"customer_id": cid, "currency": "EUR", "items": [{"description": "X", "quantity": "1", "unit_price": "1.00", "tax_rate": "0"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(ID).execute(&st.pool).await.unwrap();
}
