//! Bundle ownership discount (module `bundle_ownership`): items a buyer
//! already owns complete a bundle, priced by share or list price with a
//! floor, shown in the cart and on the invoice; owning everything blocks it.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn dec(v: &Value) -> Decimal {
    v.as_str()
        .map(|s| s.parse().unwrap())
        .or_else(|| v.as_f64().map(|f| Decimal::try_from(f).unwrap()))
        .unwrap_or_else(|| panic!("not a decimal: {v}"))
}
fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

#[tokio::test]
async fn owned_items_discount_the_rest_of_a_bundle() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": format!("Packs {tag}"), "slug": format!("packs-{tag}")})),
        &admin.h(),
    )
    .await;
    let g = r.body["id"].as_str().unwrap().to_string();
    let mut p = vec![];
    for (i, amount) in ["10.00", "10.00", "20.00"].iter().enumerate() {
        let r = call(&app, "POST", "/api/v1/admin/catalog/products", Some(json!({"group_id": g, "name": format!("Res {i} {tag}"), "slug": format!("res-{i}-{tag}")})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let pid = r.body["id"].as_str().unwrap().to_string();
        let r = call(
            &app,
            "PUT",
            &format!("/api/v1/admin/catalog/products/{pid}/prices"),
            Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": amount}]})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        p.push(pid);
    }
    let one = |pid: &str| json!({"product_id": pid, "cycle": "one_time"});
    let r = call(&app, "POST", "/api/v1/admin/bundles", Some(json!({"name": format!("Mega {tag}"), "discount_percent": "20", "items": [one(&p[0]), one(&p[1]), one(&p[2])]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let bundle = r.body["id"].as_str().unwrap().to_string();

    let pay = |order: Value| {
        let app = app.clone();
        let h = admin.h().map(|(k, v)| (k.to_string(), v.to_string()));
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let inv = order["invoice"]["id"].as_str().unwrap().to_string();
            let total = order["invoice"]["total"].as_str().unwrap().to_string();
            let r = call(
                &app,
                "POST",
                &format!("/api/v1/admin/invoices/{inv}/payments"),
                Some(json!({"amount": total, "idempotency_key": uuid::Uuid::new_v4().to_string()})),
                &h,
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            let oid = order["order"]["id"].as_str().unwrap();
            let r = call(
                &app,
                "POST",
                &format!("/api/v1/admin/orders/{oid}/approve"),
                None,
                &h,
            )
            .await;
            assert!(
                r.status == StatusCode::OK || r.status == StatusCode::CONFLICT,
                "{:?}",
                r.body
            );
        }
    };
    // Bob owns item 0.
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [one(&p[0])]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    pay(o.body).await;

    let cart = uuid::Uuid::new_v4();
    let preview = |items: Vec<Value>| {
        let app = app.clone();
        let h = bob.h().map(|(k, v)| (k.to_string(), v.to_string()));
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let r = call(
                &app,
                "PUT",
                &format!("/api/v1/store/carts/{cart}"),
                Some(json!({"currency": "EUR", "items": items})),
                &h,
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body
        }
    };
    let rest = || vec![one(&p[1]), one(&p[2])];

    // Module off: the rest alone is not a bundle.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/public/bundle-ownership",
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let v = preview(rest()).await;
    assert_eq!(v["preview"]["bundle"], Value::Null);
    assert_eq!(dec(&v["preview"]["total"]), d("30"));

    enable(&app, &admin, &["bundle_ownership"]).await;
    let r = call(
        &app,
        "GET",
        "/api/v1/public/bundle-ownership",
        None,
        &bob.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["mode"], "share");
    assert!(r.body["owned"].as_array().unwrap().contains(&json!(p[0])));
    assert!(!r.body["owned"].as_array().unwrap().contains(&json!(p[1])));
    let anon = call(&app, "GET", "/api/v1/public/bundle-ownership", None, &[]).await;
    assert_eq!(anon.body["owned"], json!([]));

    // Share mode: the rest keeps the bundle percent: 30 * 80% = 24.
    let v = preview(rest()).await;
    assert_eq!(v["preview"]["bundle"], json!(format!("Mega {tag}")));
    assert_eq!(
        (
            v["preview"]["bundle_owned"].as_u64(),
            v["preview"]["bundle_size"].as_u64()
        ),
        (Some(1), Some(3))
    );
    assert_eq!(dec(&v["preview"]["bundle_discount"]), d("6"));
    assert_eq!(dec(&v["preview"]["total"]), d("24"));
    // Missing an item the buyer does not own: no bundle.
    let v = preview(vec![one(&p[2])]).await;
    assert_eq!(v["preview"]["bundle"], Value::Null);

    // List mode: bundle price 40 * 80% = 32 minus owned list price 10 = 22.
    let set = |settings: Value| {
        let app = app.clone();
        let h = admin.h().map(|(k, v)| (k.to_string(), v.to_string()));
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let r = call(
                &app,
                "PUT",
                "/api/v1/admin/modules/bundle_ownership",
                Some(json!({"enabled": true, "settings": settings})),
                &h,
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        }
    };
    set(json!({"mode": "list", "min_percent": "0"})).await;
    let v = preview(rest()).await;
    assert_eq!(dec(&v["preview"]["total"]), d("22"));
    // Floor: the rest costs at least 90% of its list price (27).
    set(json!({"mode": "list", "min_percent": "90"})).await;
    let v = preview(rest()).await;
    assert_eq!(dec(&v["preview"]["total"]), d("27"));
    set(json!({"mode": "list", "min_percent": "0"})).await;

    // Claiming the bundle on a client order works without the owned item,
    // and the invoice says why.
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": rest(), "bundle_id": bundle})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(dec(&o.body["invoice"]["total"]), d("22"));
    let inv = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    let lines: Vec<String> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["description"].as_str().unwrap().to_string())
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l == &format!("Bundle discount: Mega {tag} (1 of 3 already owned)")),
        "{lines:?}"
    );
    pay(o.body).await;

    // Owning everything: re-buying the whole bundle is refused, in the cart
    // preview, at store checkout and on a client order claiming it.
    let all = || vec![one(&p[0]), one(&p[1]), one(&p[2])];
    let v = preview(all()).await;
    assert_eq!(v["preview"], Value::Null);
    assert_eq!(v["error"], "error.bundle_all_owned");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/checkout",
        Some(json!({"cart_id": cart})),
        &bob.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.bundle_all_owned");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": all(), "bundle_id": bundle})),
        &bob.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    // Another customer who owns nothing still gets the plain bundle.
    let eve = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/store/carts/{}", uuid::Uuid::new_v4()),
        Some(json!({"currency": "EUR", "items": all()})),
        &eve.h(),
    )
    .await;
    assert_eq!(dec(&r.body["preview"]["total"]), d("32"));
    assert_eq!(r.body["preview"]["bundle_owned"], 0);
}
