use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn network_issues_public_and_admin() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let cust = sess(&app, &st, "customer").await;
    let body = json!({"title": "EU-1 packet loss", "component": "EU-1", "kind": "degraded", "priority": "high", "status": "investigating"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/network-issues",
            Some(body.clone()),
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/network-issues",
            Some(body.clone()),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = json!({"title": "x", "kind": "boom", "priority": "high", "status": "investigating"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/network-issues",
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/network-issues",
        Some(body),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();

    // Public (no session) sees it and the page is not operational.
    let p = call(&app, "GET", "/api/v1/public/network-status", None, &[]).await;
    assert_eq!(p.status, StatusCode::OK);
    assert_eq!(p.body["operational"], false);
    let it = p.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(it["kind"], "degraded");

    // Timeline update resolving it sets ends_at.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/network-issues/{id}/updates"),
        Some(json!({"status": "resolved", "message": "Fixed upstream"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/network-issues/{id}/updates"),
            Some(json!({"status": "resolved", "message": " "})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let p = call(&app, "GET", "/api/v1/public/network-status", None, &[]).await;
    let it = p.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(it["status"], "resolved");
    assert!(!it["ends_at"].is_null());
    assert_eq!(it["updates"][0]["message"], "Fixed upstream");

    // Edit, then delete; unknown id → 404.
    let r = call(&app, "PUT", &format!("/api/v1/admin/network-issues/{id}"), Some(json!({"title": "EU-1 loss", "kind": "outage", "priority": "low", "status": "monitoring"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let other = uuid::Uuid::new_v4();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/network-issues/{other}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/network-issues/{id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let a = call(
        &app,
        "GET",
        "/api/v1/admin/network-issues",
        None,
        &admin.h(),
    )
    .await;
    assert!(!a.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == id.as_str()));
}
