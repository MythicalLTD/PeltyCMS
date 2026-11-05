use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn module_declared_checkout_fields() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let (ah, chh) = (admin.h(), c.h());
    let murl = format!("/api/v1/admin/products/{pid}/module");
    let set = |m: &str| call(&app, "PUT", &murl, Some(json!({"module": m, "config": {"ssl_product": "dv"}})), &ah);
    let r = set("ssl_reseller").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    set("ssl_reseller").await; // idempotent

    let ex = call(&app, "GET", &format!("/api/v1/catalog/products/{pid}/extras"), None, &[]).await;
    let fields = ex.body["fields"].as_array().unwrap().clone();
    assert_eq!(fields.len(), 2, "{fields:?}");
    let cn = fields.iter().find(|f| f["name"] == "Domain (common name)").unwrap()["id"].as_str().unwrap().to_string();

    let order = |f: serde_json::Value| call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0].clone()}, "fields": f}]})), &chh);
    let r = order(json!({})).await;
    assert_eq!(r.body["error"]["code"], "error.field_required", "{:?}", r.body);
    let r = order(json!({cn.clone(): "not a domain"})).await;
    assert_eq!(r.body["error"]["code"], "error.invalid_field", "{:?}", r.body);
    let r = order(json!({cn.clone(): "shop.example.com"})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // The module sees the value under its own key.
    let order_id = r.body["order"]["id"].as_str().unwrap();
    let sid = call(&app, "POST", &format!("/api/v1/admin/orders/{order_id}/approve"), None, &admin.h()).await.body["services"][0].as_str().unwrap().parse().unwrap();
    let t = pelty_database::provisioning::target(&st.pool, sid).await.unwrap().unwrap();
    assert_eq!(t.custom_fields["common_name"], "shop.example.com");

    // Switching module drops its fields.
    assert_eq!(set("webhook").await.status, StatusCode::OK);
    let ex = call(&app, "GET", &format!("/api/v1/catalog/products/{pid}/extras"), None, &[]).await;
    assert!(ex.body["fields"].as_array().unwrap().is_empty());
}
