use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn client_sso_one_time_token() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;
    call(&app, "GET", "/api/v1/client/services", None, &cust.h()).await;
    call(&app, "GET", "/api/v1/client/services", None, &other.h()).await;
    let (pid, _, _) = seed_product(&app, &admin).await;
    let sid: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date, remote_id)
         SELECT gen_random_uuid(), c.id, $2::uuid, 'active', 'monthly', c.currency, 0, CURRENT_DATE + 30, 'r42' FROM customers c JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1 RETURNING id",
    ).bind(&cust.email).bind(uuid::Uuid::parse_str(&pid).unwrap()).fetch_one(&state.pool).await.unwrap();
    let path = format!("/api/v1/client/services/{sid}/sso");

    // No module: unsupported.
    let r = call(&app, "POST", &path, None, &cust.h()).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(
        r.body["error"]["code"], "error.sso_unsupported",
        "{:?}",
        r.body
    );

    let srv = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": "SSO", "module": "webhook", "base_url": "https://panel.test", "api_key": "shared-secret",
        "config": {"sso_url": "{base_url}/sso/{remote_id}?t={token}"}})), &admin.h()).await;
    assert_eq!(srv.status, StatusCode::OK, "{:?}", srv.body);
    let srv = srv.body["id"].as_str().unwrap().to_string();
    let pm = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/products/{pid}/module"),
        Some(json!({"module": "webhook", "server_id": srv, "config": {}})),
        &admin.h(),
    )
    .await;
    assert_eq!(pm.status, StatusCode::OK, "{:?}", pm.body);

    assert_eq!(
        call(&app, "POST", &path, None, &other.h()).await.status,
        StatusCode::NOT_FOUND
    );
    let r = call(&app, "POST", &path, None, &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let url = r.body["url"].as_str().unwrap();
    assert!(url.starts_with("https://panel.test/sso/r42?t="), "{url}");
    let token = url.split("?t=").nth(1).unwrap().to_string();
    // Signed with the server key: the panel can verify offline.
    let c = pelty_provisioning::sso::verify(
        "shared-secret",
        &token,
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )
    .unwrap();
    assert_eq!(c.email, cust.email);

    // Tampered token rejected; genuine one works exactly once.
    let mut forged = token.clone();
    forged.pop();
    forged.push(if token.ends_with('0') { '1' } else { '0' });
    let none: [(&str, &str); 0] = [];
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/sso/consume",
            Some(json!({"token": forged})),
            &none
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let ok = call(
        &app,
        "POST",
        "/api/v1/sso/consume",
        Some(json!({"token": token})),
        &none,
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["remote_id"], "r42");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/sso/consume",
            Some(json!({"token": token})),
            &none
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    // Suspended services can't SSO.
    sqlx::query("UPDATE services SET status = 'suspended' WHERE id = $1")
        .bind(sid)
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "POST", &path, None, &cust.h()).await.status,
        StatusCode::CONFLICT
    );
}
