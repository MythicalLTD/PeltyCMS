//! Custom-field kit: presets add only missing fields, staff keys, e-mail and
//! password fields validated at checkout, every field reaches the
//! provisioning module under its key (passwords opened), passwords are
//! sealed at rest and redacted in API responses, and delivery commands get
//! `{key}` placeholders with passwords filled only when sent.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

const PASSWORD: &str = "s3cret pass!";

fn by_key<'a>(items: &'a [Value], key: &str) -> &'a Value {
    items
        .iter()
        .find(|f| f["field_key"] == key || f["module_key"] == key)
        .unwrap_or_else(|| panic!("{key} in {items:?}"))
}

#[tokio::test]
async fn field_kit_presets_provisioning_passwords_and_delivery() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    enable(&app, &admin, &["game_profiles", "game_delivery"]).await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let base = format!("/api/v1/admin/catalog/products/{pid}/fields");

    // Presets are listed with the games they suit.
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/catalog/field-presets",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let presets = r.body["items"].as_array().unwrap();
    for id in [
        "fivem",
        "minecraft_java",
        "minecraft_bedrock",
        "steam",
        "account",
    ] {
        assert!(presets.iter().any(|p| p["id"] == id), "{id}");
    }
    let fivem = presets.iter().find(|p| p["id"] == "fivem").unwrap();
    assert_eq!(fivem["games"], json!(["fivem", "redm"]));

    // A FiveM store package: the editor knows the game.
    let r = call(&app, "POST", "/api/v1/admin/game/servers", Some(json!({"game": "fivem", "name": format!("RP {tag}"), "slug": format!("kit-{tag}"), "transport": "agent"})), &admin.h()).await;
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

    // Staff already had a licence field under another key.
    let r = call(&app, "POST", &base, Some(json!({"name": "Rockstar", "kind": "text", "identity_key": "license", "key": "rockstar"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["field_key"], "rockstar");

    let r = apply(&app, &admin.h(), &base, "fivem").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let added: Vec<_> = r.body["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["field_key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        added,
        ["fivem", "steamid", "discord_id", "username"],
        "licence already there"
    );
    assert_eq!(
        apply(&app, &admin.h(), &base, "fivem").await.body["added"],
        json!([]),
        "no duplicates"
    );
    let r = apply(&app, &admin.h(), &base, "account").await;
    assert_eq!(r.body["added"].as_array().unwrap().len(), 3, "{:?}", r.body);
    assert_eq!(
        apply(&app, &admin.h(), &base, "nope").await.body["error"]["code"],
        "error.invalid_field_preset"
    );

    // Keys: unique, valid, not a built-in placeholder; derived from the name.
    let r = call(
        &app,
        "POST",
        &base,
        Some(
            json!({"name": "Rank", "kind": "select", "choices": ["vip", "mvp"], "required": true}),
        ),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["field_key"], "rank", "{:?}", r.body);
    let rank = r.body["id"].as_str().unwrap().to_string();
    for (body, code) in [
        (
            json!({"name": "X", "kind": "text", "key": "rank"}),
            "error.field_key_taken",
        ),
        (
            json!({"name": "X", "kind": "text", "key": "server"}),
            "error.invalid_field_key",
        ),
        (
            json!({"name": "X", "kind": "text", "key": "Bad Key"}),
            "error.invalid_field_key",
        ),
        (
            json!({"name": "X", "kind": "password", "identity_key": "license"}),
            "error.invalid_field_identity",
        ),
    ] {
        let r = call(&app, "POST", &base, Some(body.clone()), &admin.h()).await;
        assert_eq!(r.body["error"]["code"], code, "{body} → {:?}", r.body);
    }

    let r = call(&app, "GET", &base, None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["games"], json!(["fivem"]));
    let items = r.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 9, "{items:?}");
    assert_eq!(by_key(&items, "username")["name"], "Nume jucător");
    assert_eq!(by_key(&items, "account_password")["kind"], "password");
    assert_eq!(by_key(&items, "account_email")["kind"], "email");
    assert!(by_key(&items, "account_password")["identity_key"].is_null());
    let id_of = |k: &str| by_key(&items, k)["id"].as_str().unwrap().to_string();

    // Reorder: the account fields first.
    let first: Vec<String> = ["account_username", "account_password", "account_email"]
        .iter()
        .map(|k| id_of(k))
        .collect();
    let mut order = first.clone();
    order.extend(
        items
            .iter()
            .map(|f| f["id"].as_str().unwrap().to_string())
            .filter(|i| !first.contains(i)),
    );
    let r = call(
        &app,
        "PUT",
        &format!("{base}/order"),
        Some(json!({"ids": order})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/catalog/products/{pid}/extras"),
        None,
        &[],
    )
    .await;
    assert_eq!(
        r.body["fields"][0]["field_key"], "account_username",
        "{:?}",
        r.body
    );

    // Delivery commands may use field keys; passwords are filled when sent.
    let r = call(&app, "PUT", &format!("/api/v1/admin/game/delivery/products/{pid}/commands"), Some(json!({"commands": [
        {"trigger": "purchase", "command": "createaccount {account_username} {account_password} {account_email} {rank}"}
    ]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/game/delivery/global-commands",
        Some(json!({"commands": [{"trigger": "purchase", "command": "say {rank}"}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "global commands only know built-in placeholders"
    );

    // Checkout validation.
    let lic = "license:0123456789abcdef0123456789abcdef01234567";
    let good = json!({
        id_of("rockstar"): lic, id_of("account_username"): "niko", id_of("account_password"): PASSWORD,
        id_of("account_email"): "niko@example.com", rank.clone(): "vip"
    });
    let mut bad_email = good.clone();
    bad_email[id_of("account_email")] = json!("not-an-email");
    assert_eq!(
        order_with(&app, &alice.h(), &pid, &oid, &ch[0], bad_email)
            .await
            .body["error"]["code"],
        "error.invalid_field"
    );
    let mut no_pw = good.clone();
    no_pw[id_of("account_password")] = json!("   ");
    assert_eq!(
        order_with(&app, &alice.h(), &pid, &oid, &ch[0], no_pw)
            .await
            .body["error"]["code"],
        "error.field_required"
    );
    let mut bad_rank = good.clone();
    bad_rank[&rank] = json!("god");
    assert_eq!(
        order_with(&app, &alice.h(), &pid, &oid, &ch[0], bad_rank)
            .await
            .body["error"]["code"],
        "error.invalid_field"
    );
    let r = order_with(&app, &alice.h(), &pid, &oid, &ch[0], good).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let order_id = r.body["order"]["id"].as_str().unwrap().to_string();

    // Sealed at rest.
    let cf: Value =
        sqlx::query_scalar("SELECT custom_fields FROM order_items WHERE order_id = $1::uuid")
            .bind(&order_id)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    let stored = cf[id_of("account_password")].as_str().unwrap();
    assert!(
        stored.starts_with("enc:v1:") && !stored.contains("s3cret"),
        "{stored}"
    );

    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let sid: uuid::Uuid = r.body["services"][0].as_str().unwrap().parse().unwrap();

    // The module sees every kit field under its key, password opened.
    let t = pelty_database::provisioning::target(&st.pool, sid)
        .await
        .unwrap()
        .unwrap();
    let c = &t.custom_fields;
    assert_eq!(c["account_password"], PASSWORD);
    assert_eq!(c[id_of("account_password")], PASSWORD);
    assert_eq!(c["account_username"], "niko");
    assert_eq!(c["account_email"], "niko@example.com");
    assert_eq!(c["rank"], "vip");
    assert_eq!(c["rockstar"], lic);
    assert_eq!(
        c["license"], lic,
        "identity field also under its identity key"
    );
    let ctx = pelty_provisioning::ServiceCtx {
        custom_fields: c.clone(),
        ..Default::default()
    };
    assert_eq!(ctx.field("account_password"), Some(PASSWORD));
    assert!(
        !format!("{ctx:?}").contains("s3cret"),
        "Debug never prints values"
    );

    // Redacted in API responses (client and staff).
    for (url, h) in [
        (format!("/api/v1/client/services/{sid}"), alice.h()),
        (format!("/api/v1/admin/services/{sid}"), admin.h()),
    ] {
        let r = call(&app, "GET", &url, None, &h).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let text = r.body.to_string();
        assert!(
            !text.contains("s3cret") && !text.contains("enc:v1:"),
            "{url}: {text}"
        );
        let svc = if r.body["service"].is_object() {
            &r.body["service"]
        } else {
            &r.body
        };
        assert_eq!(
            svc["custom_fields"][id_of("account_password")],
            "********",
            "{url}: {text}"
        );
        assert_eq!(svc["custom_fields"][id_of("account_username")], "niko");
    }

    // Queue keeps the placeholder; the agent gets the password at send time.
    let mut row = Value::Null;
    for _ in 0..50 {
        let r = call(
            &app,
            "GET",
            &format!("/api/v1/admin/game/delivery/queue?service_id={sid}"),
            None,
            &admin.h(),
        )
        .await;
        if let Some(d) = r.body["items"].as_array().and_then(|a| {
            a.iter().find(|d| {
                d["command"]
                    .as_str()
                    .is_some_and(|c| c.starts_with("createaccount"))
            })
        }) {
            row = d.clone();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        row["command"], "createaccount niko {account_password} niko@example.com vip",
        "{row:?}"
    );
    let token = call(
        &app,
        "POST",
        &format!("/api/v1/admin/game/servers/{srv}/agent-token"),
        None,
        &admin.h(),
    )
    .await
    .body["token"]
        .as_str()
        .unwrap()
        .to_string();
    let auth = format!("Bearer {token}");
    let r = call(
        &app,
        "POST",
        "/api/v1/game-agent/poll",
        Some(json!({})),
        &[("authorization", auth.as_str())],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cmds = r.body["commands"].as_array().unwrap();
    assert!(
        cmds.iter()
            .any(|c| c["command"] == format!("createaccount niko {PASSWORD} niko@example.com vip")),
        "{cmds:?}"
    );
    assert!(cmds.iter().all(|c| c.get("service_id").is_none()));
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/game/delivery/queue?service_id={sid}"),
        None,
        &admin.h(),
    )
    .await;
    assert!(!r.body.to_string().contains("s3cret"));

    // The dynamic checkout form never sees (or echoes) a password.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/catalog/products/{pid}/checkout-config"),
        Some(
            json!({"selected": {id_of("account_password"): "x1", id_of("account_username"): "n"}}),
        ),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(
        !r.body["selected"].to_string().contains("x1"),
        "{:?}",
        r.body
    );
    reset_modules(&st).await;
}

async fn apply(app: &axum::Router, h: &[(&str, &str)], base: &str, preset: &str) -> Resp {
    call(
        app,
        "POST",
        &format!("{base}/preset"),
        Some(json!({"preset": preset, "names": {"username": "Nume jucător"}})),
        h,
    )
    .await
}

async fn order_with(
    app: &axum::Router,
    h: &[(&str, &str)],
    pid: &str,
    oid: &str,
    choice: &str,
    f: Value,
) -> Resp {
    call(
        app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: choice}, "fields": f}]})),
        h,
    )
    .await
}
