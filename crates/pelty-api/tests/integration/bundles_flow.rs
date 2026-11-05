use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn bundles_and_recommendations() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    call(&app, "GET", "/api/v1/client/services", None, &cust.h()).await;
    let (a, ao, ac) = seed_product(&app, &admin).await;
    let (b, bo, bc) = seed_product(&app, &admin).await;
    let (c, _, _) = seed_product(&app, &admin).await;

    // Validation + permissions.
    let body = json!({"name": "Starter pack", "discount_percent": "10", "items": [{"product_id": a, "cycle": "monthly"}, {"product_id": b, "cycle": "monthly"}]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/bundles",
            Some(body.clone()),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = json!({"name": "x", "discount_percent": "10", "items": [{"product_id": a, "cycle": "monthly"}]});
    assert_eq!(
        call(&app, "POST", "/api/v1/admin/bundles", Some(bad), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/bundles",
        Some(body),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let bid = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "GET", "/api/v1/catalog/bundles", None, &cust.h()).await;
    let pub_b = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == bid.as_str())
        .unwrap()
        .clone();
    assert_eq!(pub_b["items"].as_array().unwrap().len(), 2);
    assert_eq!(pub_b["discount_percent"], "10.00");

    // Incomplete cart is rejected.
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "bundle_id": bid, "items": [{"product_id": a, "cycle": "monthly", "config": {ao.clone(): ac[0]}}]})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.bundle_incomplete");
    // Full bundle: 10% off both recurring prices (10.00 each), setup untouched.
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "bundle_id": bid, "items": [{"product_id": b, "cycle": "monthly", "config": {bo.clone(): bc[0]}}, {"product_id": a, "cycle": "monthly", "config": {ao.clone(): ac[0]}}]})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let line: String = sqlx::query_scalar("SELECT unit_price::text FROM invoice_items WHERE invoice_id = $1::uuid AND description LIKE 'Bundle discount:%'")
        .bind(&inv).fetch_one(&state.pool).await.unwrap();
    assert_eq!(line, "-2.0000");
    let oid: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT bundle_id FROM orders WHERE invoice_id = $1::uuid")
            .bind(&inv)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(oid.map(|x| x.to_string()), Some(bid.clone()));

    // Inactive bundle can't be used.
    let upd = json!({"name": "Starter pack", "active": false, "discount_percent": "10", "items": [{"product_id": a, "cycle": "monthly"}, {"product_id": b, "cycle": "monthly"}]});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/bundles/{bid}"),
            Some(upd),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "bundle_id": bid, "items": [{"product_id": b, "cycle": "monthly", "config": {bo.clone(): bc[0]}}, {"product_id": a, "cycle": "monthly", "config": {ao.clone(): ac[0]}}]})), &cust.h()).await;
    assert_eq!(r.body["error"]["code"], "error.bundle_unavailable");

    // Recommendations.
    let path = format!("/api/v1/admin/catalog/products/{a}/recommendations");
    assert_eq!(
        call(&app, "PUT", &path, Some(json!({"upsell": [a]})), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        &path,
        Some(json!({"upsell": [b], "cross_sell": [c, b]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/catalog/products/{a}/recommendations"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["upsell"], json!([b]));
    assert_eq!(r.body["cross_sell"], json!([c, b]));
    // Hidden products drop out of the public list.
    sqlx::query("UPDATE products SET status = 'hidden' WHERE id = $1::uuid")
        .bind(&c)
        .execute(&state.pool)
        .await
        .unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/catalog/products/{a}/recommendations"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["cross_sell"], json!([b]));

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/bundles/{bid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
}
