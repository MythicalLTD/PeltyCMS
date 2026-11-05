use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn catalog_admin_and_public() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, _oid, _ch) = seed_product(&app, &admin).await;

    // Public listing hides module_config, shows prices as exact strings.
    let r = call(&app, "GET", "/api/v1/catalog", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let p = r.body["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == pid.as_str())
        .unwrap()
        .clone();
    assert_eq!(p["module_config"], json!({}));
    let eur_m = p["prices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["currency"] == "EUR" && x["cycle"] == "monthly")
        .unwrap();
    assert_eq!(eur_m["amount"], "10.0000");
    assert_eq!(p["options"][0]["choices"].as_array().unwrap().len(), 2);

    // Too many decimals for the currency, bad cycle, unknown currency.
    let bad = |b| async {
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/catalog/products/{pid}/prices"),
            Some(b),
            &admin.h(),
        )
        .await
        .status
    };
    assert_eq!(
        bad(json!({"prices": [{"currency": "JPY", "cycle": "monthly", "amount": "1.5"}]})).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        bad(json!({"prices": [{"currency": "EUR", "cycle": "weekly", "amount": "1"}]})).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        bad(json!({"prices": [{"currency": "XYZ", "cycle": "monthly", "amount": "1"}]})).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        bad(json!({"prices": [{"currency": "EUR", "cycle": "monthly", "amount": "-1"}]})).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Hidden product disappears from public catalog.
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/catalog/products/{pid}"),
        Some(json!({"status": "hidden"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "GET", "/api/v1/catalog", None, &[]).await;
    assert!(!r.body["products"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["id"] == pid.as_str()));

    // Duplicate slug → 409; staff cannot manage catalog.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "X", "slug": "Bad Slug"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let staff = sess(&app, &st, "staff").await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/catalog/products",
            None,
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert!(
        call(&app, "GET", "/api/v1/currencies", None, &[])
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .len()
            >= 6
    );
}
