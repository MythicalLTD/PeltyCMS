//! Creator marketplace: mode switch, creator approval, listing review,
//! sale accrual minus commission, payouts (manual, stub provider, reject),
//! and refund reversal. 404 while the module is off.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn dec(v: &Value) -> Decimal {
    v.as_str().unwrap().parse().unwrap()
}

async fn mode(app: &axum::Router, admin: &Sess, settings: Value) {
    let r = call(
        app,
        "PUT",
        "/api/v1/admin/modules/creator_marketplace",
        Some(json!({"enabled": true, "settings": settings})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

async fn balance(app: &axum::Router, s: &Sess) -> Value {
    let d = call(app, "GET", "/api/v1/client/creator", None, &s.h()).await;
    d.body["balances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["currency"] == "EUR")
        .cloned()
        .unwrap_or(json!({"available": "0", "pending": "0"}))
}

#[tokio::test]
async fn creators_listings_sales_and_payouts() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let carol = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];

    assert_eq!(
        call(&app, "GET", "/api/v1/public/creators", None, &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["digital_products"]).await;
    mode(&app, &admin, json!({"mode": "admin_only"})).await;
    let profile = json!({"slug": format!("carol-{tag}"), "display_name": "Carol Dev", "bio": "Plugins", "website": "https://carol.example", "payout_method": "manual"});
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator",
        Some(profile.clone()),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body["error"]["code"], "error.market_closed");
    mode(&app, &admin, json!({"mode": "multi_vendor", "commission_percent": "20", "hold_days": 0, "min_payout": "5"})).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/public/marketplace", None, &[])
            .await
            .body["mode"],
        "multi_vendor"
    );

    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator",
        Some(profile.clone()),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "pending");
    let cid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/creator",
            Some(profile.clone()),
            &carol.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let mut taken = profile.clone();
    taken["slug"] = json!(format!("carol-{tag}"));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/creator",
            Some(taken),
            &bob.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Plugins", "slug": format!("plug-{tag}")})),
        &admin.h(),
    )
    .await;
    let gid = g.body["id"].as_str().unwrap().to_string();
    let listing = json!({"group_id": gid, "name": "Shop Plugin", "slug": format!("shop-plugin-{tag}"), "description": "Sells things", "currency": "EUR", "cycle": "one_time", "price": "50.00"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/creator/listings",
            Some(listing.clone()),
            &carol.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN,
        "pending creators cannot list"
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creators/{cid}/status"),
        Some(json!({"status": "approved"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator/listings",
        Some(listing.clone()),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "pending");
    assert_eq!(r.body["product_status"], "hidden");
    let pid = r.body["product_id"].as_str().unwrap().to_string();
    let page = call(
        &app,
        "GET",
        &format!("/api/v1/public/creators/carol-{tag}"),
        None,
        &[],
    )
    .await;
    assert_eq!(page.body["listings"].as_array().unwrap().len(), 0);
    let q = call(
        &app,
        "GET",
        "/api/v1/admin/creator-listings?status=pending",
        None,
        &admin.h(),
    )
    .await;
    assert!(q.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["product_id"] == pid.as_str()));
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/creator-listings/{pid}/review"),
            Some(json!({"approve": true})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let page = call(
        &app,
        "GET",
        &format!("/api/v1/public/creators/carol-{tag}"),
        None,
        &[],
    )
    .await;
    assert_eq!(page.body["listings"][0]["name"], "Shop Plugin");
    assert!(
        page.body["creator"].get("payout_details").is_none(),
        "payout details stay private"
    );

    // Sale: 50.00 at 20% commission → 40.00 for the creator.
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order = o.body["order"]["id"].as_str().unwrap().to_string();
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("mk-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let mut b = json!(null);
    for _ in 0..100 {
        b = balance(&app, &carol).await;
        if dec(&b["available"]) > Decimal::ZERO {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(dec(&b["available"]), Decimal::from(40), "{b:?}");
    assert_eq!(dec(&b["commission"]), Decimal::from(10));
    let l = call(
        &app,
        "GET",
        "/api/v1/client/creator/ledger",
        None,
        &carol.h(),
    )
    .await;
    assert_eq!(l.body["items"][0]["kind"], "sale");

    // Payouts.
    let req = |amount: &str| json!({"currency": "EUR", "amount": amount});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/creator/payouts",
            Some(req("100")),
            &carol.h()
        )
        .await
        .body["error"]["code"],
        "error.payout_insufficient"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/creator/payouts",
            Some(req("3")),
            &carol.h()
        )
        .await
        .body["error"]["code"],
        "error.payout_below_minimum"
    );
    let po = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(req("30")),
        &carol.h(),
    )
    .await;
    assert_eq!(po.status, StatusCode::OK, "{:?}", po.body);
    let po = po.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        dec(&balance(&app, &carol).await["available"]),
        Decimal::from(10)
    );
    let sent = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po}/send"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(sent.body["status"], "processing", "{:?}", sent.body);
    let paid = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po}/status"),
        Some(json!({"status": "paid", "provider_ref": "bank-123"})),
        &admin.h(),
    )
    .await;
    assert_eq!(paid.body["status"], "paid");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/creator-payouts/{po}/status"),
            Some(json!({"status": "rejected"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // A stub provider refuses; rejecting credits the amount back.
    let mut pp = profile.clone();
    pp["payout_method"] = json!("paypal");
    pp["payout_details"] = json!({"paypal_email": "carol@example.com"});
    assert_eq!(
        call(&app, "PUT", "/api/v1/client/creator", Some(pp), &carol.h())
            .await
            .status,
        StatusCode::OK
    );
    let po2 = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(req("10")),
        &carol.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let s2 = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po2}/send"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(s2.status, StatusCode::CONFLICT);
    assert!(s2.body["error"]["code"]
        .as_str()
        .unwrap()
        .starts_with("error.payout_provider_"));
    assert_eq!(
        dec(&balance(&app, &carol).await["available"]),
        Decimal::ZERO
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po2}/status"),
        Some(json!({"status": "rejected", "note": "use bank"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        dec(&balance(&app, &carol).await["available"]),
        Decimal::from(10)
    );

    // Refund reverses the sale.
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/refund"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mut ok = false;
    for _ in 0..100 {
        if dec(&balance(&app, &carol).await["available"]) == Decimal::from(-30) {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ok, "{:?}", balance(&app, &carol).await);

    // Admin-only mode keeps data but closes selling.
    mode(&app, &admin, json!({"mode": "admin_only"})).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/creator/listings",
            Some(listing),
            &carol.h()
        )
        .await
        .body["error"]["code"],
        "error.market_closed"
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/creator", None, &carol.h())
            .await
            .body["creator"]["id"],
        cid.as_str()
    );
}
