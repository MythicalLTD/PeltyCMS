use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn product_stock_limits_and_backorders() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let (ah, chh) = (admin.h(), c.h());
    let surl = format!("/api/v1/admin/catalog/products/{pid}/stock");
    let stock = |b: serde_json::Value| call(&app, "PUT", &surl, Some(b), &ah);
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/catalog/products/{pid}/stock"), Some(json!({"stock_enabled": true, "stock_qty": 1, "allow_backorder": false})), &c.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(stock(json!({"stock_enabled": true, "stock_qty": 1, "allow_backorder": false})).await.status, StatusCode::OK);

    let order = || call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0].clone()}}]})), &chh);
    let qty = || async {
        let p = call(&app, "GET", "/api/v1/catalog", None, &[]).await;
        let list = p.body.get("products").cloned().unwrap_or(p.body.clone());
        list.as_array().unwrap().iter().find(|x| x["id"] == pid.as_str()).unwrap().clone()
    };
    assert_eq!(qty().await["stock_qty"], 1, "public catalog exposes stock");
    let r = order().await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(qty().await["stock_qty"], 0);

    // Sold out: refused, nothing written.
    let r = order().await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.out_of_stock", "{:?}", r.body);
    assert_eq!(qty().await["stock_qty"], 0);

    // Backorder keeps selling below zero.
    stock(json!({"stock_enabled": true, "stock_qty": 0, "allow_backorder": true})).await;
    assert_eq!(order().await.status, StatusCode::OK);
    assert_eq!(qty().await["stock_qty"], -1);

    // Two units in one cart with only one left fails atomically.
    stock(json!({"stock_enabled": true, "stock_qty": 1, "allow_backorder": false})).await;
    let line = json!({"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0].clone()}});
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [line.clone(), line]})), &c.h()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(qty().await["stock_qty"], 1, "rolled back");

    // Tracking off: unlimited.
    stock(json!({"stock_enabled": false, "stock_qty": 0, "allow_backorder": false})).await;
    assert_eq!(order().await.status, StatusCode::OK);
    assert_eq!(stock(json!({"stock_enabled": true, "stock_qty": 99_999_999, "allow_backorder": false})).await.status, StatusCode::UNPROCESSABLE_ENTITY);
}
