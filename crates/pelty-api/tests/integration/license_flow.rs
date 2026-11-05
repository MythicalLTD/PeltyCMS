use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn license_status_accept_and_soft_overage() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();

    let r = call(&app, "GET", "/api/v1/admin/license", None, &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["free_limit"], 50);
    assert_eq!(r.body["enforcement"], "soft");
    assert_eq!(r.body["show_accept_gate"], true);

    let r = call(&app, "POST", "/api/v1/admin/license/accept", Some(json!({ "accept": false })), &ah).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = call(&app, "POST", "/api/v1/admin/license/accept", Some(json!({ "accept": true })), &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["accepted"], true);
    assert_eq!(r.body["show_accept_gate"], false);

    let wiz = call(&app, "GET", "/api/v1/admin/setup-wizard", None, &ah).await;
    let lic_step = wiz.body["steps"].as_array().unwrap().iter().find(|s| s["id"] == "license").unwrap();
    assert_eq!(lic_step["done"], true);

    // Soft overage: many active customers, panel still answers; banner shows.
    for i in 0..51 {
        let id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO customers (id, email, status) VALUES ($1, $2, 'active')")
            .bind(id)
            .bind(format!("lic-{i}@example.test"))
            .execute(&st.pool)
            .await
            .unwrap();
    }
    let r = call(&app, "GET", "/api/v1/admin/license", None, &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["active_clients"].as_i64().unwrap() >= 51);
    assert_eq!(r.body["over_free_limit"], true);
    assert_eq!(r.body["show_overage_banner"], true);
    assert_eq!(r.body["enforcement"], "soft");

    let r = call(&app, "PUT", "/api/v1/admin/license/overage-notice", Some(json!({ "dismissed": true })), &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["show_overage_banner"], false);

    let cust = sess(&app, &st, "customer").await;
    assert_eq!(call(&app, "GET", "/api/v1/admin/license", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
}
