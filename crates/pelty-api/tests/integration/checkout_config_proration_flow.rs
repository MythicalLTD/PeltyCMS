//! Module-driven checkout-config + mid-cycle configurable-option proration.
use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_billing::{cycle, proration};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
use time::OffsetDateTime;

fn dec(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap_or_else(|| panic!("not a decimal: {v}"))).unwrap()
}

#[tokio::test]
async fn checkout_config_returns_fields() {
    let (app, _st) = app().await;
    let admin = sess(&app, &_st, "admin").await;
    let (pid, _oid, _ch) = seed_product(&app, &admin).await;

    let ex = call(
        &app,
        "GET",
        &format!("/api/v1/catalog/products/{pid}/extras"),
        None,
        &[],
    )
    .await;
    assert_eq!(ex.status, StatusCode::OK, "{:?}", ex.body);

    let cfg = call(
        &app,
        "POST",
        &format!("/api/v1/catalog/products/{pid}/checkout-config"),
        Some(json!({"selected": {}})),
        &[],
    )
    .await;
    assert_eq!(cfg.status, StatusCode::OK, "{:?}", cfg.body);
    assert!(cfg.body["fields"].is_array(), "{:?}", cfg.body);
    // Without a module that declares checkout fields, response mirrors extras.
    assert_eq!(
        cfg.body["fields"].as_array().unwrap().len(),
        ex.body["fields"].as_array().unwrap().len()
    );
}

#[tokio::test]
async fn option_upgrade_prorates_mid_cycle() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let today = OffsetDateTime::now_utc().date();

    let (pid, oid, ch) = seed_product(&app, &admin).await;
    // Add a pricey second-tier choice on the seeded Location option.
    let pricey = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO product_config_choices (id, option_id, label, sort_order) VALUES ($1,$2::uuid,'Premium', 50)",
    )
    .bind(pricey)
    .bind(&oid)
    .execute(&st.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_config_prices (choice_id, currency, cycle, amount) VALUES ($1,'EUR','monthly',10.00)",
    )
    .bind(pricey)
    .execute(&st.pool)
    .await
    .unwrap();

    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({
            "currency": "EUR",
            "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0].clone()}}]
        })),
        &alice.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["order"]["id"].as_str().unwrap();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    let sid = ap.body["services"][0].as_str().unwrap().to_string();

    let next = today + time::Duration::days(10);
    sqlx::query(
        "UPDATE services SET next_due_date = $2, recurring_amount = 10.00 WHERE id = $1::uuid",
    )
    .bind(&sid)
    .bind(next)
    .execute(&st.pool)
    .await
    .unwrap();

    let new_cfg = json!({ oid.clone(): pricey.to_string() });
    let q = call(
        &app,
        "POST",
        &format!("/api/v1/client/services/{sid}/options/quote"),
        Some(json!({ "config": new_cfg })),
        &alice.h(),
    )
    .await;
    assert_eq!(q.status, StatusCode::OK, "{:?}", q.body);
    assert_eq!(q.body["kind"], "upgrade", "{:?}", q.body);
    let new_amount = dec(&q.body["new_amount"]);
    let expect = proration::prorate(
        Decimal::from(10),
        new_amount,
        cycle::sub_months(next, 1),
        next,
        today,
        2,
    )
    .unwrap();
    assert_eq!(dec(&q.body["due_now"]), expect.net);

    let c = call(
        &app,
        "POST",
        &format!("/api/v1/client/services/{sid}/options/change"),
        Some(json!({ "config": new_cfg })),
        &alice.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    assert_eq!(c.body["change"]["status"], "awaiting_payment");
    assert!(c.body["invoice"]["id"].is_string(), "{:?}", c.body);
}
