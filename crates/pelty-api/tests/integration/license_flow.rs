use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn license_status_accept_no_client_cap() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();

    let r = call(&app, "GET", "/api/v1/admin/license", None, &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["license"], "AGPL-3.0-or-later");
    assert_eq!(r.body["version"], "2.0");
    assert!(r.body.get("free_limit").is_none());

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/license/accept",
        Some(json!({ "accept": false })),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/license/accept",
        Some(json!({ "accept": true })),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["accepted"], true);
    assert_eq!(r.body["show_accept_gate"], false);

    let wiz = call(&app, "GET", "/api/v1/admin/setup-wizard", None, &ah).await;
    let lic_step = wiz.body["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "license")
        .unwrap();
    assert_eq!(lic_step["done"], true);

    // No client cap: many active customers change nothing.
    for i in 0..51 {
        let id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO customers (id, email, status) VALUES ($1, $2, 'active')")
            .bind(id)
            .bind(format!("lic-{i}-{id}@example.test"))
            .execute(&st.pool)
            .await
            .unwrap();
    }
    let r = call(&app, "GET", "/api/v1/admin/license", None, &ah).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.get("over_free_limit").is_none());
    assert!(r.body.get("show_overage_banner").is_none());
    assert_eq!(r.body["show_accept_gate"], false);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/license/overage-notice",
        Some(json!({ "dismissed": true })),
        &ah,
    )
    .await;
    assert!(
        matches!(
            r.status,
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "{:?}",
        r.status
    );

    let cust = sess(&app, &st, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/license", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}
