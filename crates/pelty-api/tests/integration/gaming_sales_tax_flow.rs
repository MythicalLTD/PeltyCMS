//! Sales by game server: net of tax (tax-inclusive prices too), and orders
//! whose invoice was merged into a paid replacement still count. Own suite:
//! it changes the global tax settings and the module list.
use crate::common;
use crate::gaming_reports_flow::run_path;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

fn num(v: &Value) -> rust_decimal::Decimal {
    v.as_str().unwrap().parse().unwrap()
}

async fn server_row(app: &axum::Router, admin: &Sess, name: &str) -> Option<Value> {
    let r = call(
        app,
        "GET",
        &run_path("game_server_sales", ""),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[0] == name)
        .cloned()
}

async fn pay(app: &axum::Router, admin: &Sess, inv: &Value) {
    let r = call(
        app,
        "POST",
        &format!(
            "/api/v1/admin/invoices/{}/payments",
            inv["id"].as_str().unwrap()
        ),
        Some(json!({"amount": inv["total"], "idempotency_key": uuid::Uuid::new_v4().to_string()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

/// Server sales are net of tax (tax-inclusive prices too) and still count an
/// order whose invoice was merged into a replacement that got paid.
#[tokio::test]
async fn game_server_sales_net_of_tax_and_counts_merged_invoices() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let buyer = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "LU"})),
        &buyer.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    enable(&app, &admin, &["game_profiles"]).await;
    let name = format!("Taxed {tag}");
    let srv = call(&app, "POST", "/api/v1/admin/game/servers",
        Some(json!({"game": "fivem", "name": name, "slug": format!("tax-{tag}"), "transport": "agent"})), &admin.h()).await;
    assert_eq!(srv.status, StatusCode::OK, "{:?}", srv.body);
    let srv = srv.body["id"].as_str().unwrap().to_string();
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{srv}/products"),
        Some(json!({"product_ids": [pid]})),
        &admin.h(),
    )
    .await;

    // 25% VAT on tax-inclusive prices: 10 + 5 gross is 12 net.
    let before = call(&app, "GET", "/api/v1/admin/settings/tax", None, &admin.h())
        .await
        .body;
    let r = call(&app, "PUT", "/api/v1/admin/settings/tax", Some(json!({"enabled": true, "inclusive": true, "home_country": "LU", "reverse_charge": false})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rule = call(
        &app,
        "POST",
        "/api/v1/admin/tax-rules",
        Some(json!({"name": format!("VAT {tag}"), "country": "LU", "rate": "25"})),
        &admin.h(),
    )
    .await;
    assert_eq!(rule.status, StatusCode::OK, "{:?}", rule.body);
    let rule_id = rule.body["id"].as_str().unwrap().to_string();

    let order = || async {
        let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]})), &buyer.h()).await;
        assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
        o.body["invoice"].clone()
    };
    let first = order().await;
    assert_eq!(num(&first["total"]), "15".parse().unwrap());
    assert_eq!(num(&first["tax_total"]), "3".parse().unwrap());
    pay(&app, &admin, &first).await;
    let row = server_row(&app, &admin, &name).await.expect("server row");
    assert_eq!(
        (num(&row[2]), row[3].clone()),
        ("12".parse().unwrap(), json!("1")),
        "{row:?}"
    );

    // Two more orders, their invoices merged; only the replacement is paid.
    let (a, b) = (order().await, order().await);
    let m = call(
        &app,
        "POST",
        "/api/v1/admin/invoices/merge",
        Some(json!({"invoice_ids": [a["id"], b["id"]]})),
        &admin.h(),
    )
    .await;
    assert_eq!(m.status, StatusCode::OK, "{:?}", m.body);
    assert!(m.body["invoice"]["order_id"].is_null());
    pay(&app, &admin, &m.body["invoice"]).await;
    let row = server_row(&app, &admin, &name).await.expect("server row");
    assert_eq!(
        (num(&row[2]), row[3].clone()),
        ("36".parse().unwrap(), json!("3")),
        "{row:?}"
    );

    // Restore the global tax setup for later suites.
    pelty_database::raw_execute(
        &st.pool,
        &format!("DELETE FROM tax_rules WHERE id = '{rule_id}'"),
    )
    .await
    .unwrap();
    let mut restore = before.clone();
    if !restore.is_object() {
        restore = json!({"enabled": false});
    }
    call(
        &app,
        "PUT",
        "/api/v1/admin/settings/tax",
        Some(restore),
        &admin.h(),
    )
    .await;
    reset_modules(&st).await;
}
