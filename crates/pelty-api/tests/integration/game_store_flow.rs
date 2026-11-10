use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

/// Product images and the game-store site settings round-trip to the public API.
#[tokio::test]
async fn product_image_and_store_mode() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];

    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "VIP", "slug": format!("vip{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let group = g.body["id"].as_str().unwrap().to_string();

    let create = |image: &str| {
        let body = json!({
            "group_id": group,
            "name": "VIP Gold",
            "slug": format!("gold{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
            "image_url": image,
        });
        let (app, admin) = (&app, &admin);
        async move {
            call(
                app,
                "POST",
                "/api/v1/admin/catalog/products",
                Some(body),
                &admin.h(),
            )
            .await
        }
    };
    for bad in ["javascript:alert(1)", "//evil.example/x.png", "/a b.png"] {
        assert_eq!(
            create(bad).await.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}"
        );
    }
    let p = create("/api/v1/media/gold.png").await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert_eq!(p.body["image_url"], "/api/v1/media/gold.png");
    let id = p.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{id}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "15.00"}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let find = |body: &serde_json::Value| {
        body["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == id.as_str())
            .cloned()
            .expect("product in public catalog")
    };
    let cat = call(&app, "GET", "/api/v1/catalog", None, &[]).await;
    assert_eq!(find(&cat.body)["image_url"], "/api/v1/media/gold.png");

    // A patch that leaves the image out keeps it; an empty string clears it.
    let uri = format!("/api/v1/admin/catalog/products/{id}");
    let r = call(
        &app,
        "PATCH",
        &uri,
        Some(json!({"name": "VIP Gold+"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["image_url"], "/api/v1/media/gold.png", "{:?}", r.body);
    let r = call(
        &app,
        "PATCH",
        &uri,
        Some(json!({"image_url": ""})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["image_url"], "", "{:?}", r.body);

    let original = call(&app, "GET", "/api/v1/admin/site", None, &admin.h())
        .await
        .body["config"]
        .clone();
    assert_eq!(original["store_mode"], "hosting");
    let put = |cfg: serde_json::Value| {
        let (app, admin) = (&app, &admin);
        async move {
            call(app, "PUT", "/api/v1/admin/site", Some(cfg), &admin.h())
                .await
                .status
        }
    };
    let mut cfg = original.clone();
    cfg["store_mode"] = json!("casino");
    assert_eq!(put(cfg.clone()).await, StatusCode::UNPROCESSABLE_ENTITY);
    cfg["store_mode"] = json!("game");
    cfg["game_join_code"] = json!("https://cfx.re/join/abc123");
    assert_eq!(put(cfg.clone()).await, StatusCode::UNPROCESSABLE_ENTITY);
    cfg["game_join_code"] = json!("abc123");
    assert_eq!(put(cfg).await, StatusCode::OK);
    let site = call(&app, "GET", "/api/v1/public/site", None, &[]).await;
    assert_eq!(site.body["config"]["store_mode"], "game");
    assert_eq!(site.body["config"]["game_join_code"], "abc123");
    assert_eq!(put(original).await, StatusCode::OK);
}
