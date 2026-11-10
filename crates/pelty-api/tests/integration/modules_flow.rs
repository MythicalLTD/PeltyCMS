use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

/// Clean slate: every optional module off (a fresh install).
pub async fn reset_modules(st: &pelty_api::AppState) {
    sqlx::query("DELETE FROM feature_modules")
        .execute(&st.pool)
        .await
        .unwrap();
}

/// Enable modules in dependency order for a suite.
pub async fn enable(app: &axum::Router, admin: &Sess, ids: &[&str]) {
    for id in ids {
        let r = call(
            app,
            "PUT",
            &format!("/api/v1/admin/modules/{id}"),
            Some(json!({"enabled": true})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{id}: {:?}", r.body);
    }
}

/// Modules are off on a fresh install, toggle with dependency checks and
/// only staff with settings.manage can change them.
#[tokio::test]
async fn toggles_and_dependencies() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let client = sess(&app, &st, "customer").await;

    let r = call(&app, "GET", "/api/v1/public/modules", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["enabled"], json!([]));

    let r = call(&app, "GET", "/api/v1/admin/modules", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = r.body["items"].as_array().unwrap();
    assert!(items.iter().all(|m| m["enabled"] == false));
    assert!(items
        .iter()
        .any(|m| m["id"] == "creator_marketplace" && m["requires"] == json!(["digital_products"])));

    let put = |id: &str, body: serde_json::Value| {
        let (app, h) = (&app, admin.h());
        let uri = format!("/api/v1/admin/modules/{id}");
        async move { call(app, "PUT", &uri, Some(body), &h).await }
    };
    // Dependencies first.
    assert_eq!(
        put("game_delivery", json!({"enabled": true})).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        put("unknown_mod", json!({"enabled": true})).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        put("game_profiles", json!({"enabled": true, "settings": [1]}))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = put(
        "game_profiles",
        json!({"enabled": true, "settings": {"games": ["minecraft_java"]}}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["settings"]["games"], json!(["minecraft_java"]));
    assert_eq!(
        put("game_delivery", json!({"enabled": true})).await.status,
        StatusCode::OK
    );
    // A module another enabled one needs cannot be switched off.
    assert_eq!(
        put("game_profiles", json!({"enabled": false})).await.status,
        StatusCode::CONFLICT
    );

    let r = call(&app, "GET", "/api/v1/public/modules", None, &[]).await;
    assert_eq!(r.body["enabled"], json!(["game_profiles", "game_delivery"]));

    // Clients cannot toggle.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/modules/game_delivery",
        Some(json!({"enabled": false})),
        &client.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Settings survive a toggle that omits them.
    assert_eq!(
        put("game_delivery", json!({"enabled": false})).await.status,
        StatusCode::OK
    );
    let r = put("game_profiles", json!({"enabled": false})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["settings"]["games"], json!(["minecraft_java"]));
    reset_modules(&st).await;
}
