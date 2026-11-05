use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn fixed_term_products_terminate_after_term() {
    let (app, st) = app().await;
    let pool = &st.pool;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE", "first_name": "Alice"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let uri = format!("/api/v1/admin/catalog/products/{pid}/fixed-term");

    assert_eq!(
        call(&app, "GET", &uri, None, &alice.h()).await.status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", &uri, None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["auto_terminate_days"].is_null());
    for bad in [
        json!({"auto_terminate_days": 0}),
        json!({"auto_terminate_days": 5000}),
        json!({"x": 1}),
    ] {
        assert_eq!(
            call(&app, "PUT", &uri, Some(bad), &admin.h()).await.status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let r = call(
        &app,
        "PUT",
        &uri,
        Some(json!({"auto_terminate_days": 30})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call(&app, "GET", &uri, None, &admin.h()).await.body["auto_terminate_days"],
        30
    );
    let missing = format!(
        "/api/v1/admin/catalog/products/{}/fixed-term",
        Uuid::new_v4()
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &missing,
            Some(json!({"auto_terminate_days": 3})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // An active service of that product.
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let order = r.body["order"]["id"].as_str().unwrap().to_string();
    let inv: Uuid = r.body["invoice"]["id"].as_str().unwrap().parse().unwrap();
    let total = r.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{inv}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("ft-{inv}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let a = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let (sid, st0): (Uuid, String) = sqlx::query_as("SELECT s.id, s.status FROM services s JOIN order_items oi ON oi.id = s.order_item_id JOIN orders o ON o.id = oi.order_id WHERE o.invoice_id = $1")
        .bind(inv).fetch_one(pool).await.unwrap();
    if st0 != "active" {
        let a = call(
            &app,
            "POST",
            &format!("/api/v1/admin/services/{sid}/action"),
            Some(json!({"action": "activate"})),
            &admin.h(),
        )
        .await;
        assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    }
    let status = || async {
        sqlx::query_scalar::<_, String>("SELECT status FROM services WHERE id = $1")
            .bind(sid)
            .fetch_one(pool)
            .await
            .unwrap()
    };

    // Term not elapsed: untouched.
    sqlx::query("UPDATE services SET activated_at = now() - interval '29 days' WHERE id = $1")
        .bind(sid)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(pelty_api::routes::billable::run_fixed_term(&st).await, 0);
    assert_eq!(status().await, "active");
    // Elapsed: terminated once, audited.
    sqlx::query("UPDATE services SET activated_at = now() - interval '31 days' WHERE id = $1")
        .bind(sid)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(pelty_api::routes::billable::run_fixed_term(&st).await, 1);
    assert_eq!(status().await, "terminated");
    assert_eq!(pelty_api::routes::billable::run_fixed_term(&st).await, 0);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'service.transition' AND target = $1 AND metadata->>'reason' = 'fixed_term'")
        .bind(sid.to_string()).fetch_one(pool).await.unwrap();
    assert_eq!(n, 1);

    // Clearing the term.
    let r = call(
        &app,
        "PUT",
        &uri,
        Some(json!({"auto_terminate_days": null})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(call(&app, "GET", &uri, None, &admin.h()).await.body["auto_terminate_days"].is_null());
}
