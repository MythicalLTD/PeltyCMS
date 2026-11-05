//! Vendor marketplace end to end against a local fake vendor (never a real
//! one) through the `vendor_reseller` WASM addon: catalog sync, retire,
//! import with markup (exact Decimal), reprice, order → create → suspend.
use crate::common;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::IntoResponse;
use common::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct V {
    log: Arc<Mutex<Vec<(String, String, String)>>>,
    shrink: Arc<Mutex<bool>>,
}

async fn vendor(State(v): State<V>, method: Method, uri: Uri, b: Bytes) -> axum::response::Response {
    let path = uri.path().to_string();
    v.log.lock().unwrap().push((method.to_string(), path.clone(), String::from_utf8_lossy(&b).to_string()));
    let ok = |x: Value| axum::Json(x).into_response();
    match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/catalog") => {
            let mut p = vec![json!({"sku": "ssl-dv", "name": "DV SSL", "cost": "4.50", "currency": "eur", "cycle": "annual"})];
            if !*v.shrink.lock().unwrap() {
                p.push(json!({"sku": "mail-pro", "name": "Mail Pro", "cost": "0.125", "currency": "EUR", "cycle": "monthly"}));
                p.push(json!({"sku": "bad sku!", "cost": "1", "currency": "EUR"}));
                p.push(json!({"sku": "neg", "cost": "-1", "currency": "EUR"}));
            }
            ok(json!({"products": p}))
        }
        ("POST", "/api/v1/orders") => ok(json!({"id": "vo-9", "status": "active"})),
        ("POST", "/api/v1/orders/vo-9/suspend") => ok(json!({})),
        _ => (StatusCode::NOT_FOUND, "no").into_response(),
    }
}

async fn drain(st: &pelty_api::AppState, sid: &str) -> String {
    for _ in 0..200 {
        let _ = pelty_api::worker::run_once(st, "vendor-test", 10).await.unwrap();
        let s: String = sqlx::query_scalar("SELECT provision_status FROM services WHERE id = $1::uuid").bind(sid).fetch_one(&st.pool).await.unwrap();
        if s != "pending" {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    "pending".into()
}

#[tokio::test]
async fn vendor_catalog_import_order_and_suspend() {
    let v = V::default();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let r = axum::Router::new().fallback(vendor).with_state(v.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;

    let c = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": "Vendor", "module": "vendor_reseller", "base_url": url, "api_key": "vk"})), &admin.h()).await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    let srv = c.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/servers/{srv}/vendor-catalog/sync"), None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    let s = call(&app, "POST", &format!("/api/v1/admin/servers/{srv}/vendor-catalog/sync"), None, &admin.h()).await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    assert_eq!(s.body["synced"], 2, "invalid rows skipped: {:?}", s.body);
    // A module without a catalog → 409.
    let lic = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": "L", "module": "licensing", "base_url": "https://x.example.com", "api_key": "lic-signing-secret-0123456789"})), &admin.h()).await.body["id"].as_str().unwrap().to_string();
    let n = call(&app, "POST", &format!("/api/v1/admin/servers/{lic}/vendor-catalog/sync"), None, &admin.h()).await;
    assert_eq!(n.body["error"]["code"], "error.module_no_catalog", "{:?}", n.body);

    let list = call(&app, "GET", &format!("/api/v1/admin/vendor-catalog?server_id={srv}"), None, &admin.h()).await.body;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let ssl = items.iter().find(|i| i["sku"] == "ssl-dv").unwrap();
    assert_eq!((ssl["cost"].as_str(), ssl["currency"].as_str()), (Some("4.5000"), Some("EUR")));
    let ssl_id = ssl["id"].as_str().unwrap().to_string();
    let mail_id = items.iter().find(|i| i["sku"] == "mail-pro").unwrap()["id"].as_str().unwrap().to_string();

    let (seed_pid, _, _) = seed_product(&app, &admin).await;
    let gid: uuid::Uuid = sqlx::query_scalar("SELECT group_id FROM products WHERE id = $1::uuid").bind(&seed_pid).fetch_one(&st.pool).await.unwrap();
    let bad = call(&app, "POST", &format!("/api/v1/admin/vendor-catalog/{ssl_id}/import"), Some(json!({"group_id": gid, "markup_percent": "-150"})), &admin.h()).await;
    assert_eq!(bad.body["error"]["code"], "error.invalid_markup");
    let im = call(&app, "POST", &format!("/api/v1/admin/vendor-catalog/{ssl_id}/import"), Some(json!({"group_id": gid, "markup_percent": "40"})), &admin.h()).await;
    assert_eq!(im.status, StatusCode::OK, "{:?}", im.body);
    let pid = im.body["product_id"].as_str().unwrap().to_string();
    let again = call(&app, "POST", &format!("/api/v1/admin/vendor-catalog/{ssl_id}/import"), Some(json!({"group_id": gid, "markup_percent": "40"})), &admin.h()).await;
    assert_eq!(again.body["error"]["code"], "error.vendor_already_imported");
    let price: rust_decimal::Decimal = sqlx::query_scalar("SELECT amount FROM product_prices WHERE product_id = $1::uuid AND cycle = 'annual'").bind(&pid).fetch_one(&st.pool).await.unwrap();
    assert_eq!(price.to_string(), "6.3000");
    let (module, cfg): (Option<String>, Value) = sqlx::query_as("SELECT module, module_config FROM products WHERE id = $1::uuid").bind(&pid).fetch_one(&st.pool).await.unwrap();
    assert_eq!((module.as_deref(), cfg["sku"].as_str()), (Some("vendor_reseller"), Some("ssl-dv")));
    // Half-up rounding to the currency exponent.
    let im2 = call(&app, "POST", &format!("/api/v1/admin/vendor-catalog/{mail_id}/import"), Some(json!({"group_id": gid, "markup_percent": "0", "name": "Mail"})), &admin.h()).await;
    assert_eq!(im2.status, StatusCode::OK, "{:?}", im2.body);
    let rp = call(&app, "PUT", &format!("/api/v1/admin/vendor-catalog/{ssl_id}/markup"), Some(json!({"markup_percent": "100"})), &admin.h()).await;
    assert_eq!(rp.body["price"], "9.00", "{:?}", rp.body);
    let l2 = call(&app, "GET", "/api/v1/admin/vendor-catalog", None, &admin.h()).await.body;
    let row = l2["items"].as_array().unwrap().iter().find(|i| i["id"] == ssl_id.as_str()).unwrap().clone();
    assert_eq!((row["price"].as_str(), row["markup_percent"].as_str()), (Some("9.0000"), Some("100.0000")));
    let mail = l2["items"].as_array().unwrap().iter().find(|i| i["id"] == mail_id.as_str()).unwrap().clone();
    assert_eq!((mail["price"].as_str(), mail["product_name"].as_str()), (Some("0.1300"), Some("Mail")));

    // Ordering the imported product provisions at the vendor.
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "annual"}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["id"].as_str().or(o.body["order"]["id"].as_str()).unwrap().to_string();
    let ap = call(&app, "POST", &format!("/api/v1/admin/orders/{order_id}/approve"), None, &admin.h()).await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    let sid = ap.body["services"][0].as_str().unwrap().to_string();
    assert_eq!(drain(&st, &sid).await, "ok");
    
    let state = call(&app, "GET", &format!("/api/v1/admin/services/{sid}/provisioning"), None, &admin.h()).await;
    assert_eq!(state.body["state"]["remote_id"], "vo-9", "{:?}", state.body);
    let created = v.log.lock().unwrap().iter().find(|(m, p, _)| m == "POST" && p == "/api/v1/orders").cloned().unwrap();
    let body: Value = serde_json::from_str(&created.2).unwrap();
    assert_eq!((body["sku"].as_str(), body["external_id"].as_str().map(|s| s.starts_with("pelty-"))), (Some("ssl-dv"), Some(true)));
    let s = call(&app, "POST", &format!("/api/v1/admin/services/{sid}/action"), Some(json!({"action": "suspend", "reason": "t"})), &admin.h()).await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    assert_eq!(drain(&st, &sid).await, "ok");
    assert!(v.log.lock().unwrap().iter().any(|(_, p, _)| p == "/api/v1/orders/vo-9/suspend"));

    // Vendor drops a SKU → unavailable, linked product kept, no re-import.
    *v.shrink.lock().unwrap() = true;
    let s = call(&app, "POST", &format!("/api/v1/admin/servers/{srv}/vendor-catalog/sync"), None, &admin.h()).await;
    assert_eq!(s.body["retired"], 1, "{:?}", s.body);
    let l3 = call(&app, "GET", "/api/v1/admin/vendor-catalog", None, &admin.h()).await.body;
    let mail = l3["items"].as_array().unwrap().iter().find(|i| i["id"] == mail_id.as_str()).unwrap().clone();
    assert_eq!((mail["available"].as_bool(), mail["product_id"].is_string()), (Some(false), true));
}
