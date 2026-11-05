//! WHMCS API gap commands: DeleteOrder, PendingOrder, UpdateTransaction and
//! the extended `/includes/api.php` action set.
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn form(app: &axum::Router, body: &str) -> Value {
    let req = Request::builder()
        .method("POST")
        .uri("/includes/api.php")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

#[tokio::test]
async fn order_delete_pending_transaction_note_and_whmcs_actions() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"city": "Cluj"})),
        &alice.h(),
    )
    .await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let order = || async {
        let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {&oid: &ch[0]}}]})), &alice.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        (
            r.body["order"]["id"].as_str().unwrap().to_string(),
            r.body["invoice"]["id"].as_str().unwrap().to_string(),
        )
    };

    // PendingOrder: only from cancelled/fraud; reopens the voided invoice.
    let (o1, i1) = order().await;
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/orders/{o1}/pending"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/orders/{o1}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{o1}/cancel"),
        Some(json!({"fraud": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/invoices/{i1}"),
            None,
            &admin.h()
        )
        .await
        .body["invoice"]["status"]
            .as_str()
            .or(Some("?"))
            .map(|_| ()),
        Some(())
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{o1}/pending"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (StatusCode::OK, Some("pending")),
        "{:?}",
        r.body
    );
    let inv: String = sqlx::query_scalar("SELECT status FROM invoices WHERE id = $1::uuid")
        .bind(&i1)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(inv, "unpaid");
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/orders/{o1}"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // UpdateTransaction: staff note only.
    let total: String = sqlx::query_scalar("SELECT total::text FROM invoices WHERE id = $1::uuid")
        .bind(&i1)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{i1}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("gap-{i1}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let pay: uuid::Uuid = sqlx::query_scalar("SELECT id FROM payments WHERE invoice_id = $1::uuid")
        .bind(&i1)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/transactions/{pay}"),
        Some(json!({"note": "  bank ref 77 "})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let note: Option<String> = sqlx::query_scalar("SELECT admin_note FROM payments WHERE id = $1")
        .bind(pay)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some("bank ref 77"));
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/admin/transactions/{pay}"),
            Some(json!({"note": "x".repeat(2001)})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/v1/admin/transactions/{}", uuid::Uuid::new_v4()),
            Some(json!({"note": "x"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // DeleteOrder through the WHMCS shim: a cancelled order without services.
    call(
        &app,
        "POST",
        "/api/v1/admin/addons/integration-whmcs-api/enable",
        None,
        &admin.h(),
    )
    .await;
    let k = call(&app, "POST", "/api/v1/api-keys", Some(json!({"name": "whmcs-gaps", "scopes": ["admin.access", "orders.manage", "invoices.manage", "customers.manage"]})), &admin.h()).await;
    assert_eq!(k.status, StatusCode::OK, "{:?}", k.body);
    let (secret, ident) = (
        k.body["secret"].as_str().unwrap().to_string(),
        k.body["key"]["prefix"].as_str().unwrap().to_string(),
    );
    let auth = format!("identifier={ident}&secret={secret}");
    let (o2, _) = order().await;
    assert_eq!(
        form(&app, &format!("action=DeleteOrder&{auth}&orderid={o2}")).await["result"],
        "error"
    );
    assert_eq!(
        form(&app, &format!("action=CancelOrder&{auth}&orderid={o2}")).await["result"],
        "success"
    );
    let v = form(&app, &format!("action=DeleteOrder&{auth}&orderid={o2}")).await;
    assert_eq!(v["result"], "success", "{v:?}");
    let gone: Option<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM orders WHERE id = $1::uuid")
        .bind(&o2)
        .fetch_optional(&st.pool)
        .await
        .unwrap();
    assert!(gone.is_none());

    let v = form(&app, &format!("action=GetOrderStatuses&{auth}")).await;
    assert_eq!(v["totalresults"], 4, "{v:?}");
    assert_eq!(
        form(
            &app,
            "action=GetOrderStatuses&identifier=pelty_x&secret=pelty_x_bad"
        )
        .await["result"],
        "error"
    );
    let v = form(&app, &format!("action=GetTransactions&{auth}&limitnum=5")).await;
    assert_eq!(v["result"], "success", "{v:?}");
    assert!(v["transactions"].is_array());
    let v = form(
        &app,
        &format!("action=UpdateTransaction&{auth}&transactionid={pay}&description=via+api"),
    )
    .await;
    assert_eq!(v["result"], "success", "{v:?}");
    let cid: String =
        sqlx::query_scalar("SELECT customer_id::text FROM invoices WHERE id = $1::uuid")
            .bind(&i1)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    let v = form(
        &app,
        &format!(
            "action=AddCredit&{auth}&clientid={cid}&amount=3.50&currency=eur&description=goodwill"
        ),
    )
    .await;
    assert_eq!(v["result"], "success", "{v:?}");
    let v = form(&app, &format!("action=GetCredits&{auth}&clientid={cid}")).await;
    assert_eq!(v["result"], "success", "{v:?}");
    let v = form(
        &app,
        &format!("action=AddClientNote&{auth}&userid={cid}&notes=VIP"),
    )
    .await;
    assert_eq!(v["result"], "success", "{v:?}");
    let v = form(&app, &format!("action=AddBillableItem&{auth}&clientid={cid}&description=Setup+work&amount=20.00&invoiceaction=nextinvoice")).await;
    assert_eq!(v["result"], "success", "{v:?}");
    // Scope is still enforced: no tickets scope.
    assert_eq!(
        form(&app, &format!("action=GetAnnouncements&{auth}")).await["result"],
        "error"
    );
}
