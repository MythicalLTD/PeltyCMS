use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const PW: &str = "Very-Long-Pass-123";

#[tokio::test]
async fn admin_creates_clients_with_password_invite_or_no_login() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let up = "/api/v1/admin/customers";

    let e1 = email();
    let body = json!({"email": e1, "login": "password", "password": PW, "first_name": "Ana", "last_name": "Pop", "country": "ro", "status": "inactive", "notes": "vip", "locale": "ro"});
    assert_eq!(
        call(&app, "POST", up, Some(body.clone()), &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "POST", up, Some(body.clone()), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["email"].as_str(),
            r.body["status"].as_str(),
            r.body["notes"].as_str(),
            r.body["country"].as_str()
        ),
        (Some(e1.as_str()), Some("inactive"), Some("vip"), Some("RO"))
    );
    assert!(
        r.body["owner_user_id"].is_string(),
        "a login was created: {:?}",
        r.body
    );
    // The client can sign in with the password, and lands on the same record.
    let l = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": e1, "password": PW})),
        &[],
    )
    .await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    // Duplicate email is refused, not a second record.
    assert_eq!(
        call(&app, "POST", up, Some(body), &admin.h()).await.status,
        StatusCode::CONFLICT
    );
    // Password mode without a password, unknown modes and groups are refused.
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": email(), "login": "password"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.password_required");
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": email(), "login": "magic"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_login_mode");
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": email(), "group_id": uuid::Uuid::new_v4()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_group");

    // Invite: an email with a set-password link that works.
    let e2 = email();
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": e2, "login": "invite", "first_name": "Ion"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mails = mails_to(&st, &e2).await;
    assert_eq!(mails.len(), 1, "one invite mail");
    let tok = token_from(&mails[0]);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": tok, "password": PW})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email": e2, "password": PW})),
            &[]
        )
        .await
        .status,
        StatusCode::OK
    );

    // No login: a billing record only.
    let e3 = email();
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": e3, "organization": "Acme"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["owner_user_id"].is_null());
    assert!(mails_to(&st, &e3).await.is_empty());
}
