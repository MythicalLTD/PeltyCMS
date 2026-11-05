use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

// Own binary: reads global configuration and toggles the global flag.
#[tokio::test]
async fn setup_wizard_tracks_configuration_and_dismissal() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();
    let reset = json!({"dismissed": false});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/setup-wizard/dismissed",
            Some(reset.clone()),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = call(&app, "GET", "/api/v1/admin/setup-wizard", None, &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    let steps = r.body["steps"].as_array().unwrap();
    let ids: Vec<&str> = steps.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "license", "company", "currency", "email", "gateway", "product", "tax", "domains",
            "server"
        ]
    );
    let step = |b: &serde_json::Value, id: &str| {
        b["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()["done"]
            .as_bool()
            .unwrap()
    };
    let complete = r.body["complete"].as_bool().unwrap();
    assert_eq!(r.body["show"], !complete);

    // Adding a product flips the product step.
    let had_product = step(&r.body, "product");
    let g = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO product_groups (id, name, slug) VALUES ($1,'G',$2)")
        .bind(g)
        .bind(format!("sw-{}", &g.simple().to_string()[..10]))
        .execute(&st.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO products (id, group_id, name, slug) VALUES ($1,$2,'P',$3)")
        .bind(uuid::Uuid::new_v4())
        .bind(g)
        .bind(format!("swp-{}", &g.simple().to_string()[..10]))
        .execute(&st.pool)
        .await
        .unwrap();
    let r = call(&app, "GET", "/api/v1/admin/setup-wizard", None, &ah).await;
    assert!(step(&r.body, "product"));
    let _ = had_product;

    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/setup-wizard/dismissed",
        Some(json!({"dismissed": true})),
        &ah,
    )
    .await;
    assert_eq!(
        (r.body["dismissed"].as_bool(), r.body["show"].as_bool()),
        (Some(true), Some(false))
    );
    let cust = sess(&app, &st, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/setup-wizard", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/setup-wizard/dismissed",
            Some(reset.clone()),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
}
