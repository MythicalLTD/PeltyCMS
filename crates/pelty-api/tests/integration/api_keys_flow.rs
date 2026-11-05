use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn api_keys_scoped_hashed_revocable() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;

    let r = call(
        &app,
        "POST",
        "/api/v1/api-keys",
        Some(json!({"name": "ci", "scopes": ["audit.read"]})),
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.invalid_scopes",
        "customers can't mint scopes they lack"
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/api-keys",
        Some(json!({"name": "ci", "scopes": ["client.access"], "expires_in_days": 0})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_expiry");

    // Admin key limited to customers.manage: can list customers, not read audit.
    let r = call(&app, "POST", "/api/v1/api-keys", Some(json!({"name": "billing bot", "scopes": ["customers.manage", "admin.access"], "expires_in_days": 30})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let secret = r.body["secret"].as_str().unwrap().to_string();
    let kid = r.body["key"]["id"].as_str().unwrap().to_string();
    assert!(secret.starts_with(r.body["key"]["prefix"].as_str().unwrap()));
    // Only the hash is stored.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id = $1::uuid")
        .bind(&kid)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(stored, pelty_auth::token::hash(&secret).to_vec());
    let list = call(&app, "GET", "/api/v1/api-keys", None, &admin.h()).await;
    assert!(!list.body.to_string().contains(&secret));

    let bearer = format!("Bearer {secret}");
    let h = [("authorization", bearer.as_str())];
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/customers", None, &h)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/audit-logs", None, &h)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    // Mutations work without CSRF (no cookie involved).
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(
            json!({"email": email(), "first_name": "Key", "last_name": "Made", "currency": "EUR"}),
        ),
        &h,
    )
    .await;
    assert!(
        r.status == StatusCode::OK || r.status == StatusCode::CREATED,
        "{:?}",
        r.body
    );
    // Keys can't touch auth or key management.
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", None, &h).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/api-keys", None, &h).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/api-keys",
            Some(json!({"name": "x", "scopes": ["admin.access"]})),
            &h
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    // Wrong secret, other user can't revoke, revoke works.
    let bad = format!("Bearer {secret}x");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/customers",
            None,
            &[("authorization", bad.as_str())]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/api-keys/{kid}"),
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/api-keys/{kid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/customers", None, &h)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let used: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = $1::uuid")
            .bind(&kid)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(used.is_some());
}

#[tokio::test]
async fn admin_can_list_and_revoke_other_users_keys() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let cust = sess(&app, &state, "customer").await;
    let mut secrets = vec![];
    for n in ["a", "b"] {
        let r = call(
            &app,
            "POST",
            "/api/v1/api-keys",
            Some(json!({"name": n, "scopes": ["client.access"]})),
            &cust.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        secrets.push((
            r.body["key"]["id"].as_str().unwrap().to_string(),
            r.body["secret"].as_str().unwrap().to_string(),
        ));
    }
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/api-keys", None, &staff.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/api-keys/{}", secrets[0].0),
            None,
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    let q = format!("/api/v1/admin/api-keys?active=true&q={}", cust.email);
    let l = call(&app, "GET", &q, None, &admin.h()).await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    assert_eq!(l.body["total"], 2);
    assert_eq!(l.body["items"][0]["user_email"], cust.email.as_str());
    assert!(!l.body.to_string().contains(&secrets[0].1));
    let uid = l.body["items"][0]["user_id"].as_str().unwrap().to_string();

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/api-keys/{}", secrets[0].0),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/api-keys/{}", secrets[0].0),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let b0 = format!("Bearer {}", secrets[0].1);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/profile",
            None,
            &[("authorization", b0.as_str())]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let b1 = format!("Bearer {}", secrets[1].1);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/profile",
            None,
            &[("authorization", b1.as_str())]
        )
        .await
        .status,
        StatusCode::OK
    );
    // A key can never use the admin key endpoints, even an admin's.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/api-keys",
            None,
            &[("authorization", b1.as_str())]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{uid}/api-keys/revoke-all"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["revoked"], 1, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/profile",
            None,
            &[("authorization", b1.as_str())]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", &q, None, &admin.h()).await.body["total"],
        0
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'api_key.revoked%by_admin' AND actor_id IS NOT NULL AND (target = $1 OR target = $2)")
        .bind(&secrets[0].0).bind(&uid).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn ip_allowlist_enforced_and_editable() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let ah = admin.h();
    let r = call(
        &app,
        "POST",
        "/api/v1/api-keys",
        Some(json!({"name": "x", "scopes": ["admin.access"], "allowed_ips": ["not-an-ip"]})),
        &ah,
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_ip_allowlist");
    let r = call(&app, "POST", "/api/v1/api-keys", Some(json!({"name": "ip bound", "scopes": ["admin.access", "customers.read"], "allowed_ips": ["198.51.100.0/24", "2001:db8::1"]})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["key"]["allowed_ips"],
        json!(["198.51.100.0/24", "2001:db8::1"])
    );
    let kid = r.body["key"]["id"].as_str().unwrap().to_string();
    let bearer = format!("Bearer {}", r.body["secret"].as_str().unwrap());
    let h = [("authorization", bearer.as_str())];
    assert_eq!(
        call_from(
            &app,
            "198.51.100.42",
            "GET",
            "/api/v1/admin/customers",
            None,
            &h
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call_from(
            &app,
            "2001:db8::1",
            "GET",
            "/api/v1/admin/customers",
            None,
            &h
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = call_from(
        &app,
        "203.0.113.9",
        "GET",
        "/api/v1/admin/customers",
        None,
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["error"]["code"], "error.api_key_ip_denied");
    // No known client IP: denied too.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/customers", None, &h)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let denied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'api_key.ip_denied' AND target = $1",
    )
    .bind(&kid)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(denied >= 2);

    // Owner widens the list; others can't edit; admin can clear it.
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/api-keys/{kid}"),
        Some(json!({"allowed_ips": ["203.0.113.0/24"]})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call_from(
            &app,
            "203.0.113.9",
            "GET",
            "/api/v1/admin/customers",
            None,
            &h
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/api-keys/{kid}"),
            Some(json!({"allowed_ips": []})),
            &staff.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/admin/api-keys/{kid}"),
            Some(json!({"allowed_ips": []})),
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/api-keys/{kid}"),
        Some(json!({"allowed_ips": [], "expires_in_days": 7})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["key"]["expires_at"].is_string());
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/customers", None, &h)
            .await
            .status,
        StatusCode::OK
    );
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/api-keys/{kid}"),
        Some(json!({"expires_in_days": 0})),
        &ah,
    )
    .await;
    assert!(r.body["key"]["expires_at"].is_null());
    // Keys can't edit keys.
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/api-keys/{kid}"),
            Some(json!({"allowed_ips": []})),
            &h
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
}
