//! Gaming storefront: 404 while off, strict settings validation, and the
//! public layout with resolved widgets (featured package, recent payments,
//! top customer; community widgets dropped while `server_community` is off).
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn layout_widgets_and_validation() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    for (m, p) in [
        ("GET", "/api/v1/public/storefront"),
        ("GET", "/api/v1/admin/storefront"),
    ] {
        assert_eq!(
            call(&app, m, p, None, &admin.h()).await.status,
            StatusCode::NOT_FOUND,
            "{p}"
        );
    }
    enable(&app, &admin, &["gaming_storefront"]).await;

    let def = call(&app, "GET", "/api/v1/admin/storefront", None, &admin.h()).await;
    assert_eq!(def.status, StatusCode::OK, "{:?}", def.body);
    assert_eq!(def.body["accent"], "default");
    assert_eq!(def.body["widgets"][0]["type"], "recent_payments");
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/storefront", None, &bob.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // A paid order for a tagged product, to show up in recent payments.
    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Crates", "slug": format!("cr-{tag}")})),
        &admin.h(),
    )
    .await;
    let name = format!("Crate {tag}");
    let p = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": g.body["id"], "name": name, "slug": format!("crate-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let pid = p.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "4.00"}]})),
        &admin.h(),
    )
    .await;
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let pay = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("sf-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(pay.status, StatusCode::OK, "{:?}", pay.body);

    for bad in [
        json!({"hero_image": "javascript:alert(1)"}),
        json!({"accent": "neon"}),
        json!({"widgets": [{"type": "html", "body": "<script>"}]}),
        json!({"widgets": [{"type": "discord", "invite_url": "https://evil.example/x", "label": "x"}]}),
        json!({"widgets": [{"type": "recent_payments", "limit": 99}]}),
        json!({"unknown": true}),
    ] {
        let r = call(
            &app,
            "PUT",
            "/api/v1/admin/storefront",
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
    let cfg = json!({
        "hero_title": "Play with us", "hero_subtitle": "Ranks and crates", "hero_image": "/uploads/banner.png",
        "accent": "violet", "layout": "grid", "sidebar": "left",
        "widgets": [
            {"type": "text", "title": "Rules", "body": "<b>No refunds</b> on crates."},
            {"type": "featured_package", "product_id": pid},
            {"type": "featured_package", "product_id": uuid::Uuid::new_v4()},
            {"type": "recent_payments", "limit": 10},
            {"type": "top_customer", "period": "all", "show_amount": true},
            {"type": "server_status"},
            {"type": "goal"},
            {"type": "discord", "invite_url": "https://discord.gg/pelty-test", "label": "Join Discord"}
        ]
    });
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/storefront",
        Some(cfg),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let pubv = call(&app, "GET", "/api/v1/public/storefront", None, &[]).await;
    assert_eq!(pubv.status, StatusCode::OK, "{:?}", pubv.body);
    assert_eq!(pubv.body["accent"], "violet");
    assert_eq!(pubv.body["layout"], "grid");
    let types: Vec<&str> = pubv.body["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["type"].as_str().unwrap())
        .collect();
    // Unknown product, server status and goal are dropped (community off).
    assert_eq!(
        types,
        [
            "text",
            "featured_package",
            "recent_payments",
            "top_customer",
            "discord"
        ]
    );
    let w = &pubv.body["widgets"];
    assert_eq!(
        w[0]["body"], "<b>No refunds</b> on crates.",
        "stored as text, escaped by the client"
    );
    assert_eq!(w[1]["product"]["name"], name.as_str());
    assert!(
        w[2]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["products"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == name.as_str())),
        "{:?}",
        w[2]
    );
    assert!(w[3]["customer"]["name"].is_string() || w[3]["customer"].is_null());
    assert!(
        !pubv.body.to_string().contains('@'),
        "no emails in public widgets"
    );

    // With the community module on, the status widget appears.
    enable(&app, &admin, &["game_profiles", "server_community"]).await;
    let pubv = call(&app, "GET", "/api/v1/public/storefront", None, &[]).await;
    assert!(pubv.body["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["type"] == "server_status"));

    // Hidden products are not featured.
    sqlx::query("UPDATE products SET status = 'hidden' WHERE id = $1::uuid")
        .bind(&pid)
        .execute(&st.pool)
        .await
        .unwrap();
    let pubv = call(&app, "GET", "/api/v1/public/storefront", None, &[]).await;
    assert!(!pubv.body["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["type"] == "featured_package"));
    reset_modules(&st).await;
}
