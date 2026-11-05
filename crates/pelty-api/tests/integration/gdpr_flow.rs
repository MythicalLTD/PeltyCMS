use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn gdpr_export_and_erasure() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let cust = sess(&app, &state, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/client/profile", None, &cust.h())
            .await
            .status,
        StatusCode::OK
    );
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&cust.email)
        .fetch_one(&state.pool)
        .await
        .unwrap();

    // Export: own data, secrets stripped.
    let r = call(
        &app,
        "GET",
        "/api/v1/account/privacy/export",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["account"]["email"], cust.email);
    assert!(r.body["account"].get("password_hash").is_none());
    assert!(r.body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s.get("token_hash").is_none() && s.get("csrf_token").is_none()));
    assert_eq!(r.body["customer"]["owner_user_id"], uid.to_string());
    assert_eq!(r.body["roles"], json!(["customer"]));

    // Staff without gdpr.manage cannot use admin endpoints.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/gdpr/requests", None, &staff.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/gdpr/users/{uid}/export"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    // Request requires the password; only one pending.
    let bad = call(
        &app,
        "POST",
        "/api/v1/account/privacy/erasure",
        Some(json!({"password": "nope"})),
        &cust.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/account/privacy/erasure",
        Some(json!({"password": "Very-Long-Pass-123", "reason": "leaving"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let dup = call(
        &app,
        "POST",
        "/api/v1/account/privacy/erasure",
        Some(json!({"password": "Very-Long-Pass-123"})),
        &cust.h(),
    )
    .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    assert_eq!(dup.body["error"]["code"], "error.gdpr_pending");

    // Cancel, re-request, reject, re-request.
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/account/privacy/erasure",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/account/privacy/erasure",
        Some(json!({"password": "Very-Long-Pass-123"})),
        &cust.h(),
    )
    .await;
    let rid = r.body["request"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/gdpr/requests/{rid}/reject"),
            Some(json!({"note": "talk first"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let mine = call(
        &app,
        "GET",
        "/api/v1/account/privacy/erasure",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(mine.body["request"]["status"], "rejected");
    let r = call(
        &app,
        "POST",
        "/api/v1/account/privacy/erasure",
        Some(json!({"password": "Very-Long-Pass-123"})),
        &cust.h(),
    )
    .await;
    let rid = r.body["request"]["id"].as_str().unwrap().to_string();

    // Blocked while a live service exists.
    let (pid, _, _) = seed_product(&app, &admin).await;
    let sid: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount)
         SELECT gen_random_uuid(), c.id, $2::uuid, 'active', 'monthly', c.currency, 0 FROM customers c WHERE c.owner_user_id = $1 RETURNING id",
    ).bind(uid).bind(pid).fetch_one(&state.pool).await.unwrap();
    {
        let b = call(
            &app,
            "POST",
            &format!("/api/v1/admin/gdpr/requests/{rid}/approve"),
            None,
            &admin.h(),
        )
        .await;
        assert_eq!(b.status, StatusCode::CONFLICT);
        assert_eq!(b.body["error"]["code"], "error.gdpr_active_services");
        sqlx::query("UPDATE services SET status = 'terminated' WHERE id = $1")
            .bind(sid)
            .execute(&state.pool)
            .await
            .unwrap();
    }

    let list = call(
        &app,
        "GET",
        "/api/v1/admin/gdpr/requests?status=pending",
        None,
        &admin.h(),
    )
    .await;
    assert!(list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["id"] == rid.as_str()));
    let ok = call(
        &app,
        "POST",
        &format!("/api/v1/admin/gdpr/requests/{rid}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT, "{:?}", ok.body);

    // Anonymised; session gone; cannot log in.
    let (email, active, name): (String, bool, String) =
        sqlx::query_as("SELECT email::text, is_active, display_name FROM users WHERE id = $1")
            .bind(uid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(email.ends_with("@erased.invalid"));
    assert!(!active && name.is_empty());
    let (cemail, status, first): (String, String, String) = sqlx::query_as(
        "SELECT email::text, status, first_name FROM customers WHERE owner_user_id = $1",
    )
    .bind(uid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(
        (cemail.as_str(), status.as_str(), first.as_str()),
        (email.as_str(), "closed", "")
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", None, &cust.h())
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": cust.email, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await;
    assert_ne!(login.status, StatusCode::OK);
    let done = call(
        &app,
        "GET",
        "/api/v1/admin/gdpr/requests?status=completed",
        None,
        &admin.h(),
    )
    .await;
    assert!(done.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["id"] == rid.as_str() && x["subject_ref"] == email.as_str()));
}

#[tokio::test]
async fn gdpr_staff_cannot_be_erased() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/account/privacy/erasure",
        Some(json!({"password": "Very-Long-Pass-123"})),
        &staff.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let rid = r.body["request"]["id"].as_str().unwrap().to_string();
    let b = call(
        &app,
        "POST",
        &format!("/api/v1/admin/gdpr/requests/{rid}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(b.status, StatusCode::CONFLICT);
    assert_eq!(b.body["error"]["code"], "error.gdpr_staff_account");
}
