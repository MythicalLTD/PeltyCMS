//! ToS versions, acceptance (consent record) and its presence in the GDPR export.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn tos_versions_and_acceptance() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let cust = sess(&app, &state, "customer").await;
    let v1 = format!("v1-{}", uuid::Uuid::new_v4().simple());
    let v2 = format!("v2-{}", uuid::Uuid::new_v4().simple());

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/security/tos",
            Some(json!({"version": v1})),
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/tos",
        Some(json!({"version": " "})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_version");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/tos",
        Some(json!({"version": v1, "url": "http://x"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_url");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/tos",
        Some(json!({"version": v1, "url": "/terms", "summary": "first"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id1 = r.body["version"]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/tos",
        Some(json!({"version": v1})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    let r = call(&app, "GET", "/api/v1/account/tos", None, &cust.h()).await;
    assert_eq!(
        (
            r.body["current"]["id"].as_str(),
            r.body["pending"].as_bool()
        ),
        (Some(id1.as_str()), Some(true))
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/account/tos/accept",
        Some(json!({"version_id": id1})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // Idempotent.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/account/tos/accept",
            Some(json!({"version_id": id1})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = call(&app, "GET", "/api/v1/account/tos", None, &cust.h()).await;
    assert_eq!(r.body["pending"], false);
    assert!(r.body["accepted_at"].is_string());

    // A new version makes acceptance pending again; the old one can no longer be accepted.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/tos",
        Some(json!({"version": v2})),
        &admin.h(),
    )
    .await;
    let id2 = r.body["version"]["id"].as_str().unwrap().to_string();
    let r = call(&app, "GET", "/api/v1/account/tos", None, &cust.h()).await;
    assert_eq!(r.body["pending"], true);
    assert_eq!(r.body["history"].as_array().unwrap().len(), 1);
    let r = call(
        &app,
        "POST",
        "/api/v1/account/tos/accept",
        Some(json!({"version_id": id1})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.tos_outdated");
    call(
        &app,
        "POST",
        "/api/v1/account/tos/accept",
        Some(json!({"version_id": id2})),
        &cust.h(),
    )
    .await;

    let r = call(&app, "GET", "/api/v1/admin/security/tos", None, &admin.h()).await;
    let items = r.body["items"].as_array().unwrap();
    let n = |v: &str| {
        items
            .iter()
            .find(|i| i["version"]["version"] == v)
            .map(|i| i["acceptances"].as_i64().unwrap())
    };
    assert_eq!((n(&v1), n(&v2)), (Some(1), Some(1)));

    // GDPR export carries the consent record.
    let r = call(
        &app,
        "GET",
        "/api/v1/account/privacy/export",
        None,
        &cust.h(),
    )
    .await;
    let acc = r.body["tos_acceptances"].as_array().unwrap();
    assert_eq!(acc.len(), 2);
    assert!(r.body["linked_identities"].is_array());

    // Marketing consent (upstream) still toggles.
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/client/marketing",
            Some(json!({"opt_out": true})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/marketing", None, &cust.h())
            .await
            .body["opt_out"],
        true
    );
}
