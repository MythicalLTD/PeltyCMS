use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn password_reset_full_flow_is_single_use_and_revokes_sessions() {
    let (app, state) = app().await;
    let (cookie, _csrf, email) = login_new(&app, &state, "customer").await;

    // Unknown account: same 204, no mail.
    let ghost = common::email();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/forgot",
        Some(json!({"email": ghost})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(mails_to(&state, &ghost).await.is_empty());

    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/forgot",
        Some(json!({"email": email.to_uppercase()})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let mails: Vec<_> = mails_to(&state, &email)
        .await
        .into_iter()
        .filter(|m| m.text.contains("reset-password"))
        .collect();
    assert_eq!(mails.len(), 1, "one reset mail");
    assert!(mails[0].subject.contains("PeltyCMS") && mails[0].html.contains("<a href="));
    let tok = token_from(&mails[0]);

    // Email jobs never expose their payload and are wiped after delivery.
    let leaked: Option<i64> = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'email.send' AND status = 'succeeded' AND payload::text LIKE '%' || $1 || '%'")
        .bind(&tok).fetch_one(&state.pool).await.unwrap();
    assert_eq!(leaked, Some(0));

    // Weak password: rejected and the token survives.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": tok, "password": "short"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": tok, "password": "Brand-New-Pass-456"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    // Single use.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": tok, "password": "Another-Pass-789x"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.invalid_token");

    // Old session is gone; old password fails; new password works.
    let r = call(&app, "GET", "/api/v1/auth/me", None, &[("cookie", &cookie)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": email, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": email, "password": "Brand-New-Pass-456"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn reset_tokens_expire_and_newer_requests_invalidate_older() {
    let (app, state) = app().await;
    let (_c, _x, email) = login_new(&app, &state, "customer").await;
    for _ in 0..2 {
        call(
            &app,
            "POST",
            "/api/v1/auth/password/forgot",
            Some(json!({"email": email})),
            &[],
        )
        .await;
    }
    let mails: Vec<_> = mails_to(&state, &email)
        .await
        .into_iter()
        .filter(|m| m.text.contains("reset-password"))
        .collect();
    assert_eq!(mails.len(), 2);
    let (old, new) =
        common::stale_and_live(&state, token_from(&mails[0]), token_from(&mails[1])).await;
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": old, "password": "Brand-New-Pass-456"})),
        &[],
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "superseded token"
    );
    // Expire the newest.
    sqlx::query(
        "UPDATE user_tokens SET expires_at = now() - interval '1 second' WHERE token_hash = $1",
    )
    .bind(&pelty_auth::token::hash(&new)[..])
    .execute(&state.pool)
    .await
    .unwrap();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": new, "password": "Brand-New-Pass-456"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "expired token");
}

#[tokio::test]
async fn registration_sends_verification_and_verify_marks_user() {
    let (app, state) = app().await;
    let e = common::email();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": e, "password": "Very-Long-Pass-123", "display_name": "Ana"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": e, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await;
    assert_eq!(r.body["email_verified"], false);
    let cookie = r
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.body["csrf_token"].as_str().unwrap().to_string();

    // Resend issues a new link; the first one stops working.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/email/resend",
        None,
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    // The welcome email (client_signup) also goes out; keep verification mails only.
    let mails: Vec<_> = mails_to(&state, &e).await.into_iter().filter(|m| m.text.contains("/verify-email?token=")).collect();
    assert_eq!(mails.len(), 2);
    assert!(mails
        .iter()
        .all(|m| m.text.contains("Ana") && m.text.contains("/verify-email?token=")));
    let (old, new) =
        common::stale_and_live(&state, token_from(&mails[0]), token_from(&mails[1])).await;
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/email/verify",
        Some(json!({"token": old})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/email/verify",
        Some(json!({"token": new})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = call(&app, "GET", "/api/v1/auth/me", None, &[("cookie", &cookie)]).await;
    assert_eq!(r.body["email_verified"], true);
    // Verified: resend is a no-op.
    call(
        &app,
        "POST",
        "/api/v1/auth/email/resend",
        None,
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
    )
    .await;
    assert_eq!(mails_to(&state, &e).await.iter().filter(|m| m.text.contains("/verify-email?token=")).count(), 2);
}

#[tokio::test]
async fn admin_edits_templates_per_locale_with_validation() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/email-templates",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/email-templates",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let pr = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "password_reset")
        .unwrap()
        .clone();
    assert!(pr["vars"].as_array().unwrap().iter().any(|v| v == "link"));
    assert!(pr["locales"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["locale"] == "ro"));

    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/email-templates/password_reset/ro",
        Some(json!({"subject": "x", "body": "{{secret_key}}"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.unknown_template_var");
    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/email-templates/password_reset/ro",
        Some(json!({"subject": "a\nBcc: x", "body": "b"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.invalid_subject");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/email-templates/nope/ro",
        Some(json!({"subject": "x", "body": "y"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/email-templates/password_reset/ro/preview",
        Some(json!({"subject": "Hei {{name}}", "body": "Link: {{link}} <b>"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["subject"], "Hei Ana Popescu");
    assert!(r.body["html"].as_str().unwrap().contains("&lt;b&gt;"));

    // A Romanian user gets the customised Romanian template.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/email-templates/password_reset/ro",
        Some(json!({"subject": "Parolă nouă", "body": "Salut {{name}}: {{link}}"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    call(
        &app,
        "PUT",
        "/api/v1/auth/locale",
        Some(json!({"locale": "ro"})),
        &cust.h(),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/v1/auth/password/forgot",
        Some(json!({"email": cust.email})),
        &[],
    )
    .await;
    let m = mails_to(&state, &cust.email).await;
    let last = m.last().unwrap();
    assert_eq!(last.subject, "Parolă nouă");
    assert!(last.text.starts_with("Salut T: http"));

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/email-templates/password_reset/en/test",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(mails_to(&state, &admin.email).await.len(), 1);

    let r = call(
        &app,
        "DELETE",
        "/api/v1/admin/email-templates/password_reset/ro",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/email-templates",
        None,
        &admin.h(),
    )
    .await;
    let pr = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "password_reset")
        .unwrap()
        .clone();
    let ro = pr["locales"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["locale"] == "ro")
        .unwrap()
        .clone();
    assert_eq!(ro["customized"], false);

    // Job listing redacts email payloads.
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/jobs?per_page=100",
        None,
        &admin.h(),
    )
    .await;
    for j in r.body["items"].as_array().unwrap() {
        if j["kind"] == "email.send" {
            assert_eq!(j["payload"], json!({"redacted": true}));
        }
    }
}
