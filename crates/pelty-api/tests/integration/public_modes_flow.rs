//! Public site modes: the site `public_mode` setting, the store theme and hero
//! server on `/public/storefront`, and the marketplace browse/resource API.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn put_storefront(app: &axum::Router, admin: &Sess, patch: Value) -> u16 {
    let mut cfg = call(app, "GET", "/api/v1/admin/storefront", None, &admin.h())
        .await
        .body;
    for (k, v) in patch.as_object().unwrap() {
        cfg[k] = v.clone();
    }
    call(
        app,
        "PUT",
        "/api/v1/admin/storefront",
        Some(cfg),
        &admin.h(),
    )
    .await
    .status
    .as_u16()
}

#[tokio::test]
async fn store_theme_hero_server_and_marketplace_browse() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let carol = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];

    // Site setting: validated, defaults to auto.
    let site = call(&app, "GET", "/api/v1/admin/site", None, &admin.h()).await;
    assert_eq!(site.status, StatusCode::OK, "{:?}", site.body);
    let original = site.body["config"].clone();
    assert_eq!(original["public_mode"], "auto");
    let mut cfg = original.clone();
    cfg["public_mode"] = json!("casino");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/site",
        Some(cfg.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    cfg["public_mode"] = json!("marketplace");
    let r = call(&app, "PUT", "/api/v1/admin/site", Some(cfg), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let public = call(&app, "GET", "/api/v1/public/site", None, &[]).await;
    assert_eq!(public.body["config"]["public_mode"], "marketplace");
    call(
        &app,
        "PUT",
        "/api/v1/admin/site",
        Some(original),
        &admin.h(),
    )
    .await;

    // Store: theme, Discord link and the hero server.
    enable(&app, &admin, &["game_profiles", "gaming_storefront"]).await;
    let srv = call(
        &app,
        "POST",
        "/api/v1/admin/game/servers",
        Some(json!({"game": "fivem", "name": format!("RP {tag}"), "slug": format!("rp-{tag}"), "host": "rp.example.com", "sort_order": -1000})),
        &admin.h(),
    )
    .await;
    assert_eq!(srv.status, StatusCode::OK, "{:?}", srv.body);
    let sid = srv.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        put_storefront(&app, &admin, json!({"theme": "neon"})).await,
        422
    );
    assert_eq!(
        put_storefront(
            &app,
            &admin,
            json!({"discord_url": "https://evil.example/x"})
        )
        .await,
        422
    );
    assert_eq!(
        put_storefront(
            &app,
            &admin,
            json!({"theme": "fivem", "discord_url": "https://discord.gg/rp", "logo_url": "/uploads/logo.png", "server_id": sid})
        )
        .await,
        200
    );
    let sf = call(&app, "GET", "/api/v1/public/storefront", None, &[]).await;
    assert_eq!(sf.status, StatusCode::OK, "{:?}", sf.body);
    assert_eq!(sf.body["theme"], "fivem");
    assert_eq!(sf.body["discord_url"], "https://discord.gg/rp");
    assert_eq!(sf.body["server"]["name"], format!("RP {tag}"));
    assert_eq!(sf.body["server"]["game"], "fivem");
    assert_eq!(sf.body["server"]["address"], "rp.example.com:30120");
    assert!(
        sf.body["server"]["status"].is_null(),
        "no live status without server_community"
    );
    assert!(sf.body["server"].get("rcon_host").is_none());
    put_storefront(
        &app,
        &admin,
        json!({"theme": "default", "discord_url": "", "logo_url": "", "server_id": null}),
    )
    .await;
    call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/game/servers/{sid}"),
        None,
        &admin.h(),
    )
    .await;

    // Marketplace browse and resource page.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/public/marketplace/listings",
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["digital_products"]).await;
    call(
        &app,
        "PUT",
        "/api/v1/admin/modules/creator_marketplace",
        Some(json!({"enabled": true, "settings": {"mode": "multi_vendor"}})),
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator",
        Some(json!({"slug": format!("dev-{tag}"), "display_name": format!("Dev {tag}"), "bio": "Scripts", "website": "", "payout_method": "manual"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cid = r.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/creators/{cid}/status"),
        Some(json!({"status": "approved"})),
        &admin.h(),
    )
    .await;
    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "FiveM scripts", "slug": format!("fivem-{tag}")})),
        &admin.h(),
    )
    .await;
    let gid = g.body["id"].as_str().unwrap().to_string();
    let mut ids = Vec::new();
    for n in ["Garage", "Phone"] {
        let r = call(
            &app,
            "POST",
            "/api/v1/client/creator/listings",
            Some(json!({"group_id": gid, "name": format!("{n} {tag}"), "slug": format!("{}-{tag}", n.to_lowercase()), "description": "A script", "currency": "EUR", "cycle": "one_time", "price": "10.00"})),
            &carol.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        ids.push(r.body["product_id"].as_str().unwrap().to_string());
    }
    let browse = |q: String| {
        let app = app.clone();
        async move {
            call(
                &app,
                "GET",
                &format!("/api/v1/public/marketplace/listings?{q}"),
                None,
                &[],
            )
            .await
        }
    };
    let r = browse(format!("category=fivem-{tag}")).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["items"].as_array().unwrap().len(),
        0,
        "pending listings stay hidden"
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-listings/{}/review", ids[0]),
        Some(json!({"approve": true})),
        &admin.h(),
    )
    .await;
    let r = browse(format!("category=fivem-{tag}&sort=new")).await;
    let items = r.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], format!("Garage {tag}"));
    assert_eq!(items[0]["creator_slug"], format!("dev-{tag}"));
    assert_eq!(items[0]["category"], "FiveM scripts");
    assert!(items[0]["rating"].is_null());
    assert!(r.body["categories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["slug"] == format!("fivem-{tag}") && c["listings"] == 1));
    let r = browse(format!("q=garage%20{tag}")).await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 1);
    let r = browse(format!("q=nothing-{tag}&sort=top_rated")).await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 0);
    let r = browse(format!("q={}", "x".repeat(101))).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let page = call(
        &app,
        "GET",
        &format!("/api/v1/public/marketplace/listings/{}", ids[0]),
        None,
        &[],
    )
    .await;
    assert_eq!(page.status, StatusCode::OK, "{:?}", page.body);
    assert_eq!(page.body["creator"]["slug"], format!("dev-{tag}"));
    assert!(page.body["creator"].get("payout_details").is_none());
    assert_eq!(page.body["digital"], true);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/public/marketplace/listings/{}", ids[1]),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "unapproved listing"
    );
    reset_modules(&st).await;
}
