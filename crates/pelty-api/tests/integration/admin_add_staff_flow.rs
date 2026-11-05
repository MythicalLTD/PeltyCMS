use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const PW: &str = "Very-Long-Pass-123";

#[tokio::test]
async fn admin_adds_staff_by_invite_password_or_promotion() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let up = "/api/v1/admin/staff";
    let staff_emails = |b: &serde_json::Value| -> Vec<String> {
        b["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["email"].as_str().unwrap().to_string())
            .collect()
    };

    // Invite: new account, roles applied, set-password link works.
    let e1 = email();
    assert_eq!(
        call(
            &app,
            "POST",
            up,
            Some(json!({"email": e1, "roles": ["admin"]})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": e1, "display_name": "Ana", "roles": ["admin"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invited"], true);
    let list = call(&app, "GET", up, None, &admin.h()).await;
    assert!(staff_emails(&list.body).contains(&e1));
    let mails = mails_to(&st, &e1).await;
    assert_eq!(mails.len(), 1);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": token_from(&mails[0]), "password": PW})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email": e1, "password": PW})),
            &[]
        )
        .await
        .status,
        StatusCode::OK
    );
    // Already staff: refused.
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": e1, "roles": ["admin"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.already_staff");

    // Password mode: can log in right away, no mail.
    let e2 = email();
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": e2, "roles": ["admin"], "login": "password", "password": PW})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
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
    assert!(mails_to(&st, &e2).await.is_empty());

    // Unknown role: refused and no account is left behind.
    let e3 = email();
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": e3, "roles": ["no-such-role"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.unknown_role");
    assert!(pelty_database::users::find_by_email(&st.pool, &e3)
        .await
        .unwrap()
        .is_none());
    // Only "customer" is not a staff role.
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": email(), "roles": ["customer"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.roles_required");

    // Promotion of an existing customer account keeps the customer role.
    let ce = cust.email.clone();
    let r = call(
        &app,
        "POST",
        up,
        Some(json!({"email": ce, "roles": ["admin"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invited"], false);
    let roles = pelty_database::users::roles(
        &st.pool,
        uuid::Uuid::parse_str(r.body["id"].as_str().unwrap()).unwrap(),
    )
    .await
    .unwrap();
    assert!(
        roles.contains(&"admin".to_string()) && roles.contains(&"customer".to_string()),
        "{roles:?}"
    );
}
