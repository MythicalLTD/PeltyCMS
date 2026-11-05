use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn currencies_rates_default_and_price_sync() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let (pid, _oid, _ch) = seed_product(&app, &admin).await;
    let code = fresh_currency_code(&app, &admin, 'Q').await;

    assert_eq!(call(&app, "GET", "/api/v1/admin/currencies", None, &staff.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", "/api/v1/admin/currencies", Some(json!({"code": code, "exponent": 2, "enabled": true, "rate": "0"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/currencies", Some(json!({"code": code.to_lowercase(), "exponent": 2, "enabled": true, "rate": "4.975", "suffix": " Q"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", "/api/v1/admin/currencies", Some(json!({"code": code, "exponent": 2, "enabled": true, "rate": "1"})), &admin.h()).await.status, StatusCode::CONFLICT);
    // Default currency is locked to rate 1 / enabled.
    assert_eq!(call(&app, "PUT", "/api/v1/admin/currencies/EUR", Some(json!({"exponent": 2, "enabled": true, "rate": "2"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Missing prices are generated from EUR (seed product: 10.00 EUR monthly).
    let r = call(&app, "POST", "/api/v1/admin/currencies/sync-prices", Some(json!({"only_missing": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let price = || async {
        let p = call(&app, "GET", "/api/v1/admin/catalog/products", None, &admin.h()).await;
        let prod = p.body["items"].as_array().unwrap().iter().find(|x| x["id"] == pid.as_str()).unwrap().clone();
        prod["prices"].as_array().unwrap().iter().find(|x| x["currency"] == code.as_str() && x["cycle"] == "monthly").map(|x| x["amount"].as_str().unwrap().to_string())
    };
    let first = price().await.expect("converted price");
    assert_eq!(first.parse::<f64>().unwrap(), 49.75);
    // only_missing keeps the existing price after a rate change; a full sync rewrites it.
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/currencies/{code}"), Some(json!({"exponent": 2, "enabled": true, "rate": "5", "suffix": " Q"})), &admin.h()).await.status, StatusCode::OK);
    call(&app, "POST", "/api/v1/admin/currencies/sync-prices", Some(json!({"only_missing": true})), &admin.h()).await;
    assert_eq!(price().await.unwrap().parse::<f64>().unwrap(), 49.75);
    call(&app, "POST", "/api/v1/admin/currencies/sync-prices", Some(json!({"only_missing": false})), &admin.h()).await;
    assert_eq!(price().await.unwrap().parse::<f64>().unwrap(), 50.0);
    // Exponent is locked once prices exist.
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/currencies/{code}"), Some(json!({"exponent": 3, "enabled": true, "rate": "5", "suffix": " Q"})), &admin.h()).await.status, StatusCode::CONFLICT);

    // Public list exposes rate/suffix for enabled currencies.
    let pubc = call(&app, "GET", "/api/v1/currencies", None, &[]).await;
    let me = pubc.body["items"].as_array().unwrap().iter().find(|c| c["code"] == code.as_str()).unwrap().clone();
    assert_eq!(me["suffix"], "Q");

    // Switching default rebases rates; switch back afterwards.
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/currencies/{code}/default"), None, &admin.h()).await.status, StatusCode::OK);
    let l = call(&app, "GET", "/api/v1/admin/currencies", None, &admin.h()).await;
    let eur = l.body["items"].as_array().unwrap().iter().find(|c| c["code"] == "EUR").unwrap().clone();
    assert_eq!(eur["rate"].as_str().unwrap().parse::<f64>().unwrap(), 0.2);
    assert_eq!(l.body["items"][0]["code"], code.as_str());
    assert_eq!(call(&app, "POST", "/api/v1/admin/currencies/EUR/default", None, &admin.h()).await.status, StatusCode::OK);
    let l = call(&app, "GET", "/api/v1/admin/currencies", None, &admin.h()).await;
    let me = l.body["items"].as_array().unwrap().iter().find(|c| c["code"] == code.as_str()).unwrap().clone();
    assert_eq!(me["rate"].as_str().unwrap().parse::<f64>().unwrap(), 5.0);
    // Disable it so other tests are unaffected.
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/currencies/{code}"), Some(json!({"exponent": 2, "enabled": false, "rate": "5"})), &admin.h()).await.status, StatusCode::OK);
}
