use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::json;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

#[tokio::test]
async fn metered_usage_billed_once_at_cycle_end() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;
    // Make sure the customer row exists.
    call(&app, "GET", "/api/v1/client/services", None, &cust.h()).await;
    let (pid, _, _) = seed_product(&app, &admin).await;
    let today = time::OffsetDateTime::now_utc().date();
    let due = today + time::Duration::days(10);
    let sid: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date)
         SELECT gen_random_uuid(), c.id, $2::uuid, 'active', 'monthly', c.currency, 0, $3 FROM customers c JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1 RETURNING id",
    ).bind(&cust.email).bind(uuid::Uuid::parse_str(&pid).unwrap()).bind(due).fetch_one(&state.pool).await.unwrap();

    // Metric catalog.
    assert_eq!(call(&app, "GET", "/api/v1/admin/usage-metrics", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "Bad Key", "name": "x", "unit_price": "1"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "bw", "name": "Bandwidth", "unit": "GB", "unit_price": "-1"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "bw", "name": "Bandwidth", "unit": "GB", "unit_price": "0.00012345", "included_quantity": "10"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let bw = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "cpu", "name": "CPU hours", "unit": "h", "unit_price": "0.015"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "idle", "name": "Idle", "unit_price": "5", "included_quantity": "100"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "bw", "name": "Dup", "unit_price": "1"})), &admin.h()).await.status, StatusCode::CONFLICT);

    let rec = |metric: &str, q: &str, key: &str| json!({"metric": metric, "quantity": q, "idempotency_key": key});
    let url = format!("/api/v1/admin/services/{sid}/usage");
    assert_eq!(call(&app, "POST", &url, Some(rec("bw", "1", "a")), &cust.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", &url, Some(rec("nope", "1", "a")), &admin.h()).await.body["error"]["code"], "error.unknown_metric");
    assert_eq!(call(&app, "POST", &url, Some(rec("bw", "0", "a")), &admin.h()).await.body["error"]["code"], "error.invalid_quantity");
    let future = (time::OffsetDateTime::now_utc() + time::Duration::days(1)).format(&time::format_description::well_known::Rfc3339).unwrap();
    assert_eq!(call(&app, "POST", &url, Some(json!({"metric": "bw", "quantity": "1", "idempotency_key": "f", "recorded_at": future})), &admin.h()).await.body["error"]["code"], "error.usage_in_future");

    let r = call(&app, "POST", &url, Some(rec("bw", "1000.25", "a1")), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["period"]["period_end"], due.to_string());
    // Retried delivery is deduplicated.
    let r = call(&app, "POST", &url, Some(rec("bw", "1000.25", "a1")), &admin.h()).await;
    assert_eq!(r.body["duplicate"], true);
    call(&app, "POST", &url, Some(rec("bw", "10.25", "a2")), &admin.h()).await;
    call(&app, "POST", &url, Some(rec("cpu", "33.333333", "c1")), &admin.h()).await;
    call(&app, "POST", &url, Some(rec("idle", "3", "i1")), &admin.h()).await;

    let r = call(&app, "GET", &format!("/api/v1/client/services/{sid}/usage"), None, &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let m = &r.body["items"][0]["metrics"];
    let bwm = m.as_array().unwrap().iter().find(|x| x["key"] == "bw").unwrap();
    assert_eq!(d(bwm["quantity"].as_str().unwrap()), d("1010.5"));
    assert_eq!(d(bwm["billable"].as_str().unwrap()), d("1000.5"));
    assert_eq!(call(&app, "GET", &format!("/api/v1/client/services/{sid}/usage"), None, &other.h()).await.status, StatusCode::NOT_FOUND);

    // Not due yet: nothing billed.
    assert_eq!(pelty_database::usage::bill_due(&state.pool, today).await.unwrap(), 0);
    let pid_open: uuid::Uuid = uuid::Uuid::parse_str(r.body["items"][0]["period"]["id"].as_str().unwrap()).unwrap();
    // Cycle end: billed exactly once.
    let n = pelty_database::usage::bill_due(&state.pool, due).await.unwrap();
    assert!(n >= 1);
    assert_eq!(pelty_database::usage::bill_period(&state.pool, pid_open).await.unwrap(), None, "second run is a no-op");
    let r = call(&app, "GET", &url, None, &admin.h()).await;
    let p = &r.body["items"][0]["period"];
    assert_eq!(p["status"], "billed");
    // bw: 1000.5 × 0.00012345 = 0.1235117… → 0.12; cpu: 33.333333 × 0.015 = 0.499999995 → 0.50; idle: within allowance → no line.
    assert_eq!(d(p["total"].as_str().unwrap()), d("0.62"));
    let iid = p["invoice_id"].as_str().unwrap();
    let inv = call(&app, "GET", &format!("/api/v1/admin/invoices/{iid}"), None, &admin.h()).await;
    let items = inv.body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "{:?}", inv.body);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/usage-metrics/{bw}"), None, &admin.h()).await.status, StatusCode::CONFLICT);

    // Usage dated inside an already-billed period is rejected, never re-billed.
    let r = call(&app, "POST", &url, Some(rec("bw", "5", "late")), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.usage_period_closed");
    // Once the service renews, new usage opens the next period at the old end.
    let next = due + time::Duration::days(30);
    sqlx::query("UPDATE services SET next_due_date = $2 WHERE id = $1").bind(sid).bind(next).execute(&state.pool).await.unwrap();
    sqlx::query("UPDATE usage_periods SET period_start = period_start - 20, period_end = $2 WHERE id = $1").bind(pid_open).bind(today).execute(&state.pool).await.unwrap();
    let r = call(&app, "POST", &url, Some(rec("bw", "5", "next")), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["period"]["period_start"], today.to_string());
    assert_eq!(r.body["period"]["period_end"], next.to_string());
    assert_eq!(r.body["period"]["status"], "open");
}
