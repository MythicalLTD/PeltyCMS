//! Guest checkout (module `guest_checkout`): buy on a guest-enabled store
//! with a player name / game ID and an email. A shadow account keyed by the
//! email gets the order, invoice and delivery; the buyer claims it with the
//! emailed set-password link. Mojang is mocked; FiveM IDs get a format check;
//! rate limits and the email ban list apply (store ban list: store_rules_flow).
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use pelty_payments::http::MockTransport;
use serde_json::{json, Value};
use std::sync::Arc;

const PW: &str = "Claimed-Pass-123!";

async fn product(app: &axum::Router, admin: &Sess, tag: &str, n: &str) -> String {
    let r = call(
        app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": format!("G {n} {tag}"), "slug": format!("g-{n}-{tag}")})),
        &admin.h(),
    )
    .await;
    let g = r.body["id"].as_str().unwrap().to_string();
    let r = call(app, "POST", "/api/v1/admin/catalog/products", Some(json!({"group_id": g, "name": format!("Rank {n} {tag}"), "slug": format!("rank-{n}-{tag}")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "7.50"}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    pid
}

async fn server(
    app: &axum::Router,
    admin: &Sess,
    game: &str,
    slug: &str,
    guest: bool,
    products: &[&str],
) -> String {
    let r = call(app, "POST", "/api/v1/admin/game/servers", Some(json!({"game": game, "name": slug, "slug": slug, "transport": "agent", "guest_checkout": guest})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["guest_checkout"], guest);
    let id = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{id}/products"),
        Some(json!({"product_ids": products})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    id
}

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

async fn guest(app: &axum::Router, cart: &str, email: &str, identity: Value) -> Resp {
    call(
        app,
        "POST",
        "/api/v1/store/guest-checkout",
        Some(json!({"cart_id": cart, "email": email, "identity": identity})),
        &[],
    )
    .await
}

fn mojang(mock: &MockTransport, name: &str, id: &str) {
    mock.push(200, json!({"id": id, "name": name}).to_string());
}

#[tokio::test]
async fn guest_checkout_shadow_accounts_claims_and_limits() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let mc = product(&app, &admin, tag, "mc").await;
    let fm = product(&app, &admin, tag, "fivem").await;
    let closed = product(&app, &admin, tag, "closed").await;

    // Module off: the API is hidden.
    let c = cart(&app, &mc).await;
    assert_eq!(
        guest(&app, &c, &email(), json!({"username": "Notch"}))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    enable(
        &app,
        &admin,
        &["game_profiles", "game_delivery", "guest_checkout"],
    )
    .await;
    let set = |v: Value| {
        let app = app.clone();
        let h: Vec<(String, String)> = admin
            .h()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let r = call(
                &app,
                "PUT",
                "/api/v1/admin/modules/guest_checkout",
                Some(json!({"enabled": true, "settings": v})),
                &h,
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        }
    };
    set(json!({"ip_per_hour": 1000, "email_per_hour": 1000})).await;
    let srv = server(
        &app,
        &admin,
        "minecraft_java",
        &format!("mc-{tag}"),
        true,
        &[&mc],
    )
    .await;
    server(&app, &admin, "fivem", &format!("fm-{tag}"), true, &[&fm]).await;
    server(
        &app,
        &admin,
        "minecraft_java",
        &format!("closed-{tag}"),
        false,
        &[&closed],
    )
    .await;
    let r = call(&app, "PUT", &format!("/api/v1/admin/game/delivery/products/{mc}/commands"), Some(json!({"commands": [
        {"server_id": srv, "trigger": "purchase", "command": "lp user {username} parent add vip"}
    ]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mock = MockTransport::new();
    st.set_http(Arc::new(mock.clone()));

    // Availability: per store setting, and which identity fields to ask.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/store/guest-checkout/{c}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["available"], true);
    assert_eq!(r.body["game"], "minecraft_java");
    assert_eq!(r.body["fields"], json!(["username", "uuid"]));
    let shut = cart(&app, &closed).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/store/guest-checkout/{shut}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.body["available"], false);
    let r = guest(&app, &shut, &email(), json!({"username": "Notch"})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.guest_not_available");

    // Minecraft: unknown name (Mojang 404), lookup down, bad format.
    mock.push(404, "{}");
    let r = guest(&app, &c, &email(), json!({"username": "NoSuchPlayer1"})).await;
    assert_eq!(
        r.body["error"]["code"], "error.guest_mc_unknown",
        "{:?}",
        r.body
    );
    mock.push(503, "down");
    let r = guest(&app, &c, &email(), json!({"username": "Notch"})).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body["error"]["code"], "error.guest_mc_lookup_failed");
    let before = mock.requests().len();
    let r = guest(&app, &c, &email(), json!({"username": "bad name;op"})).await;
    assert_eq!(r.body["error"]["code"], "error.game_identity_invalid");
    assert_eq!(
        mock.requests().len(),
        before,
        "no lookup for a malformed name"
    );
    let r = guest(&app, &c, "not-an-email", json!({"username": "Notch"})).await;
    assert_eq!(r.body["error"]["code"], "error.invalid_email");

    // New email: shadow account, canonical name + UUID from Mojang, session.
    let buyer = format!("guest-{tag}@pelty.test");
    let name = format!("Pl{}", &tag[..6]);
    let uuid_hex = format!("{}{}", tag, "0123456789abcdef0123456789ab");
    mojang(&mock, &name, &uuid_hex[..32]);
    let r = guest(
        &app,
        &c,
        &buyer.to_uppercase(),
        json!({"username": name.to_lowercase()}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["session"], true);
    let req = mock.requests().pop().unwrap();
    assert_eq!(
        req.url,
        format!(
            "https://api.mojang.com/users/profiles/minecraft/{}",
            name.to_lowercase()
        )
    );
    let cookie = r
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .expect("session cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.body["csrf_token"].as_str().unwrap().to_string();
    let gh = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let order = r.body["order"]["id"].as_str().unwrap().to_string();
    let inv = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = r.body["invoice"]["total"].as_str().unwrap().to_string();
    assert_eq!(r.body["invoice"]["status"], "unpaid");
    // The guest sees their order and invoice like any customer.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/orders/{order}"),
        None,
        &gh,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["id"], inv.as_str());
    let accts = call(&app, "GET", "/api/v1/client/game/accounts", None, &gh).await;
    let mut got: Vec<(String, String)> = accts.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["field"].as_str().unwrap().to_string(),
                a["value"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    got.sort();
    let dashed = format!(
        "{}-{}-{}-{}-{}",
        &uuid_hex[0..8],
        &uuid_hex[8..12],
        &uuid_hex[12..16],
        &uuid_hex[16..20],
        &uuid_hex[20..32]
    );
    assert_eq!(
        got,
        vec![("username".into(), name.clone()), ("uuid".into(), dashed)]
    );
    // The cart is closed; a second try is refused.
    assert_eq!(
        guest(&app, &c, &buyer, json!({"username": name}))
            .await
            .status,
        StatusCode::CONFLICT
    );

    // Pay + approve: delivery runs for the guest's player.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{inv}/payments"),
        Some(json!({"amount": total, "idempotency_key": uuid::Uuid::new_v4().to_string()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mut cmds: Vec<String> = vec![];
    for _ in 0..100 {
        cmds = sqlx::query_scalar("SELECT d.command FROM game_deliveries d JOIN services s ON s.id = d.service_id JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1::uuid")
            .bind(&order).fetch_all(&st.pool).await.unwrap();
        if !cmds.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(cmds, vec![format!("lp user {name} parent add vip")]);

    // The claim mail goes to the buyer; it carries a set-password link.
    let mails = mails_to(&st, &buyer).await;
    let claim = mails
        .iter()
        .find(|m| m.text.contains("reset-password?token="))
        .expect("claim mail");
    assert!(claim.text.contains(&name), "{}", claim.text);

    // Same email again (unclaimed shadow): reused, no session, link mailed;
    // a different player for that email is refused.
    let c2 = cart(&app, &mc).await;
    mojang(&mock, "Someone_Else", "00000000000000000000000000000001");
    let r = guest(&app, &c2, &buyer, json!({"username": "Someone_Else"})).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body["error"]["code"], "error.guest_identity_mismatch");
    mojang(&mock, &name, &uuid_hex[..32]);
    let r = guest(&app, &c2, &buyer, json!({"username": name})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["session"], false);
    assert!(r.cookies.iter().all(|c| !c.starts_with("pelty_session=")));
    let n_orders: i64 = sqlx::query_scalar("SELECT count(*) FROM orders o JOIN customers c ON c.id = o.customer_id WHERE lower(c.email::text) = $1")
        .bind(&buyer).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n_orders, 2);

    // Claim: set a password with the link, then sign in normally. A claimed
    // account cannot be used as a guest any more.
    let mails = mails_to(&st, &buyer).await;
    let tokens: Vec<String> = mails
        .iter()
        .filter(|m| m.text.contains("reset-password?token="))
        .map(token_from)
        .collect();
    let live = tokens.last().unwrap().clone();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/reset",
        Some(json!({"token": live, "password": PW})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": buyer, "password": PW})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c3 = cart(&app, &mc).await;
    let r = guest(&app, &c3, &buyer, json!({"username": name})).await;
    assert_eq!(
        r.body["error"]["code"], "error.guest_account_exists",
        "{:?}",
        r.body
    );
    // A regular account's email is refused the same way (sign in instead).
    let r = guest(&app, &c3, &admin.email, json!({"username": "Notch"})).await;
    assert_eq!(r.body["error"]["code"], "error.guest_account_exists");

    // FiveM: license format check (no Mojang), stored normalized.
    let fc = cart(&app, &fm).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/store/guest-checkout/{fc}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.body["fields"], json!(["username", "license", "steamid"]));
    let r = guest(&app, &fc, &email(), json!({"license": "license:xyz"})).await;
    assert_eq!(r.body["error"]["code"], "error.game_identity_invalid");
    let r = guest(&app, &fc, &email(), json!({})).await;
    assert_eq!(r.body["error"]["code"], "error.guest_identity_required");
    let r = guest(
        &app,
        &fc,
        &email(),
        json!({"discord_id": "123456789012345678"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let before = mock.requests().len();
    let lic = format!("{}{}", "AB".repeat(16), &tag[..8]);
    let fe = email();
    let r = guest(
        &app,
        &fc,
        &fe,
        json!({"license": format!("license:{lic}"), "steamid": "76561198000000001"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(mock.requests().len(), before);
    let v: String = sqlx::query_scalar("SELECT a.value FROM customer_game_accounts a JOIN customers c ON c.id = a.customer_id WHERE lower(c.email::text) = lower($1) AND a.field = 'license'")
        .bind(&fe).fetch_one(&st.pool).await.unwrap();
    assert_eq!(v, lic.to_lowercase());

    // Banned email domains (the store ban list by email / player name is
    // covered in store_rules_flow, which owns that module).
    let fc2 = cart(&app, &fm).await;
    let dom = format!("banned-{tag}.test");
    sqlx::query("INSERT INTO banned_email_domains (domain) VALUES ($1)")
        .bind(&dom)
        .execute(&st.pool)
        .await
        .unwrap();
    let r = guest(
        &app,
        &fc2,
        &format!("x@{dom}"),
        json!({"username": "Fine_Name"}),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.email_banned",
        "{:?}",
        r.body
    );
    sqlx::query("DELETE FROM banned_email_domains WHERE domain = $1")
        .bind(&dom)
        .execute(&st.pool)
        .await
        .unwrap();
    // No shadow account was made for any refused attempt.
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE lower(email::text) IN ($1, $2)")
            .bind(format!("x@{dom}"))
            .bind("not-an-email")
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(n, 0);

    // Signed-in buyers use the normal checkout.
    let bob = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/store/guest-checkout",
        Some(json!({"cart_id": fc2, "email": email(), "identity": {"username": "x_y"}})),
        &bob.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.guest_signed_in");

    // Rate limits: per email, then per IP.
    set(json!({"ip_per_hour": 1000, "email_per_hour": 2})).await;
    let limited = email();
    for _ in 0..2 {
        let r = guest(&app, &fc2, &limited, json!({})).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(
        guest(&app, &fc2, &limited, json!({})).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    set(json!({"ip_per_hour": 1, "email_per_hour": 1000})).await;
    sqlx::query(
        "INSERT INTO guest_checkout_attempts (ip, email) VALUES ('unknown', 'seed@pelty.test')",
    )
    .execute(&st.pool)
    .await
    .unwrap();
    assert_eq!(
        guest(&app, &fc2, &email(), json!({})).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    set(json!({})).await;
}
