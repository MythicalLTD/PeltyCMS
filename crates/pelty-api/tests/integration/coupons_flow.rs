use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn code(r: &Resp) -> String {
    r.body["error"]["code"].as_str().unwrap_or("").to_string()
}

fn uniq(p: &str) -> String {
    format!("{p}{}", &uuid::Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}

#[tokio::test]
async fn coupons_discount_orders_with_limits_restrictions_and_recurring() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let (pid2, oid2, ch2) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let pct = uniq("PCT");
    let fix = uniq("FIX");
    let old = uniq("OLD");

    let body = json!({"code": pct.to_lowercase(), "kind": "percent", "value": "20", "recurring": true, "max_uses_per_customer": 1, "product_ids": [pid]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/coupons",
            Some(body.clone()),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let c = call(
        &app,
        "POST",
        "/api/v1/admin/coupons",
        Some(body),
        &admin.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    assert_eq!(
        c.body["code"],
        pct.as_str(),
        "codes are normalized to upper case"
    );
    let f = call(
        &app,
        "POST",
        "/api/v1/admin/coupons",
        Some(json!({"code": fix, "kind": "fixed", "value": "3", "currency": "EUR", "max_uses": 1})),
        &admin.h(),
    )
    .await;
    assert_eq!(f.status, StatusCode::OK, "{:?}", f.body);
    let fix_id = f.body["id"].as_str().unwrap().to_string();
    let e = call(&app, "POST", "/api/v1/admin/coupons", Some(json!({"code": old, "kind": "percent", "value": "10", "expires_at": "2020-01-01T00:00:00Z"})), &admin.h()).await;
    assert_eq!(e.status, StatusCode::OK, "{:?}", e.body);
    let bad = call(
        &app,
        "POST",
        "/api/v1/admin/coupons",
        Some(json!({"code": "X", "kind": "percent", "value": "10"})),
        &admin.h(),
    )
    .await;
    assert_eq!(code(&bad), "error.invalid_coupon_code");
    let bad = call(
        &app,
        "POST",
        "/api/v1/admin/coupons",
        Some(json!({"code": "FIXNOCUR", "kind": "fixed", "value": "10"})),
        &admin.h(),
    )
    .await;
    assert_eq!(code(&bad), "error.invalid_coupon_value");

    // Client check.
    let ok = call(
        &app,
        "POST",
        "/api/v1/client/coupons/check",
        Some(json!({"code": pct, "currency": "EUR"})),
        &alice.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["kind"], "percent");
    assert!(
        ok.body.get("uses").is_none(),
        "usage counters stay internal"
    );
    let nope = call(
        &app,
        "POST",
        "/api/v1/client/coupons/check",
        Some(json!({"code": "NOPE-NOT-REAL", "currency": "EUR"})),
        &alice.h(),
    )
    .await;
    assert_eq!(code(&nope), "error.coupon_invalid");
    let exp = call(
        &app,
        "POST",
        "/api/v1/client/coupons/check",
        Some(json!({"code": old, "currency": "EUR"})),
        &alice.h(),
    )
    .await;
    assert_eq!(code(&exp), "error.coupon_expired");

    let order = |s: &Sess, p: &str, o: &str, c: &str, coupon: &str| {
        let b = json!({"currency": "EUR", "coupon": coupon, "items": [{"product_id": p, "cycle": "monthly", "config": {o: c}}]});
        let h = s.h().map(|(k, v)| (k.to_string(), v.to_string()));
        let app = app.clone();
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            call(&app, "POST", "/api/v1/client/orders", Some(b), &h).await
        }
    };

    // Restricted coupon on another product → not applicable.
    assert_eq!(
        code(&order(&alice, &pid2, &oid2, &ch2[0], &pct).await),
        "error.coupon_not_applicable"
    );
    // 10 + 5 setup − 20% of 10 = 13.
    let r = order(&alice, &pid, &oid, &ch[0], &pct).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "13.0000");
    let o1 = r.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{o1}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    let rec: Decimal = sqlx::query_scalar("SELECT s.recurring_amount FROM services s JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1::uuid")
        .bind(&o1).fetch_one(&st.pool).await.unwrap();
    assert_eq!(rec, Decimal::new(8, 0), "recurring coupon lowers renewals");
    // Per-customer limit.
    assert_eq!(
        code(&order(&alice, &pid, &oid, &ch[0], &pct).await),
        "error.coupon_exhausted"
    );
    // Expired at order time.
    assert_eq!(
        code(&order(&alice, &pid, &oid, &ch[0], &old).await),
        "error.coupon_expired"
    );

    // Fixed, first invoice only, global limit 1.
    let r = order(&bob, &pid2, &oid2, &ch2[0], &fix.to_lowercase()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "12.0000");
    let o2 = r.body["order"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        code(&order(&alice, &pid2, &oid2, &ch2[0], &fix).await),
        "error.coupon_exhausted"
    );
    // Cancelling releases the use.
    let cx = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{o2}/cancel"),
        Some(json!({})),
        &admin.h(),
    )
    .await;
    assert_eq!(cx.status, StatusCode::OK, "{:?}", cx.body);
    let r = order(&alice, &pid2, &oid2, &ch2[0], &fix).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let o3 = r.body["order"]["id"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{o3}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let rec: Decimal = sqlx::query_scalar("SELECT s.recurring_amount FROM services s JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1::uuid")
        .bind(&o3).fetch_one(&st.pool).await.unwrap();
    assert_eq!(
        rec,
        Decimal::new(10, 0),
        "first-invoice coupon does not touch renewals"
    );

    // Used coupons can't be deleted; listing shows usage.
    let d = call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/coupons/{fix_id}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(code(&d), "error.coupon_in_use");
    let list = call(&app, "GET", "/api/v1/admin/coupons", None, &admin.h()).await;
    let row: &Value = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == fix_id.as_str())
        .unwrap();
    assert_eq!(row["uses"], 1);
}
