//! Admin account security: login-alert email, step-up re-auth endpoint,
//! authenticated password change, admin IP allow-list for addon admin routes.
//! Does not change the global security policy (other flows share the DB).
use crate::common;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use common::*;
use serde_json::json;
use std::net::SocketAddr;
use tower::ServiceExt;

async fn login_from(app: &axum::Router, email: &str, pw: &str, ip: [u8; 4]) -> StatusCode {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email": email, "password": pw}).to_string(),
        ))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((ip, 40000))));
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn admin_account_security() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let pw = "Very-Long-Pass-123";
    let alerts = |e: String| {
        let pool = st.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'admin_login_alert' AND payload->>'to' = $1")
                .bind(e)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };

    // login alerts (policy default: on)
    assert_eq!(
        login_from(&app, &admin.email, pw, [198, 51, 100, 7]).await,
        StatusCode::OK
    );
    assert_eq!(
        alerts(admin.email.clone()).await,
        1,
        "new IP queues one alert email"
    );
    assert_eq!(
        login_from(&app, &admin.email, pw, [198, 51, 100, 7]).await,
        StatusCode::OK
    );
    assert_eq!(
        alerts(admin.email.clone()).await,
        1,
        "known IP: no second alert"
    );
    assert_eq!(
        login_from(&app, &admin.email, pw, [198, 51, 100, 8]).await,
        StatusCode::OK
    );
    assert_eq!(alerts(admin.email.clone()).await, 2);
    // Customers never get staff login alerts.
    let cust = sess(&app, &st, "customer").await;
    assert_eq!(
        login_from(&app, &cust.email, pw, [198, 51, 100, 9]).await,
        StatusCode::OK
    );
    assert_eq!(alerts(cust.email.clone()).await, 0);

    // step-up re-auth
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/reauth",
        Some(json!({"password": "wrong-password-1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.invalid_password",
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/reauth",
        Some(json!({"password": pw})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email = $1 AND s.reauth_at IS NOT NULL")
        .bind(&admin.email)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // No CSRF header: refused like every mutating call.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/reauth",
        Some(json!({"password": pw})),
        &[("cookie", admin.cookie.as_str())],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // password change
    let other = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/change",
        Some(json!({"current_password": pw, "new_password": pw})),
        &other.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.password_reused");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/change",
        Some(json!({"current_password": "nope-nope-123", "new_password": "Another-Long-Pass-456"})),
        &other.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_password");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/change",
        Some(json!({"current_password": pw, "new_password": "Another-Long-Pass-456"})),
        &other.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(
        login_from(&app, &other.email, pw, [198, 51, 100, 10]).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_from(
            &app,
            &other.email,
            "Another-Long-Pass-456",
            [198, 51, 100, 10]
        )
        .await,
        StatusCode::OK
    );

    // admin allow-list also guards addon admin routes
    let rule = call(
        &app,
        "POST",
        "/api/v1/admin/security/ip-rules",
        Some(json!({"cidr": "10.99.0.0/16", "kind": "admin_allow"})),
        &admin.h(),
    )
    .await;
    // Adding an allow rule that excludes the caller may be refused (lock-out guard);
    // the helper is checked against the rule set either way.
    if rule.status == StatusCode::OK {
        tokio::time::sleep(std::time::Duration::from_secs(11)).await; // rules cache
        assert!(
            pelty_api::security::admin_ip_allowed(&st, Some("10.99.1.2".parse().unwrap()))
                .await
                .is_ok()
        );
        assert_eq!(
            pelty_api::security::admin_ip_allowed(&st, Some("192.0.2.1".parse().unwrap())).await,
            Err("error.ip_not_allowed")
        );
        assert!(pelty_api::security::admin_ip_allowed(&st, None)
            .await
            .is_ok());
        let id = rule.body["id"].as_str().unwrap();
        assert_eq!(
            call(
                &app,
                "DELETE",
                &format!("/api/v1/admin/security/ip-rules/{id}"),
                None,
                &admin.h()
            )
            .await
            .status,
            StatusCode::NO_CONTENT
        );
    } else {
        panic!("could not add allow rule: {:?}", rule.body);
    }
}
