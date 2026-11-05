use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn active_domain(
    app: &axum::Router,
    state: &pelty_api::AppState,
    admin: &Sess,
    cust: &Sess,
    tld: &str,
    email: &str,
) -> String {
    let name = format!(
        "rv-{}.{tld}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    let r = call(app, "POST", "/api/v1/client/domains", Some(json!({"name": name, "kind": "register", "contact": {"name": "Ana Pop", "email": email, "country": "RO"}})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let did = r.body["domain"]["id"].as_str().unwrap().to_string();
    let iid = r.body["invoice_id"].as_str().unwrap().to_string();
    let inv = call(
        app,
        "GET",
        &format!("/api/v1/admin/invoices/{iid}"),
        None,
        &admin.h(),
    )
    .await;
    let total: Value = if inv.body["invoice"]["total"].is_null() {
        inv.body["total"].clone()
    } else {
        inv.body["invoice"]["total"].clone()
    };
    let r = call(app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"), Some(json!({"amount": total, "idempotency_key": format!("k-{iid}"), "reference": format!("rv-{iid}")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    pelty_api::domains::process(state).await.unwrap();
    did
}

#[tokio::test]
async fn registrant_verification_end_to_end() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;
    let tld = format!("v{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let r = call(&app, "PUT", "/api/v1/admin/tlds", Some(json!({"tld": tld, "currency": "EUR", "register_price": "5.00", "renew_price": "5.00", "transfer_price": "5.00", "min_years": 1, "max_years": 5, "registrar": "mock", "active": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/settings/registrant-verification",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/settings/registrant-verification",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["enabled"], true, "{:?}", r.body);

    let email = format!("reg-{}@example.test", uuid::Uuid::new_v4().simple());
    let did = active_domain(&app, &state, &admin, &cust, &tld, &email).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "active");
    let rv = &r.body["registrant_verification"];
    assert_eq!(rv["status"], "pending", "{:?}", r.body);
    assert!(rv.get("token_hash").is_none(), "hash must not leak: {rv:?}");
    let today = time::OffsetDateTime::now_utc().date();
    assert_eq!(
        rv["deadline"],
        (today + time::Duration::days(15)).to_string()
    );
    let mails = mails_to(&state, &email).await;
    assert_eq!(mails.len(), 1);
    assert!(mails[0].text.contains("/verify-registrant?token="));
    let tok = token_from(&mails[0]);

    // Resend is throttled right after the first mail; other customers get 404.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/domains/{did}/registrant-verification/resend"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.registrant_resend_too_soon",
        "{:?}",
        r.body
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/domains/{did}/registrant-verification/resend"),
            None,
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Bad token, then the real one (single use).
    let r = call(
        &app,
        "POST",
        "/api/v1/public/domains/verify-registrant",
        Some(json!({"token": "x".repeat(43)})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/public/domains/verify-registrant",
        Some(json!({"token": tok})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["verified"], 1);
    let r = call(
        &app,
        "POST",
        "/api/v1/public/domains/verify-registrant",
        Some(json!({"token": tok})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["registrant_verification"]["status"], "verified");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/domains/{did}/registrant-verification/resend"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.registrant_nothing_pending");

    // A second domain with the same (verified) address needs no new mail.
    let d2 = active_domain(&app, &state, &admin, &cust, &tld, &email.to_uppercase()).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{d2}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["registrant_verification"]["status"], "verified",
        "{:?}",
        r.body
    );
    assert_eq!(mails_to(&state, &email).await.len(), 1);

    // Changing to a new address restarts verification; same address does not.
    let new_email = format!("new-{}@example.test", uuid::Uuid::new_v4().simple());
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/domains/{did}/contact"),
        Some(json!({"contact": {"name": "Ana Pop", "email": new_email}, "privacy": false})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["registrant_verification"]["status"], "pending",
        "{:?}",
        r.body
    );
    assert_eq!(
        r.body["registrant_verification"]["email"]
            .as_str()
            .unwrap()
            .to_lowercase(),
        new_email
    );
    assert_eq!(mails_to(&state, &new_email).await.len(), 1);
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/domains/{did}/contact"),
        Some(json!({"contact": {"name": "Ana P.", "email": new_email}, "privacy": false})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(mails_to(&state, &new_email).await.len(), 1);

    // Daily: reminder at 10 days left, again at 5, overdue after the deadline. Never twice.
    let dl = today + time::Duration::days(15);
    let (rem, _) = pelty_api::registrant_verify::daily(&state, dl - time::Duration::days(11))
        .await
        .unwrap();
    let _ = rem;
    assert_eq!(
        mails_to(&state, &new_email).await.len(),
        1,
        "no reminder with 11 days left"
    );
    pelty_api::registrant_verify::daily(&state, dl - time::Duration::days(10))
        .await
        .unwrap();
    assert_eq!(mails_to(&state, &new_email).await.len(), 2);
    pelty_api::registrant_verify::daily(&state, dl - time::Duration::days(9))
        .await
        .unwrap();
    assert_eq!(mails_to(&state, &new_email).await.len(), 2);
    pelty_api::registrant_verify::daily(&state, dl - time::Duration::days(5))
        .await
        .unwrap();
    assert_eq!(mails_to(&state, &new_email).await.len(), 3);
    pelty_api::registrant_verify::daily(&state, dl + time::Duration::days(1))
        .await
        .unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains/{did}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["registrant_verification"]["status"], "overdue",
        "{:?}",
        r.body
    );
    // Old reminder tokens are rotated: only the latest works.
    let all = mails_to(&state, &new_email).await;
    let old = token_from(&all[0]);
    let r = call(
        &app,
        "POST",
        "/api/v1/public/domains/verify-registrant",
        Some(json!({"token": old})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Staff actions.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/domains/{did}/registrant-verification"),
            Some(json!({"action": "verify"})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/domains/{did}/registrant-verification"),
        Some(json!({"action": "nope"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/domains/{did}/registrant-verification"),
        Some(json!({"action": "verify"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["registrant_verification"]["status"], "verified",
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/domains/{did}/registrant-verification"),
        Some(json!({"action": "restart"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["registrant_verification"]["status"], "pending",
        "{:?}",
        r.body
    );
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE target = $1 AND action LIKE 'domain.registrant%'",
    )
    .bind(&did)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(n >= 5, "audit rows {n}");

    // Disabled: new domains get no request.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/registrant-verification",
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["enabled"], false);
    let e3 = format!("off-{}@example.test", uuid::Uuid::new_v4().simple());
    let d3 = active_domain(&app, &state, &admin, &cust, &tld, &e3).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{d3}"),
        None,
        &cust.h(),
    )
    .await;
    assert!(r.body["registrant_verification"].is_null(), "{:?}", r.body);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/registrant-verification",
        Some(json!({"enabled": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["enabled"], true);
}
