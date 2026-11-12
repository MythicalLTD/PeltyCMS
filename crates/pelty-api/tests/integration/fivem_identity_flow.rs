//! FiveM store checkout: player IDs we already know (Login with FiveM /
//! Steam / Discord or an earlier purchase) are filled, a missing ID the
//! package's delivery needs (the Rockstar licence) is asked at checkout and
//! saved to the game account, and package checkout fields mapped to a
//! player ID are filled from it. Guests get the same requirement.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

const LICENSE: &str = "license:0123456789ABCDEF0123456789abcdef01234567";

async fn cart(app: &axum::Router, pid: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/store/carts/{id}"),
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    id
}

fn field<'a>(games: &'a Value, game: &str, f: &str) -> &'a Value {
    games
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["game"] == game)
        .unwrap_or_else(|| panic!("{game} in {games:?}"))["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["field"] == f)
        .unwrap_or_else(|| panic!("{f} in {games:?}"))
}

#[tokio::test]
async fn fivem_checkout_asks_missing_ids_and_fills_package_fields() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    enable(
        &app,
        &admin,
        &["game_profiles", "game_delivery", "guest_checkout"],
    )
    .await;

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": format!("RP {tag}"), "slug": format!("rp-{tag}")})),
        &admin.h(),
    )
    .await;
    let g = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": g, "name": format!("VIP {tag}"), "slug": format!("vip-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "5.00"}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "POST", "/api/v1/admin/game/servers", Some(json!({"game": "fivem", "name": format!("RP {tag}"), "slug": format!("rp-id-{tag}"), "transport": "agent", "guest_checkout": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let srv = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{srv}/products"),
        Some(json!({"product_ids": [pid]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // The `fivem` identity is a known placeholder now.
    let r = call(&app, "PUT", &format!("/api/v1/admin/game/delivery/products/{pid}/commands"), Some(json!({"commands": [
        {"trigger": "purchase", "command": "add_principal identifier.license:{license} group.vip"},
        {"trigger": "purchase", "command": "say fivem:{fivem} bought {package}"}
    ]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(
        r.body["placeholders"].is_null()
            || r.body["placeholders"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "fivem")
    );

    // Package checkout fields: one free text, one filled from the Cfx.re id.
    let base = format!("/api/v1/admin/catalog/products/{pid}/fields");
    let r = call(
        &app,
        "POST",
        &base,
        Some(json!({"name": "Character name", "kind": "text", "required": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let char_field = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        &base,
        Some(
            json!({"name": "Cfx.re ID", "kind": "text", "required": true, "identity_key": "fivem"}),
        ),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["identity_key"], "fivem");
    let cfx_field = r.body["id"].as_str().unwrap().to_string();
    for bad in [
        json!({"name": "X", "kind": "text", "identity_key": "password"}),
        json!({"name": "X", "kind": "select", "choices": ["a"], "identity_key": "license"}),
    ] {
        let r = call(&app, "POST", &base, Some(bad), &admin.h()).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
        assert_eq!(r.body["error"]["code"], "error.invalid_field_identity");
    }
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/catalog/products/{pid}/extras"),
        None,
        &[],
    )
    .await;
    let fields = r.body["fields"].as_array().unwrap();
    assert!(
        fields
            .iter()
            .any(|f| f["id"] == cfx_field.as_str() && f["identity_key"] == "fivem"),
        "{fields:?}"
    );

    // Alice signed in with FiveM earlier: her Cfx.re id is on her account.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "fivem", "field": "fivem", "value": "fivem:4242"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["value"], "4242");

    let c1 = cart(&app, &pid).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/game/checkout-identity/{c1}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let games = &r.body["games"];
    assert_eq!(field(games, "fivem", "fivem")["value"], "4242");
    assert_eq!(field(games, "fivem", "fivem")["required"], true);
    assert!(field(games, "fivem", "license")["value"].is_null());
    assert_eq!(field(games, "fivem", "license")["required"], true);
    assert_eq!(field(games, "fivem", "steamid")["required"], false);

    let checkout = |body: Value| {
        let app = app.clone();
        let h: Vec<(String, String)> = alice
            .h()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            call(&app, "POST", "/api/v1/client/checkout", Some(body), &h).await
        }
    };
    let fields = json!({"0": {char_field.clone(): "Niko Bellic"}});
    // Licence missing: asked, checkout refused.
    let r = checkout(json!({"cart_id": c1, "fields": fields})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.game_identity_required");
    let r = checkout(json!({"cart_id": c1, "fields": fields, "identity": {"fivem": {"license": "license:nothex"}}})).await;
    assert_eq!(
        r.body["error"]["code"], "error.game_identity_invalid",
        "{:?}",
        r.body
    );
    let r = checkout(json!({"cart_id": c1, "fields": fields, "identity": {"rust": {"steamid": "76561198000000000"}}})).await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "game not in cart"
    );
    // Typed licence: saved to the account, order placed, Cfx field filled.
    let r = checkout(
        json!({"cart_id": c1, "fields": fields, "identity": {"fivem": {"license": LICENSE}}}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let order = r.body["order"]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "GET",
        "/api/v1/client/game/accounts",
        None,
        &alice.h(),
    )
    .await;
    let lic = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["game"] == "fivem" && a["field"] == "license")
        .cloned()
        .expect("licence saved");
    assert_eq!(lic["value"], "0123456789abcdef0123456789abcdef01234567");
    let cf: Value =
        sqlx::query_scalar("SELECT custom_fields FROM order_items WHERE order_id = $1::uuid")
            .bind(&order)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(cf[&cfx_field], "fivem:4242", "{cf:?}");
    assert_eq!(cf[&char_field], "Niko Bellic", "{cf:?}");

    // Next purchase: everything known, nothing to type.
    let c2 = cart(&app, &pid).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/game/checkout-identity/{c2}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(
        field(&r.body["games"], "fivem", "license")["value"],
        "0123456789abcdef0123456789abcdef01234567"
    );
    let r = checkout(json!({"cart_id": c2, "fields": {"0": {char_field.clone(): "Niko"}}})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // A typed licence never replaces the linked one from checkout.
    let c3 = cart(&app, &pid).await;
    let r = checkout(json!({"cart_id": c3, "fields": {"0": {char_field.clone(): "Niko"}}, "identity": {"fivem": {"license": "license:ffffffffffffffffffffffffffffffffffffffff"}}})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        "/api/v1/client/game/accounts",
        None,
        &alice.h(),
    )
    .await;
    assert!(r.body["items"].as_array().unwrap().iter().any(
        |a| a["field"] == "license" && a["value"] == "0123456789abcdef0123456789abcdef01234567"
    ));

    // Guests: the licence the delivery needs is required too.
    let g1 = cart(&app, &pid).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/store/guest-checkout/{g1}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["required"], json!(["license"]), "{:?}", r.body);
    assert_eq!(r.body["fields"], json!(["username", "license", "steamid"]));
    let r = call(
        &app,
        "POST",
        "/api/v1/store/guest-checkout",
        Some(json!({"cart_id": g1, "email": email(), "identity": {"username": "Niko"}})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.game_identity_required");
    reset_modules(&st).await;
}
