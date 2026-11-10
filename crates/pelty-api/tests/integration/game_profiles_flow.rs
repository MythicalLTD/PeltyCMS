use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::json;

/// Write-only server field that carries the RCON credential.
const RCON_FIELD: &str = "rcon_password";

/// Game servers, per-server stores and linked game accounts; everything is
/// 404 while the module is off.
#[tokio::test]
async fn servers_store_and_accounts() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let client = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    // Generated per run: no fixed credential-looking literals in the repo.
    let rcon_secret = uuid::Uuid::new_v4().simple().to_string();

    // Off: routes do not exist.
    for (m, u, h) in [
        ("GET", "/api/v1/public/game/games", vec![]),
        ("GET", "/api/v1/admin/game/servers", admin.h().to_vec()),
        ("GET", "/api/v1/client/game/accounts", client.h().to_vec()),
    ] {
        assert_eq!(
            call(&app, m, u, None, &h).await.status,
            StatusCode::NOT_FOUND,
            "{u}"
        );
    }
    enable(&app, &admin, &["game_profiles"]).await;

    let r = call(&app, "GET", "/api/v1/public/game/games", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let ids: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    for g in [
        "minecraft_java",
        "fivem",
        "rust",
        "gmod",
        "ark",
        "cs2",
        "generic",
    ] {
        assert!(ids.contains(&g), "{g}");
    }

    let srv = json!({
        "game": "minecraft_java", "name": "Survival", "slug": format!("survival-{tag}"),
        "host": "play.example.com", "port": 25565, "transport": "rcon",
        "rcon_host": "127.0.0.1", "rcon_port": 25575
    });
    let mut srv = srv;
    srv[RCON_FIELD] = json!(rcon_secret);
    let mut bad = srv.clone();
    bad["host"] = json!("http://evil");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/game/servers",
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // Clients cannot manage servers.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/game/servers",
            Some(srv.clone()),
            &client.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/game/servers",
        Some(srv.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["has_rcon_password"], true);
    assert!(r.body.get("rcon_password").is_none());
    let sid = r.body["id"].as_str().unwrap().to_string();

    // Update without a password keeps it; empty string clears.
    let mut upd = srv.clone();
    upd.as_object_mut().unwrap().remove(RCON_FIELD);
    upd["name"] = json!("Survival EU");
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{sid}"),
        Some(upd.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["has_rcon_password"], true);
    assert_eq!(r.body["name"], "Survival EU");
    let sealed = pelty_database::games::rcon_secret(&st.pool, sid.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pelty_api::routes::games::open_secret(&st, &sealed).as_deref(),
        Some(rcon_secret.as_str())
    );

    // Store per server.
    let (pid, _, _) = seed_product(&app, &admin).await;
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{sid}/products"),
        Some(json!({"product_ids": [pid]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/game/servers/survival-{tag}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["packages"][0]["id"], pid.as_str());
    assert!(r.body["server"].get("rcon_host").is_none());
    assert!(r.body["packages"][0]["prices"].as_array().unwrap().len() >= 2);
    let r = call(&app, "GET", "/api/v1/public/game/servers", None, &[]).await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == sid.as_str()));

    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/game/servers/{sid}/agent-token"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["token"].as_str().unwrap().starts_with("pga_"));

    // Linked accounts.
    for bad in [
        json!({"game": "minecraft_java", "field": "username", "value": "x;op me"}),
        json!({"game": "minecraft_java", "field": "license", "value": "ab"}),
        json!({"game": "nope", "field": "username", "value": "Steve"}),
    ] {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/client/game/accounts",
                Some(bad.clone()),
                &client.h()
            )
            .await
            .status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}"
        );
    }
    let r = call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "minecraft_java", "field": "username", "value": " Steve "})),
        &client.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["value"], "Steve");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "minecraft_java", "field": "username", "value": "Alex"})),
        &client.h(),
    )
    .await;
    let aid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "GET",
        "/api/v1/client/game/accounts",
        None,
        &client.h(),
    )
    .await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["items"][0]["value"], "Alex");
    // Another customer cannot remove it.
    let other = sess(&app, &st, "customer").await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/client/game/accounts/{aid}"),
            None,
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/client/game/accounts/{aid}"),
            None,
            &client.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/game/servers/{sid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    reset_modules(&st).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/public/game/servers", None, &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
