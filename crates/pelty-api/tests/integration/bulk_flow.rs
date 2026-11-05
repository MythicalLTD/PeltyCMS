use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn order_status(st: &pelty_api::AppState, id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT status FROM orders WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&st.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn approval_rules_bulk_invoices_mass_pay_and_due_reminders() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let ah = admin.h();
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let ch_ = c.h();
    let place = || {
        call(
            &app,
            "POST",
            "/api/v1/client/orders",
            Some(
                json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]}),
            ),
            &ch_,
        )
    };

    // Default rules: manual mode; customers cannot touch them.
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/settings/order-approval",
        None,
        &ah,
    )
    .await;
    assert_eq!(r.body["auto_approve"], false);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/orders/approval-queue",
            None,
            &c.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/order-approval",
            Some(json!({"auto_approve": true, "max_total": "-1"})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/order-approval",
        Some(json!({"auto_approve": true, "require_paid": true, "max_total": "100"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Auto: held until paid, then approved by the invoice.paid hook.
    let o1 = place().await;
    assert_eq!(o1.status, StatusCode::OK, "{:?}", o1.body);
    let (o1id, inv1) = (
        o1.body["order"]["id"].as_str().unwrap().to_string(),
        o1.body["invoice"]["id"].as_str().unwrap().to_string(),
    );
    let q = call(
        &app,
        "GET",
        "/api/v1/admin/orders/approval-queue",
        None,
        &ah,
    )
    .await;
    let row = q.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == o1id.as_str())
        .cloned()
        .expect("queued");
    assert_eq!(row["reasons"], json!(["awaiting_payment"]));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/invoices/bulk",
            Some(json!({"ids": [inv1], "action": "mark_paid"})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "key required"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/invoices/bulk",
            Some(json!({"ids": [], "action": "void"})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let missing = uuid::Uuid::new_v4().to_string();
    // Keys and bank references are unique per run: stored payments outlive
    // the run, and a re-run would replay them (idempotency conflict).
    let (k1, k2, bank) = (
        format!("k1-{missing}"),
        format!("k2-{missing}"),
        format!("BANK-{missing}"),
    );
    let r = call(&app, "POST", "/api/v1/admin/invoices/bulk", Some(json!({"ids": [inv1, missing], "action": "mark_paid", "idempotency_key": k1, "reference": bank})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (r.body["ok"].clone(), r.body["failed"].clone()),
        (json!(1), json!(1)),
        "{:?}",
        r.body
    );
    let mut status = String::new();
    for _ in 0..50 {
        status = order_status(&st, &o1id).await;
        if status == "active" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status, "active", "auto-approved after payment");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/invoices/bulk",
        Some(json!({"ids": [inv1], "action": "mark_paid", "idempotency_key": k1})),
        &ah,
    )
    .await;
    assert_eq!(
        r.body["results"][0]["error"], "error.invoice_not_unpaid",
        "never double-paid"
    );

    // Over the limit: stays in the queue after payment; staff bulk-approve.
    call(
        &app,
        "PUT",
        "/api/v1/admin/settings/order-approval",
        Some(json!({"auto_approve": true, "require_paid": true, "max_total": "1"})),
        &ah,
    )
    .await;
    let o2 = place().await;
    let (o2id, inv2) = (
        o2.body["order"]["id"].as_str().unwrap().to_string(),
        o2.body["invoice"]["id"].as_str().unwrap().to_string(),
    );
    call(
        &app,
        "POST",
        "/api/v1/admin/invoices/bulk",
        Some(json!({"ids": [inv2], "action": "mark_paid", "idempotency_key": k2})),
        &ah,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(order_status(&st, &o2id).await, "pending");
    let q = call(
        &app,
        "GET",
        "/api/v1/admin/orders/approval-queue",
        None,
        &ah,
    )
    .await;
    let row = q.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == o2id.as_str())
        .cloned()
        .unwrap();
    assert_eq!(row["reasons"], json!(["over_limit"]));
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/orders/bulk",
        Some(json!({"ids": [o2id], "action": "approve"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 1, "{:?}", r.body);
    assert_eq!(
        r.body["results"][0]["result"]["services"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/orders/bulk",
        Some(json!({"ids": [o2id], "action": "approve"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["results"][0]["error"], "error.order_not_pending");

    // Manual mode again; bulk void + cancel.
    call(
        &app,
        "PUT",
        "/api/v1/admin/settings/order-approval",
        Some(json!({"auto_approve": false})),
        &ah,
    )
    .await;
    let o3 = place().await;
    let (o3id, inv3) = (
        o3.body["order"]["id"].as_str().unwrap().to_string(),
        o3.body["invoice"]["id"].as_str().unwrap().to_string(),
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/invoices/bulk",
        Some(json!({"ids": [inv3], "action": "void"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 1, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/orders/bulk",
        Some(json!({"ids": [o3id], "action": "cancel"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 1, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/orders/bulk",
            Some(json!({"ids": [o3id], "action": "nuke"})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Pre-due reminders: once per threshold.
    let o4 = place().await;
    let inv4 = o4.body["invoice"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/due-reminders",
            Some(json!({"enabled": true, "days_before": [0]})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/due-reminders",
        Some(json!({"enabled": true, "days_before": [7, 3, 3]})),
        &ah,
    )
    .await;
    assert_eq!(r.body["days_before"], json!([3, 7]));
    pelty_database::raw_execute(
        &st.pool,
        &format!("UPDATE invoices SET due_at = now() + interval '2 days' WHERE id = '{inv4}'"),
    )
    .await
    .unwrap();
    let today = time::OffsetDateTime::now_utc().date();
    let mine = |v: Vec<pelty_database::invoice_tools::DueSoon>| {
        v.into_iter()
            .filter(|d| d.invoice_id.to_string() == inv4)
            .collect::<Vec<_>>()
    };
    let first = mine(
        pelty_database::invoice_tools::run_due_reminders(&st.pool, today)
            .await
            .unwrap(),
    );
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].days_left, 2);
    assert!(
        mine(
            pelty_database::invoice_tools::run_due_reminders(&st.pool, today)
                .await
                .unwrap()
        )
        .is_empty(),
        "sent once"
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/invoices/bulk",
        Some(json!({"ids": [inv4], "action": "remind"})),
        &ah,
    )
    .await;
    assert_eq!(
        r.body["results"][0]["result"]["days_left"], 2,
        "{:?}",
        r.body
    );
    call(
        &app,
        "PUT",
        "/api/v1/admin/settings/due-reminders",
        Some(json!({"enabled": false, "days_before": [3]})),
        &ah,
    )
    .await;

    // Mass-pay from credit: own invoices only.
    let cid = call(&app, "GET", "/api/v1/client/profile", None, &c.h())
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{cid}/credit"),
        Some(json!({"currency": "EUR", "amount": "500", "note": "t"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let other = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/client/invoices/mass-pay",
        Some(json!({"ids": [inv4]})),
        &other.h(),
    )
    .await;
    assert_eq!(
        r.body["results"][0]["error"], "error.not_found",
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/client/invoices/mass-pay",
        Some(json!({"ids": [inv4]})),
        &c.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["paid"], 1, "{:?}", r.body);
    let st4: String = sqlx::query_scalar("SELECT status FROM invoices WHERE id = $1::uuid")
        .bind(&inv4)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(st4, "paid");
}
