use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn invite_contact_sets_password_and_scopes_orders_domains() {
    let (app, st) = app().await;
    let owner = sess(&app, &st, "customer").await;
    let admin = sess(&app, &st, "admin").await;
    let ce = email();
    let r = call(
        &app,
        "POST",
        "/api/v1/client/subaccounts/invite",
        Some(json!({"email": ce, "name": "Ivy", "permissions": ["domains"]})),
        &owner.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mails = mails_to(&st, &ce).await;
    assert_eq!(mails.len(), 1, "one invite mail");
    assert!(mails[0].text.contains("/reset-password?token="));
    let tok = token_from(&mails[0]);
    // Cannot log in before accepting (random password never shared).
    let l = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": ce, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await;
    assert_ne!(l.status, StatusCode::OK);
    let rs = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": tok, "password": "Invited-Pass-456!"})),
        &[],
    )
    .await;
    assert!(rs.status.is_success(), "{:?}", rs.body);
    let l = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": ce, "password": "Invited-Pass-456!"})),
        &[],
    )
    .await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    let cookie = l
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = l.body["csrf_token"].as_str().unwrap().to_string();
    let ch = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let a = call(&app, "GET", "/api/v1/client/access", None, &ch).await;
    assert_eq!(a.body["domains"], json!(true));
    assert_eq!(a.body["orders"], json!(false));
    assert_eq!(
        call(&app, "GET", "/api/v1/client/domains", None, &ch)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/orders", None, &ch)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/invoices", None, &ch)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    // Grant orders.
    let cid = r.body["id"].as_str().unwrap();
    let up = call(
        &app,
        "PUT",
        &format!("/api/v1/client/subaccounts/{cid}"),
        Some(json!({"name": "Ivy", "permissions": ["orders", "domains"]})),
        &owner.h(),
    )
    .await;
    assert_eq!(up.status, StatusCode::OK);
    assert_eq!(
        call(&app, "GET", "/api/v1/client/orders", None, &ch)
            .await
            .status,
        StatusCode::OK
    );
    // Admin sees the sub-account on the client.
    let owner_cust: uuid::Uuid =
        sqlx::query_scalar("SELECT customer_id FROM customer_subaccounts WHERE id = $1::uuid")
            .bind(cid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    let ad = call(
        &app,
        "GET",
        &format!("/api/v1/admin/customers/{owner_cust}/subaccounts"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ad.status, StatusCode::OK);
    assert_eq!(ad.body["items"][0]["email"], json!(ce));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/customers/{owner_cust}/subaccounts"),
            None,
            &owner.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    // Unknown permission rejected.
    let bad = call(
        &app,
        "POST",
        "/api/v1/client/subaccounts/invite",
        Some(json!({"email": email(), "name": "X", "permissions": ["root"]})),
        &owner.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}
