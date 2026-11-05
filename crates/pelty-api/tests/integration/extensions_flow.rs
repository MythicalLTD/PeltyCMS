use crate::common;
use axum::http::StatusCode;
use common::*;
use hmac::{digest::KeyInit, Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

#[tokio::test]
async fn themes_switch_via_api() {
    let (app, state) = app().await;
    let r = call(&app, "GET", "/api/v1/themes", None, &[]).await;
    let ids: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"default") && ids.contains(&"daylight"));
    let (cust, ccsrf, _) = login_new(&app, &state, "customer").await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/themes/active",
        Some(json!({"id":"daylight"})),
        &[("cookie", &cust), ("x-csrf-token", &ccsrf)],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let (adm, csrf, _) = login_new(&app, &state, "admin").await;
    let h = [("cookie", adm.as_str()), ("x-csrf-token", csrf.as_str())];
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/themes/active",
        Some(json!({"id":"../etc"})),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/themes/active",
            Some(json!({"id":"daylight"})),
            &h
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let a = call(&app, "GET", "/api/v1/themes/active", None, &[]).await;
    assert_eq!(a.body["id"], "daylight");
    assert_eq!(a.body["color_scheme"], "light");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/themes/active",
            Some(json!({"id":"default"})),
            &h
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/themes/active", None, &[])
            .await
            .body["id"],
        "default"
    );
}

#[tokio::test]
async fn theme_assets_are_traversal_safe() {
    let (app, _) = app().await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/themes/default/assets/logo.svg",
            None,
            &[]
        )
        .await
        .status,
        StatusCode::OK
    );
    for p in [
        "/api/v1/themes/default/assets/../daylight/theme.json",
        "/api/v1/themes/default/assets/..%2F..%2FCargo.toml",
        "/api/v1/themes/default/assets/theme.json",
        "/api/v1/themes/x/assets/logo.svg",
    ] {
        assert_eq!(
            call(&app, "GET", p, None, &[]).await.status,
            StatusCode::NOT_FOUND,
            "{p}"
        );
    }
}

#[tokio::test]
async fn plugin_toggle_and_hook_bus() {
    let (app, state) = app().await;
    let (adm, csrf, _) = login_new(&app, &state, "admin").await;
    let h = [("cookie", adm.as_str()), ("x-csrf-token", csrf.as_str())];
    let r = call(&app, "GET", "/api/v1/admin/plugins", None, &h).await;
    let items = r.body["items"].as_array().unwrap();
    assert!(items
        .iter()
        .any(|p| p["id"] == "event-logger" && p["kind"] == "builtin"));
    assert!(items
        .iter()
        .any(|p| p["id"] == "example-webhook" && p["kind"] == "external"));
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/plugins/nope",
            Some(json!({"enabled":true})),
            &h
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/plugins/event-logger",
            Some(json!({"enabled":true})),
            &h
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let before = state.event_logger.recent().len();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": email(), "password": "Hook-Bus-Works-1"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    for _ in 0..50 {
        if state
            .event_logger
            .recent()
            .iter()
            .skip(before)
            .any(|e| e.name == "user.registered")
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("event-logger did not receive user.registered");
}

fn sign(secret: &str, t: i64, body: &str) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    m.update(format!("{t}.{body}").as_bytes());
    format!("t={t},v1={}", hex::encode(m.finalize().into_bytes()))
}

#[tokio::test]
async fn stripe_webhook_verified_and_deduplicated() {
    let (app, state) = app().await;
    std::env::set_var("PELTY_TEST_STRIPE_SK", "sk_test_123");
    std::env::set_var("PELTY_TEST_STRIPE_WHSEC", "whsec_integration");
    pelty_database::raw_execute(
        &state.pool,
        r#"INSERT INTO payment_gateways (id, enabled, secret_refs) VALUES ('stripe', TRUE,
        '{"secret_key":"PELTY_TEST_STRIPE_SK","webhook_secret":"PELTY_TEST_STRIPE_WHSEC"}')
        ON CONFLICT (id) DO UPDATE SET enabled = TRUE, secret_refs = EXCLUDED.secret_refs"#,
    )
    .await
    .unwrap();
    let cs = format!("cs_{}", uuid::Uuid::new_v4().simple());
    pelty_database::raw_execute(&state.pool, &format!("INSERT INTO payments (id, gateway, provider_ref, amount, currency, status, idempotency_key)
        VALUES ('{}', 'stripe', '{cs}', 19.99, 'EUR', 'pending', '{}')", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())).await.unwrap();
    let evt = format!("evt_{}", uuid::Uuid::new_v4().simple());
    let body =
        json!({"id": evt, "type": "checkout.session.completed", "data": {"object": {"id": cs}}})
            .to_string();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let post = |sig: String, body: String| {
        let app = app.clone();
        async move {
            let req = axum::http::Request::post("/api/v1/webhooks/stripe")
                .header("stripe-signature", sig)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap();
            let res = tower::ServiceExt::oneshot(app, req).await.unwrap();
            let st = res.status();
            let b = http_body_util::BodyExt::collect(res.into_body())
                .await
                .unwrap()
                .to_bytes();
            (st, serde_json::from_slice::<serde_json::Value>(&b).unwrap())
        }
    };
    let (st, b) = post(sign("whsec_wrong", now, &body), body.clone()).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(b["error"]["code"], "error.webhook_signature");
    let (st, _) = post(sign("whsec_integration", now - 3600, &body), body.clone()).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "stale timestamp rejected");
    let (st, b) = post(sign("whsec_integration", now, &body), body.clone()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(b["duplicate"], false);
    assert_eq!(b["payments_updated"], 1);
    let (st, b) = post(sign("whsec_integration", now, &body), body.clone()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(b["duplicate"], true);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/webhooks/unknown",
            Some(json!({})),
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let (adm, _, _) = login_new(&app, &state, "admin").await;
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/gateways",
        None,
        &[("cookie", &adm)],
    )
    .await;
    let text = r.body.to_string();
    assert!(
        !text.contains("whsec_integration") && !text.contains("sk_test_123"),
        "secrets must never be returned"
    );
    let stripe = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "stripe")
        .unwrap();
    assert_eq!(stripe["secrets"]["webhook_secret"]["configured"], true);
}
