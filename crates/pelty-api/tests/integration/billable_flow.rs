use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn items_of(st: &pelty_api::AppState, inv: &str) -> Vec<pelty_database::invoices::Item> {
    pelty_database::invoices::items(&st.pool, inv.parse().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn billable_items_and_invoice_lead_time() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let cid = call(&app, "GET", "/api/v1/client/profile", None, &c.h())
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let today = time::OffsetDateTime::now_utc().date();
    let ah = admin.h();
    let post =
        |b: serde_json::Value| call(&app, "POST", "/api/v1/admin/billable-items", Some(b), &ah);

    assert_eq!(call(&app, "POST", "/api/v1/admin/billable-items", Some(json!({"customer_id": cid, "description": "x", "unit_price": "1", "invoice_action": "hold"})), &c.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(post(json!({"customer_id": cid, "description": "x", "unit_price": "1", "invoice_action": "later"})).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post(json!({"customer_id": uuid::Uuid::new_v4(), "description": "x", "unit_price": "1", "invoice_action": "hold"})).await.status, StatusCode::NOT_FOUND);

    // invoice_now issues an invoice straight away.
    let r = post(json!({"customer_id": cid, "description": "Migration", "quantity": "2", "unit_price": "5.00", "invoice_action": "invoice_now"})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = r.body["invoice_id"].as_str().unwrap().to_string();
    let it = items_of(&st, &inv).await;
    assert_eq!(it.len(), 1);
    assert_eq!(it[0].amount, rust_decimal::Decimal::from(10));
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!(
                "/api/v1/admin/billable-items/{}",
                r.body["id"].as_str().unwrap()
            ),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "invoiced one-off items are kept"
    );

    // hold + next_invoice items wait.
    let hold = post(json!({"customer_id": cid, "description": "Consulting", "unit_price": "40", "invoice_action": "hold"})).await;
    assert!(hold.body["invoice_id"].is_null());
    post(json!({"customer_id": cid, "description": "Extra IP", "unit_price": "3", "invoice_action": "next_invoice"})).await;

    // A service due in 20 days is not invoiced with the default 7-day lead time ...
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &c.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["order"]["id"].as_str().unwrap();
    let sid = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await
    .body["services"][0]
        .as_str()
        .unwrap()
        .to_string();
    pelty_database::raw_execute(
        &st.pool,
        &format!(
            "UPDATE services SET next_due_date = '{}' WHERE id = '{sid}'",
            today + time::Duration::days(20)
        ),
    )
    .await
    .unwrap();
    let n = pelty_database::services::bill_due_ahead(&st.pool, today, 7, 1000, 7)
        .await
        .unwrap();
    assert!(!n.iter().any(|r| r.service_id.to_string() == sid));

    // ... but is with a 30-day per-product setting, and carries the next_invoice item.
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/catalog/products/{pid}/billing"),
            Some(json!({"invoice_days_before": 400})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/catalog/products/{pid}/billing"),
            Some(json!({"invoice_days_before": 30})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/catalog/products/{pid}/billing"),
            None,
            &admin.h()
        )
        .await
        .body["invoice_days_before"],
        30
    );
    let n = pelty_database::services::bill_due_ahead(&st.pool, today, 7, 1000, 7)
        .await
        .unwrap();
    let ren = n
        .iter()
        .find(|r| r.service_id.to_string() == sid)
        .expect("renewal invoiced 30 days ahead");
    let lines = items_of(&st, &ren.invoice_id.to_string()).await;
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|l| l.description == "Extra IP"));

    // Held item is invoiced on demand; nothing left afterwards.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{cid}/billable-items/invoice"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let held_inv = r.body["invoice_id"].as_str().unwrap().to_string();
    assert_eq!(items_of(&st, &held_inv).await[0].description, "Consulting");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{cid}/billable-items/invoice"),
        None,
        &admin.h(),
    )
    .await;
    assert!(r.body["invoice_id"].is_null());

    // Recurring: billed once per period, limited to 2 runs.
    assert_eq!(post(json!({"customer_id": cid, "description": "Backup", "unit_price": "2", "invoice_action": "recurring", "recur_cycle": "monthly"})).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let rec = post(json!({"customer_id": cid, "description": "Backup", "unit_price": "2", "invoice_action": "recurring", "recur_cycle": "monthly", "next_due_date": today.to_string(), "recur_times": 2})).await;
    assert_eq!(rec.status, StatusCode::OK, "{:?}", rec.body);
    let rid = rec.body["id"].as_str().unwrap().to_string();
    let horizon = today + time::Duration::days(7);
    let mine =
        |v: Vec<(uuid::Uuid, uuid::Uuid)>| v.iter().filter(|(_, c)| c.to_string() == cid).count();
    assert_eq!(
        mine(
            pelty_database::billable::bill_recurring(&st.pool, horizon, 7, 1000)
                .await
                .unwrap()
        ),
        1
    );
    assert_eq!(
        mine(
            pelty_database::billable::bill_recurring(&st.pool, horizon, 7, 1000)
                .await
                .unwrap()
        ),
        0,
        "advanced a period"
    );
    let l = call(
        &app,
        "GET",
        &format!("/api/v1/admin/billable-items?customer_id={cid}"),
        None,
        &admin.h(),
    )
    .await;
    let row = l.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == rid.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["recur_remaining"], 1);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/billable-items/{rid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    // One-off lines on a draft invoice; refused once finalized.
    let d = call(&app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "items": [{"description": "Base", "quantity": "1", "unit_price": "1"}]})), &admin.h()).await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    let did = d.body["id"]
        .as_str()
        .or(d.body["invoice"]["id"].as_str())
        .unwrap()
        .to_string();
    let add =
        json!({"items": [{"description": "Rush fee", "quantity": "1", "unit_price": "9.99"}]});
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/invoices/{did}/items"),
            Some(add.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(items_of(&st, &did).await.len(), 2);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/invoices/{did}/finalize"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/invoices/{did}/items"),
            Some(add),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
}
