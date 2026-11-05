use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

/// Events are dispatched on a spawned task, so poll for the mail briefly.
async fn wait_mail(
    st: &pelty_api::AppState,
    to: &str,
    n: usize,
) -> Vec<pelty_notifications::Email> {
    for _ in 0..100 {
        let m = mails_to(st, to).await;
        if m.len() >= n {
            return m;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    mails_to(st, to).await
}

#[tokio::test]
async fn whmcs_catalog_toggle_and_event_mail() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let on = |n: &str| format!("/api/v1/admin/email-templates/{n}/enabled");
    assert_eq!(
        call(
            &app,
            "PUT",
            &on("client_signup"),
            Some(json!({"enabled": true})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    let l = call(
        &app,
        "GET",
        "/api/v1/admin/email-templates",
        None,
        &admin.h(),
    )
    .await;
    let items = l.body["items"].as_array().unwrap();
    assert!(items.len() >= 43, "{}", items.len());
    for cat in [
        "general",
        "product",
        "domain",
        "invoice",
        "support",
        "affiliate",
        "admin",
    ] {
        assert!(items.iter().any(|i| i["category"] == cat), "{cat}");
    }
    // Every template has a real default subject in both locales (not the raw key).
    for i in items {
        for loc in i["locales"].as_array().unwrap() {
            assert!(
                !loc["subject"].as_str().unwrap().starts_with("email."),
                "{}",
                i["name"]
            );
        }
    }
    let ro = items
        .iter()
        .find(|i| i["name"] == "invoice_payment_confirmation")
        .unwrap();
    assert!(ro["locales"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["locale"] == "ro" && l["subject"].as_str().unwrap().contains("Plată")));

    // Preview with merge fields; unknown fields are rejected.
    let p = call(
        &app,
        "POST",
        "/api/v1/admin/email-templates/new_service_info/en/preview",
        Some(json!({"subject": "Ready {{service}}", "body": "Hi {{name}}"})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert_eq!(p.body["text"], "Hi Ana Popescu");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/email-templates/new_service_info/en/preview",
            Some(json!({"subject": "x", "body": "{{nope}}"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Security templates cannot be switched off; staff cannot toggle at all.
    assert_eq!(
        call(
            &app,
            "PUT",
            &on("password_reset"),
            Some(json!({"enabled": false})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let staff = sess(&app, &st, "staff").await;
    assert_eq!(
        call(
            &app,
            "PUT",
            &on("client_signup"),
            Some(json!({"enabled": false})),
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // user.registered -> welcome email.
    let e = email();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": e, "password": "Good-Secret-2026!"})),
        &[],
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let m = wait_mail(&st, &e, 2).await;
    assert!(
        m.iter().any(|x| x.subject.starts_with("Welcome to")),
        "{:?}",
        m.iter().map(|x| &x.subject).collect::<Vec<_>>()
    );

    // Disabled -> not sent; test send still works.
    assert_eq!(
        call(
            &app,
            "PUT",
            &on("client_signup"),
            Some(json!({"enabled": false})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let l = call(
        &app,
        "GET",
        "/api/v1/admin/email-templates",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        l.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == "client_signup")
            .unwrap()["enabled"],
        false
    );
    let e2 = email();
    call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": e2, "password": "Good-Secret-2026!"})),
        &[],
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!mails_to(&st, &e2)
        .await
        .iter()
        .any(|x| x.subject.starts_with("Welcome to")));
    let t = call(
        &app,
        "POST",
        "/api/v1/admin/email-templates/client_signup/en/test",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(t.status, StatusCode::ACCEPTED);
    assert!(mails_to(&st, &admin.email)
        .await
        .iter()
        .any(|x| x.subject.starts_with("Welcome to")));
    call(
        &app,
        "PUT",
        &on("client_signup"),
        Some(json!({"enabled": true})),
        &admin.h(),
    )
    .await;
}
