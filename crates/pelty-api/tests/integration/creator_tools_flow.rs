//! Creator tools: creator discount codes (own listings only, cut the
//! creator's earnings), creator bundles (shown on the resource page, applied
//! automatically in the store cart), wishlists, buyer <-> creator messages
//! and listing analytics. 404 while the marketplace module is off.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn dec(v: &Value) -> Decimal {
    v.as_str()
        .map(|s| s.parse().unwrap())
        .unwrap_or_else(|| v.to_string().parse().unwrap())
}

async fn ok(app: &axum::Router, s: &Sess, method: &str, uri: &str, body: Option<Value>) -> Value {
    let r = call(app, method, uri, body, &s.h()).await;
    assert!(
        r.status.is_success(),
        "{method} {uri}: {} {:?}",
        r.status,
        r.body
    );
    r.body
}

async fn pay(app: &axum::Router, admin: &Sess, inv: &Value) {
    let iid = inv["id"].as_str().unwrap();
    ok(
        app,
        admin,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": inv["total"], "idempotency_key": format!("ct-{iid}")})),
    )
    .await;
}

/// Gross of the creator's sale ledger rows for one product (polls the async accrual).
async fn sale_gross(app: &axum::Router, creator: &Sess, product: &str, n: usize) -> Vec<Decimal> {
    for _ in 0..100 {
        let l = ok(app, creator, "GET", "/api/v1/client/creator/ledger", None).await;
        let rows: Vec<Decimal> = l["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["kind"] == "sale" && x["product_id"] == product)
            .map(|x| dec(&x["gross"]))
            .collect();
        if rows.len() >= n {
            return rows;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("no accrual for {product}");
}

#[tokio::test]
async fn coupons_bundles_wishlist_messages_analytics() {
    let _serial = CREATOR_PAYOUTS.lock().await;
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let nova = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let eve = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];

    assert_eq!(
        call(&app, "GET", "/api/v1/client/wishlist", None, &bob.h())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["digital_products", "creator_marketplace"]).await;
    ok(&app, &admin, "PUT", "/api/v1/admin/modules/creator_marketplace", Some(json!({"enabled": true, "settings": {"mode": "multi_vendor", "commission_percent": "20", "hold_days": 0, "min_payout": "5"}}))).await;

    let slug = format!("nova{tag}");
    let c = ok(
        &app,
        &nova,
        "POST",
        "/api/v1/client/creator",
        Some(json!({"slug": slug, "display_name": "Nova", "payout_method": "manual"})),
    )
    .await;
    let cid = c["id"].as_str().unwrap();
    ok(
        &app,
        &admin,
        "POST",
        &format!("/api/v1/admin/creators/{cid}/status"),
        Some(json!({"status": "approved"})),
    )
    .await;
    let g = ok(
        &app,
        &admin,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Plugins", "slug": format!("ct-{tag}")})),
    )
    .await;
    let mut ids = vec![];
    for (name, price) in [("Alpha", "50.00"), ("Beta", "30.00")] {
        let l = ok(&app, &nova, "POST", "/api/v1/client/creator/listings", Some(json!({"group_id": g["id"], "name": name, "slug": format!("{}-{tag}", name.to_lowercase()), "description": "x", "currency": "EUR", "cycle": "one_time", "price": price}))).await;
        let pid = l["product_id"].as_str().unwrap().to_string();
        ok(
            &app,
            &admin,
            "POST",
            &format!("/api/v1/admin/creator-listings/{pid}/review"),
            Some(json!({"approve": true})),
        )
        .await;
        ids.push(pid);
    }
    let (a, b) = (ids[0].clone(), ids[1].clone());
    // A product Nova does not own.
    let other = ok(
        &app,
        &admin,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": g["id"], "name": "House", "slug": format!("house-{tag}")})),
    )
    .await;
    let other_id = other["id"].as_str().unwrap().to_string();

    // ---- discount codes
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator/coupons",
        Some(json!({"code": "x", "kind": "percent", "value": "10", "product_ids": [other_id]})),
        &nova.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.creator_products_invalid");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator/coupons",
        Some(json!({"code": "x", "kind": "percent", "value": "10", "product_ids": []})),
        &nova.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let cp = ok(&app, &nova, "POST", "/api/v1/client/creator/coupons", Some(json!({"code": "spring", "kind": "percent", "value": "10", "max_uses_per_customer": 1, "product_ids": [a]}))).await;
    let code = cp["code"].as_str().unwrap().to_string();
    assert_eq!(code, format!("{}-SPRING", slug.to_ascii_uppercase()));
    assert_eq!(cp["product_ids"], json!([a]));
    let mine = ok(&app, &nova, "GET", "/api/v1/client/creator/coupons", None).await;
    assert_eq!(mine["items"].as_array().unwrap().len(), 1);
    // Someone else cannot edit it.
    let r = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/client/creator/coupons/{}",
            cp["id"].as_str().unwrap()
        ),
        Some(json!({"code": "spring", "kind": "percent", "value": "90", "product_ids": [a]})),
        &eve.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // Bob buys Alpha with it: 50 - 10% = 45 gross for the creator.
    let o = ok(&app, &bob, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "coupon": code, "items": [{"product_id": a, "cycle": "one_time"}]}))).await;
    assert_eq!(dec(&o["invoice"]["total"]), Decimal::from(45), "{o:?}");
    pay(&app, &admin, &o["invoice"]).await;
    assert_eq!(
        sale_gross(&app, &nova, &a, 1).await,
        vec![Decimal::from(45)]
    );
    // Used: delete only switches it off.
    let d = ok(
        &app,
        &nova,
        "DELETE",
        &format!(
            "/api/v1/client/creator/coupons/{}",
            cp["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(d["deleted"], false);

    // ---- bundles
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator/bundles",
        Some(json!({"name": "Duo", "discount_percent": "20", "product_ids": [a]})),
        &nova.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let bd = ok(&app, &nova, "POST", "/api/v1/client/creator/bundles", Some(json!({"name": "Duo", "description": "Both", "discount_percent": "20", "product_ids": [a, b]}))).await;
    assert_eq!(bd["items"].as_array().unwrap().len(), 2);
    assert_eq!(bd["items"][0]["cycle"], "one_time");
    let page = ok(
        &app,
        &eve,
        "GET",
        &format!("/api/v1/public/marketplace/listings/{a}"),
        None,
    )
    .await;
    assert_eq!(page["bundles"][0]["name"], "Duo", "{page:?}");
    // Store cart holding both: bundle applied without a code (80 * 20% = 16).
    let cart = uuid::Uuid::new_v4();
    let v = ok(&app, &eve, "PUT", &format!("/api/v1/store/carts/{cart}"), Some(json!({"currency": "EUR", "items": [{"product_id": a, "cycle": "one_time"}, {"product_id": b, "cycle": "one_time"}]}))).await;
    assert_eq!(v["preview"]["bundle"], "Duo", "{v:?}");
    assert_eq!(dec(&v["preview"]["bundle_discount"]), Decimal::from(16));
    let out = ok(
        &app,
        &eve,
        "POST",
        "/api/v1/client/checkout",
        Some(json!({"cart_id": cart})),
    )
    .await;
    assert_eq!(dec(&out["invoice"]["total"]), Decimal::from(64), "{out:?}");
    pay(&app, &admin, &out["invoice"]).await;
    // The creator bears the bundle discount: 50 - 10 and 30 - 6.
    assert!(sale_gross(&app, &nova, &a, 2)
        .await
        .contains(&Decimal::from(40)));
    assert_eq!(
        sale_gross(&app, &nova, &b, 1).await,
        vec![Decimal::from(24)]
    );

    // ---- wishlist
    ok(
        &app,
        &bob,
        "PUT",
        &format!("/api/v1/client/wishlist/{b}"),
        None,
    )
    .await;
    ok(
        &app,
        &bob,
        "PUT",
        &format!("/api/v1/client/wishlist/{b}"),
        None,
    )
    .await;
    let w = ok(&app, &bob, "GET", "/api/v1/client/wishlist", None).await;
    assert_eq!(w["items"].as_array().unwrap().len(), 1);
    assert_eq!(w["items"][0]["creator_slug"], slug.as_str());
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/client/wishlist/{}", uuid::Uuid::new_v4()),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    ok(
        &app,
        &bob,
        "DELETE",
        &format!("/api/v1/client/wishlist/{b}"),
        None,
    )
    .await;
    assert_eq!(
        ok(&app, &bob, "GET", "/api/v1/client/wishlist", None).await["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    ok(
        &app,
        &eve,
        "PUT",
        &format!("/api/v1/client/wishlist/{a}"),
        None,
    )
    .await;

    // ---- messages
    let r = call(
        &app,
        "POST",
        "/api/v1/client/market-messages",
        Some(json!({"product_id": a, "subject": "Hi", "body": "me"})),
        &nova.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.message_self");
    let cv = ok(&app, &bob, "POST", "/api/v1/client/market-messages", Some(json!({"product_id": a, "subject": "Config question", "body": "How do I set the shop title?"}))).await;
    let conv = cv["id"].as_str().unwrap().to_string();
    assert_eq!(cv["verified_buyer"], true, "Bob paid for Alpha");
    let inbox = ok(&app, &nova, "GET", "/api/v1/client/market-messages", None).await;
    assert_eq!(inbox["unread"], 1);
    assert_eq!(inbox["items"][0]["subject"], "Config question");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/market-messages/{conv}"),
            None,
            &eve.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let t = ok(
        &app,
        &nova,
        "GET",
        &format!("/api/v1/client/market-messages/{conv}"),
        None,
    )
    .await;
    assert_eq!(t["as_creator"], true);
    assert_eq!(t["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        ok(&app, &nova, "GET", "/api/v1/client/market-messages", None).await["unread"],
        0
    );
    ok(
        &app,
        &nova,
        "POST",
        &format!("/api/v1/client/market-messages/{conv}/reply"),
        Some(json!({"body": "Use /shop title."})),
    )
    .await;
    let bi = ok(&app, &bob, "GET", "/api/v1/client/market-messages", None).await;
    assert_eq!(bi["unread"], 1);
    let t = ok(
        &app,
        &bob,
        "GET",
        &format!("/api/v1/client/market-messages/{conv}"),
        None,
    )
    .await;
    assert_eq!(t["messages"][1]["from_creator"], true);
    ok(
        &app,
        &bob,
        "POST",
        &format!("/api/v1/client/market-messages/{conv}/status"),
        Some(json!({"open": false})),
    )
    .await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/market-messages/{conv}/reply"),
        Some(json!({"body": "late"})),
        &nova.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    // Eve bought Beta in the bundle; Bob did not.
    let cv = ok(
        &app,
        &eve,
        "POST",
        "/api/v1/client/market-messages",
        Some(json!({"product_id": b, "subject": "Pre-sale", "body": "Does it work on 1.21?"})),
    )
    .await;
    assert_eq!(cv["verified_buyer"], true, "Eve bought the bundle");
    let cv = ok(
        &app,
        &bob,
        "POST",
        "/api/v1/client/market-messages",
        Some(json!({"product_id": b, "subject": "Pre-sale", "body": "Does it work on 1.21?"})),
    )
    .await;
    assert_eq!(cv["verified_buyer"], false);

    // ---- analytics
    for _ in 0..3 {
        ok(
            &app,
            &eve,
            "GET",
            &format!("/api/v1/public/marketplace/listings/{a}"),
            None,
        )
        .await;
    }
    let mut an = json!(null);
    for _ in 0..100 {
        an = ok(
            &app,
            &nova,
            "GET",
            &format!("/api/v1/client/creator/listings/{a}/analytics?days=7"),
            None,
        )
        .await;
        if an["totals"]["views"].as_i64() >= Some(4) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(an["series"].as_array().unwrap().len(), 7);
    assert_eq!(an["totals"]["views"], 4, "{an:?}");
    assert_eq!(an["totals"]["sales"], 2);
    assert_eq!(dec(&an["totals"]["revenue"]), Decimal::from(85));
    assert_eq!(dec(&an["totals"]["earnings"]), Decimal::from(68));
    assert_eq!(dec(&an["totals"]["conversion"]), Decimal::from(50));
    assert_eq!(an["totals"]["wishlists"], 1);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/creator/listings/{a}/analytics"),
            None,
            &eve.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    reset_modules(&st).await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/market-messages",
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
