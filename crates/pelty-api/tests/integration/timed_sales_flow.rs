//! Timed sales: 404 while off, validation, a running group sale discounting
//! the cart preview and the first invoice total, highest sale wins, ended or
//! disabled sales ignored, and no discount while the module is off.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

fn at(d: Duration) -> String {
    (OffsetDateTime::now_utc() + d).format(&Rfc3339).unwrap()
}

/// Invoice total as a plain decimal ("7.5000" -> "7.5").
fn total(inv: &Value) -> String {
    let t = inv["total"].as_str().unwrap_or("?");
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t.to_string()
    }
}

#[tokio::test]
async fn sales_discount_preview_and_first_invoice() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/sales", None, &admin.h())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/public/sales", None, &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Two products in one group, one product in another.
    let mut groups = vec![];
    for g in ["a", "b"] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/catalog/groups",
            Some(json!({"name": format!("Sale {g} {tag}"), "slug": format!("sale-{g}-{tag}")})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        groups.push(r.body["id"].as_str().unwrap().to_string());
    }
    let mut products = vec![];
    for (i, g) in [&groups[0], &groups[0], &groups[1]].into_iter().enumerate() {
        let r = call(&app, "POST", "/api/v1/admin/catalog/products", Some(json!({"group_id": g, "name": format!("Rank {i} {tag}"), "slug": format!("rank-{i}-{tag}")})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let pid = r.body["id"].as_str().unwrap().to_string();
        let r = call(
            &app,
            "PUT",
            &format!("/api/v1/admin/catalog/products/{pid}/prices"),
            Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "10.00"}]})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        products.push(pid);
    }

    let order = |pid: String| {
        let app = app.clone();
        let h = bob.h();
        async move {
            let o = call(
                &app,
                "POST",
                "/api/v1/client/orders",
                Some(
                    json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]}),
                ),
                &h,
            )
            .await;
            assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
            o.body["invoice"].clone()
        }
    };
    let cart = |pid: String| {
        let app = app.clone();
        async move {
            let url = format!("/api/v1/store/carts/{}", uuid::Uuid::new_v4());
            let r = call(&app, "PUT", &url, Some(json!({"currency": "EUR", "country": "QZ", "items": [{"product_id": pid, "cycle": "one_time"}]})), &[]).await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body["preview"].clone()
        }
    };

    enable(&app, &admin, &["timed_sales"]).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/sales", None, &bob.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    for bad in [
        json!({"name": "", "percent": "20", "group_ids": [groups[0]], "starts_at": at(Duration::ZERO), "ends_at": at(Duration::days(1))}),
        json!({"name": "x", "percent": "96", "group_ids": [groups[0]], "starts_at": at(Duration::ZERO), "ends_at": at(Duration::days(1))}),
        json!({"name": "x", "percent": "20", "starts_at": at(Duration::ZERO), "ends_at": at(Duration::days(1))}),
        json!({"name": "x", "percent": "20", "group_ids": [groups[0]], "starts_at": at(Duration::days(1)), "ends_at": at(Duration::ZERO)}),
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/sales",
            Some(bad.clone()),
            &admin.h(),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad} -> {:?}",
            r.body
        );
    }
    // Running: 20 % on group A, 25 % on product 0 (wins there).
    let mk = |body: Value| {
        let app = app.clone();
        let h = admin.h();
        async move {
            let r = call(&app, "POST", "/api/v1/admin/sales", Some(body), &h).await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body["id"].as_str().unwrap().to_string()
        }
    };
    mk(json!({"name": format!("Group {tag}"), "percent": "20", "group_ids": [groups[0]], "starts_at": at(-Duration::hours(1)), "ends_at": at(Duration::days(1))})).await;
    let best = mk(json!({"name": format!("Best {tag}"), "percent": "25", "product_ids": [products[0]], "starts_at": at(-Duration::hours(1)), "ends_at": at(Duration::days(1))})).await;
    // Ended, scheduled and disabled sales on group B never apply.
    mk(json!({"name": "Old", "percent": "50", "group_ids": [groups[1]], "starts_at": at(-Duration::days(2)), "ends_at": at(-Duration::days(1))})).await;
    mk(json!({"name": "Soon", "percent": "50", "group_ids": [groups[1]], "starts_at": at(Duration::days(1)), "ends_at": at(Duration::days(2))})).await;
    mk(json!({"name": "Off", "percent": "50", "group_ids": [groups[1]], "starts_at": at(-Duration::hours(1)), "ends_at": at(Duration::days(1)), "enabled": false})).await;

    let public = call(&app, "GET", "/api/v1/public/sales", None, &[]).await;
    let names: Vec<&str> = public.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&format!("Best {tag}").as_str())
            && !names.contains(&"Old")
            && !names.contains(&"Soon")
            && !names.contains(&"Off"),
        "{names:?}"
    );

    let p = cart(products[0].clone()).await;
    assert_eq!(p["sale_discount"], "2.50", "{p:?}");
    assert_eq!(p["lines"][0]["sale"], format!("Best {tag}"));
    assert_eq!(p["total"], "7.50", "{p:?}");
    assert_eq!(cart(products[1].clone()).await["sale_discount"], "2.00");
    assert_eq!(cart(products[2].clone()).await["sale_discount"], "0");

    let inv = order(products[0].clone()).await;
    assert_eq!(total(&inv), "7.5", "{inv:?}");
    let inv = order(products[1].clone()).await;
    assert_eq!(total(&inv), "8", "{inv:?}");
    assert_eq!(total(&order(products[2].clone()).await), "10");

    // Disabling the best sale falls back to the group sale.
    let r = call(&app, "PUT", &format!("/api/v1/admin/sales/{best}"), Some(json!({"name": format!("Best {tag}"), "percent": "25", "product_ids": [products[0]], "starts_at": at(-Duration::hours(1)), "ends_at": at(Duration::days(1)), "enabled": false})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(cart(products[0].clone()).await["sale_discount"], "2.00");

    // Module off: no discount anywhere.
    reset_modules(&st).await;
    assert_eq!(cart(products[0].clone()).await["sale_discount"], "0");
    assert_eq!(total(&order(products[0].clone()).await), "10");
}
