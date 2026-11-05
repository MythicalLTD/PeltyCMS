use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn update_checker() {
    let (app, state) = app().await;
    let mock = pelty_payments::MockTransport::new();
    state.set_http(std::sync::Arc::new(mock.clone()));
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/system/updates", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/system/updates",
        Some(json!({"enabled": true, "url": "http://evil.test/feed"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.update_url_invalid");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/system/updates",
        Some(json!({"enabled": true, "url": "https://feed.test/latest"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["current"], env!("CARGO_PKG_VERSION"));

    // Newer release → banner data, audited once.
    mock.push(200, r#"{"tag_name":"v99.0.0","html_url":"https://github.com/x/releases/v99","body":"Notes","published_at":"2026-10-30T00:00:00Z"}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/system/updates/check",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["latest"], "99.0.0");
    assert_eq!(r.body["update_available"], true);
    assert_eq!(r.body["release_url"], "https://github.com/x/releases/v99");
    assert!(r.body["error"].is_null());
    let req = mock.requests();
    assert_eq!(req.last().unwrap().url, "https://feed.test/latest");
    mock.push(
        200,
        r#"{"tag_name":"v99.0.0","html_url":"javascript:alert(1)"}"#,
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/system/updates/check",
        None,
        &admin.h(),
    )
    .await;
    assert!(r.body["release_url"].is_null(), "non-https link dropped");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'system.update_available' AND metadata->>'latest' = '99.0.0' AND created_at > now() - interval '1 minute'").fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 1);

    // Feed down / garbage: error stored, last good version kept.
    mock.push(503, "down");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/system/updates/check",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"], "error.update_feed_unavailable");
    assert_eq!(r.body["latest"], "99.0.0");
    mock.push(200, r#"{"tag_name":"nightly"}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/system/updates/check",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"], "error.update_feed_invalid");

    // Same or older: no banner. Worker job runs the same check.
    mock.push(
        200,
        format!(r#"{{"tag_name":"v{}"}}"#, env!("CARGO_PKG_VERSION")),
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/system/updates/check",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["update_available"], false);
    mock.push(200, r#"{"tag_name":"0.0.1"}"#);
    let v = pelty_api::updates::check(&state).await.unwrap();
    assert_eq!(v["update_available"], false);

    // Disabled: nothing scheduled. Restore defaults.
    call(
        &app,
        "PUT",
        "/api/v1/admin/system/updates",
        Some(json!({"enabled": false, "url": ""})),
        &admin.h(),
    )
    .await;
    assert!(pelty_api::updates::schedule(&state)
        .await
        .unwrap()
        .is_none());
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/system/updates",
        Some(json!({"enabled": true, "url": ""})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["url"], pelty_api::updates::DEFAULT_URL);
}
