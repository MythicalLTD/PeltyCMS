//! Upgrade (prorated invoice, applied on payment), scheduled downgrade,
//! and cancellation requests (end-of-period and immediate).
use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_billing::{cycle, proration};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
use time::OffsetDateTime;

fn dec(v: &Value) -> Decimal {
    Decimal::from_str(
        v.as_str()
            .unwrap_or_else(|| panic!("not a decimal string: {v}")),
    )
    .unwrap()
}

async fn product(
    app: &axum::Router,
    admin: &Sess,
    group: &str,
    name: &str,
    amount: &str,
) -> String {
    let slug = format!("p{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let p = call(
        app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": group, "name": name, "slug": slug})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let id = p.body["id"].as_str().unwrap().to_string();
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{id}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "monthly", "amount": amount}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    id
}

async fn service(
    app: &axum::Router,
    admin: &Sess,
    client: &Sess,
    pid: &str,
    oid: &str,
    ch: &str,
) -> String {
    let o = call(app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch}}]})), &client.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    ap.body["services"][0].as_str().unwrap().to_string()
}

#[tokio::test]
async fn upgrade_downgrade_and_cancellation() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let today = OffsetDateTime::now_utc().date();

    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let group: String =
        sqlx::query_scalar("SELECT group_id::text FROM products WHERE id = $1::uuid")
            .bind(&pid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    let big = product(&app, &admin, &group, "VPS L", "20.00").await;
    let small = product(&app, &admin, &group, "VPS XS", "5.00").await;

    let sid = service(&app, &admin, &alice, &pid, &oid, &ch[0]).await;
    // Pin the current period: [next_due - 1 month, next_due) with 10 days left.
    let next = today + time::Duration::days(10);
    sqlx::query(
        "UPDATE services SET next_due_date = $2, recurring_amount = 10.00 WHERE id = $1::uuid",
    )
    .bind(&sid)
    .bind(next)
    .execute(&st.pool)
    .await
    .unwrap();
    let base = format!("/api/v1/client/services/{sid}");

    assert_eq!(
        call(&app, "GET", &format!("{base}/lifecycle"), None, &bob.h())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let lc = call(&app, "GET", &format!("{base}/lifecycle"), None, &alice.h()).await;
    assert_eq!(lc.status, StatusCode::OK, "{:?}", lc.body);
    let opts = lc.body["options"].as_array().unwrap();
    let up = opts
        .iter()
        .find(|o| o["product_id"] == big.as_str())
        .expect("upgrade option");
    let down = opts
        .iter()
        .find(|o| o["product_id"] == small.as_str())
        .expect("downgrade option");
    assert_eq!(
        (up["kind"].as_str(), down["kind"].as_str()),
        (Some("upgrade"), Some("downgrade"))
    );
    let expect = proration::prorate(
        Decimal::from(10),
        Decimal::from(20),
        cycle::sub_months(next, 1),
        next,
        today,
        2,
    )
    .unwrap();
    assert_eq!(dec(&up["due_now"]), expect.net);
    assert_eq!(dec(&up["proration"]["credit"]), expect.credit);

    // Upgrade → unpaid invoice for exactly the net amount; not applied yet.
    let c = call(
        &app,
        "POST",
        &format!("{base}/change"),
        Some(json!({"product_id": big})),
        &alice.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    assert_eq!(c.body["change"]["status"], "awaiting_payment");
    assert_eq!(dec(&c.body["invoice"]["total"]), expect.net);
    let iid = c.body["invoice"]["id"].as_str().unwrap().to_string();
    let dup = call(
        &app,
        "POST",
        &format!("{base}/change"),
        Some(json!({"product_id": small})),
        &alice.h(),
    )
    .await;
    assert_eq!(dup.body["error"]["code"], "error.change_pending");
    let p: String = sqlx::query_scalar("SELECT product_id::text FROM services WHERE id = $1::uuid")
        .bind(&sid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(p, pid);

    let pay = call(&app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": expect.net.to_string(), "idempotency_key": uuid::Uuid::new_v4().to_string(), "reference": format!("bank-up-{iid}")})), &admin.h()).await;
    assert_eq!(pay.status, StatusCode::OK, "{:?}", pay.body);
    let (p, amt): (String, Decimal) = sqlx::query_as(
        "SELECT product_id::text, recurring_amount FROM services WHERE id = $1::uuid",
    )
    .bind(&sid)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!((p.as_str(), amt), (big.as_str(), Decimal::from(20)));

    // Downgrade: scheduled at next_due, new price for renewals, withdrawable.
    let d = call(
        &app,
        "POST",
        &format!("{base}/change"),
        Some(json!({"product_id": small})),
        &alice.h(),
    )
    .await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    assert_eq!(d.body["change"]["status"], "scheduled");
    assert_eq!(d.body["change"]["effective_date"], next.to_string());
    assert!(d.body["invoice"].is_null());
    let amt: Decimal =
        sqlx::query_scalar("SELECT recurring_amount FROM services WHERE id = $1::uuid")
            .bind(&sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(amt, Decimal::from(5));
    assert_eq!(
        call(&app, "DELETE", &format!("{base}/change"), None, &alice.h())
            .await
            .status,
        StatusCode::OK
    );
    let amt: Decimal =
        sqlx::query_scalar("SELECT recurring_amount FROM services WHERE id = $1::uuid")
            .bind(&sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(amt, Decimal::from(20));
    call(
        &app,
        "POST",
        &format!("{base}/change"),
        Some(json!({"product_id": small})),
        &alice.h(),
    )
    .await;
    sqlx::query("UPDATE service_changes SET effective_date = $2 WHERE service_id = $1::uuid AND status = 'scheduled'").bind(&sid).bind(today).execute(&st.pool).await.unwrap();
    pelty_api::changes::sweep(&st).await.unwrap();
    let p: String = sqlx::query_scalar("SELECT product_id::text FROM services WHERE id = $1::uuid")
        .bind(&sid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(p, small);

    // End-of-period cancellation: request → approve → effective at next_due.
    let bad = call(
        &app,
        "POST",
        &format!("{base}/cancellation"),
        Some(json!({"kind": "whenever"})),
        &alice.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("{base}/cancellation"),
        Some(json!({"kind": "end_of_period", "reason": "moving"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rid = r.body["cancellation"]["id"].as_str().unwrap().to_string();
    let again = call(
        &app,
        "POST",
        &format!("{base}/cancellation"),
        Some(json!({"kind": "immediate"})),
        &alice.h(),
    )
    .await;
    assert_eq!(again.body["error"]["code"], "error.cancellation_pending");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/cancellations?status=pending",
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let list = call(
        &app,
        "GET",
        "/api/v1/admin/cancellations?status=pending",
        None,
        &admin.h(),
    )
    .await;
    assert!(list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == rid.as_str()));
    let ok = call(
        &app,
        "POST",
        &format!("/api/v1/admin/cancellations/{rid}/decide"),
        Some(json!({"approve": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["cancellation"]["status"], "approved");
    assert_eq!(ok.body["cancellation"]["effective_date"], next.to_string());
    let (status, cancel_at): (String, Option<time::Date>) =
        sqlx::query_as("SELECT status, cancel_at FROM services WHERE id = $1::uuid")
            .bind(&sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), cancel_at), ("active", Some(next)));
    // Renewals stop at cancel_at.
    let renewals =
        pelty_database::services::bill_due(&st.pool, next + time::Duration::days(40), 100, 7)
            .await
            .unwrap();
    assert!(renewals.iter().all(|r| r.service_id.to_string() != sid));
    sqlx::query("UPDATE cancellation_requests SET effective_date = $2 WHERE id = $1::uuid")
        .bind(&rid)
        .bind(today)
        .execute(&st.pool)
        .await
        .unwrap();
    pelty_api::changes::sweep(&st).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM services WHERE id = $1::uuid")
        .bind(&sid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(status, "cancelled");

    // Immediate: rejected first, then approved → cancelled at once.
    let sid2 = service(&app, &admin, &alice, &pid, &oid, &ch[0]).await;
    let b2 = format!("/api/v1/client/services/{sid2}");
    let r = call(
        &app,
        "POST",
        &format!("{b2}/cancellation"),
        Some(json!({"kind": "immediate"})),
        &alice.h(),
    )
    .await;
    let rid = r.body["cancellation"]["id"].as_str().unwrap().to_string();
    let no = call(
        &app,
        "POST",
        &format!("/api/v1/admin/cancellations/{rid}/decide"),
        Some(json!({"approve": false, "note": "contract"})),
        &admin.h(),
    )
    .await;
    assert_eq!(no.body["cancellation"]["status"], "rejected");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/cancellations/{rid}/decide"),
            Some(json!({"approve": true})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let r = call(
        &app,
        "POST",
        &format!("{b2}/cancellation"),
        Some(json!({"kind": "immediate"})),
        &alice.h(),
    )
    .await;
    let rid = r.body["cancellation"]["id"].as_str().unwrap().to_string();
    let yes = call(
        &app,
        "POST",
        &format!("/api/v1/admin/cancellations/{rid}/decide"),
        Some(json!({"approve": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(yes.body["cancellation"]["status"], "completed");
    let status: String = sqlx::query_scalar("SELECT status FROM services WHERE id = $1::uuid")
        .bind(&sid2)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(status, "cancelled");
}
