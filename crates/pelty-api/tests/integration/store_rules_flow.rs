//! Game store rules (module `gaming_storefront`): per-customer and global
//! purchase limits, required packages, pay what you want, gift purchases,
//! the store ban list, the announcement bar and per-category layouts.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

fn total(inv: &Value) -> String {
    let t = inv["total"].as_str().unwrap_or("?");
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t.to_string()
    }
}

#[tokio::test]
async fn limits_requirements_pwyw_gifts_and_bans() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": format!("Ranks {tag}"), "slug": format!("ranks-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let gid = r.body["id"].as_str().unwrap().to_string();
    let mut p = vec![];
    for (i, amount) in ["10.00", "10.00", "5.00"].iter().enumerate() {
        let r = call(&app, "POST", "/api/v1/admin/catalog/products", Some(json!({"group_id": gid, "name": format!("Pkg {i} {tag}"), "slug": format!("pkg-{i}-{tag}")})), &admin.h()).await;
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
    let (a, b, req) = (p[0].clone(), p[1].clone(), p[2].clone());
    let order = |lines: Vec<Value>| {
        let app = app.clone();
        let h = bob.h();
        async move {
            call(
                &app,
                "POST",
                "/api/v1/client/orders",
                Some(json!({"currency": "EUR", "items": lines})),
                &h,
            )
            .await
        }
    };
    let line = |pid: &str| json!({"product_id": pid, "cycle": "one_time"});
    let code = |r: &Resp| r.body["error"]["code"].as_str().unwrap_or("").to_string();

    // Module off: admin routes 404, gifts and pay-what-you-want refused.
    let rules_url = |pid: &str| format!("/api/v1/admin/storefront/package-rules/{pid}");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/storefront/package-rules",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = order(vec![
        json!({"product_id": req, "cycle": "one_time", "gift_to": "Steve"}),
    ])
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(code(&r), "error.gift_not_allowed");

    enable(&app, &admin, &["gaming_storefront"]).await;
    // Staff only, validated.
    assert_eq!(
        call(
            &app,
            "PUT",
            &rules_url(&a),
            Some(json!({"product_id": a, "user_limit": 1})),
            &bob.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = call(
        &app,
        "PUT",
        &rules_url(&a),
        Some(json!({"product_id": a, "user_limit": 0})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let bad = call(
        &app,
        "PUT",
        &rules_url(&a),
        Some(json!({"product_id": a, "required_product_ids": [a]})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "PUT", &rules_url(&a), Some(json!({"product_id": a, "user_limit": 1, "user_period": "month", "pay_what_you_want": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "PUT",
        &rules_url(&b),
        Some(json!({"product_id": b, "global_limit": 1, "required_product_ids": [req]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Pay what you want: below the price, too many decimals, or on a package
    // without the rule is refused; a higher amount is charged.
    for (pid, amt) in [(&a, "9.99"), (&a, "12.555"), (&b, "12")] {
        let r = order(vec![
            json!({"product_id": pid, "cycle": "one_time", "pay_amount": amt}),
        ])
        .await;
        assert_eq!(
            code(&r),
            "error.pay_amount_invalid",
            "{pid} {amt}: {:?}",
            r.body
        );
    }
    // Cart preview reports the same error, and prices a valid amount.
    let cart = uuid::Uuid::new_v4();
    let r = call(&app, "PUT", &format!("/api/v1/store/carts/{cart}"), Some(json!({"currency": "EUR", "country": "", "items": [{"product_id": a, "cycle": "one_time", "pay_amount": "3"}], "locale": "en"})), &bob.h()).await;
    assert_eq!(r.body["error"], "error.pay_amount_invalid", "{:?}", r.body);
    let r = call(&app, "PUT", &format!("/api/v1/store/carts/{cart}"), Some(json!({"currency": "EUR", "country": "", "items": [{"product_id": a, "cycle": "one_time", "pay_amount": "12.50"}], "locale": "en"})), &bob.h()).await;
    assert_eq!(
        r.body["preview"]["subtotal"]
            .as_str()
            .map(|s| s.parse::<f64>().unwrap()),
        Some(12.5),
        "{:?}",
        r.body
    );
    let r = order(vec![
        json!({"product_id": a, "cycle": "one_time", "pay_amount": "12.50"}),
    ])
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(total(&r.body["invoice"]), "12.5");

    // Per-customer limit: one per month.
    let r = order(vec![line(&a)]).await;
    assert_eq!(code(&r), "error.purchase_limit_reached", "{:?}", r.body);
    let pub_rules = call(
        &app,
        "GET",
        "/api/v1/public/storefront/package-rules",
        None,
        &bob.h(),
    )
    .await;
    let ra = pub_rules.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["product_id"] == a.as_str())
        .unwrap()
        .clone();
    assert_eq!(ra["user_left"], 0);
    assert_eq!(ra["pay_what_you_want"], true);
    let anon = call(
        &app,
        "GET",
        "/api/v1/public/storefront/package-rules",
        None,
        &[],
    )
    .await;
    assert!(anon.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| x["user_left"].is_null()));

    // Required package: refused alone, fine together with the requirement.
    let r = order(vec![line(&b)]).await;
    assert_eq!(code(&r), "error.package_requires", "{:?}", r.body);
    let rb = pub_rules.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["product_id"] == b.as_str())
        .unwrap()
        .clone();
    assert_eq!(rb["requirement_met"], false);
    assert_eq!(rb["required"][0]["id"], req.as_str());
    let r = order(vec![line(&req), line(&b)]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // Global limit of 1 is now used up.
    let r = order(vec![line(&req), line(&b)]).await;
    assert_eq!(code(&r), "error.package_sold_out", "{:?}", r.body);

    // Gifts: recipient name stored on the line and named on the invoice.
    let r = order(vec![
        json!({"product_id": req, "cycle": "one_time", "gift_to": "x\" op"}),
    ])
    .await;
    assert_eq!(code(&r), "error.gift_name_invalid");
    let r = order(vec![
        json!({"product_id": req, "cycle": "one_time", "gift_to": " Steve "}),
    ])
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let oid: uuid::Uuid = r.body["order"]["id"].as_str().unwrap().parse().unwrap();
    let g: Option<String> =
        sqlx::query_scalar("SELECT gift_to FROM order_items WHERE order_id = $1")
            .bind(oid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(g.as_deref(), Some("Steve"));
    let r = call(
        &app,
        "PUT",
        &rules_url(&req),
        Some(json!({"product_id": req, "giftable": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = order(vec![
        json!({"product_id": req, "cycle": "one_time", "gift_to": "Steve"}),
    ])
    .await;
    assert_eq!(code(&r), "error.gift_not_allowed");
    // All defaults again removes the rule.
    call(
        &app,
        "PUT",
        &rules_url(&req),
        Some(json!({"product_id": req})),
        &admin.h(),
    )
    .await;
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM package_rules WHERE product_id = $1::uuid")
            .bind(&req)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(n, 0);

    // Ban list: a banned gift recipient, then the buyer's own email.
    let bans = "/api/v1/admin/storefront/bans";
    assert_eq!(
        call(
            &app,
            "POST",
            bans,
            Some(json!({"kind": "ip", "value": "1.2.3.4"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            bans,
            Some(json!({"kind": "email", "value": "x@y.z"})),
            &bob.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        bans,
        Some(
            json!({"kind": "username", "value": format!("GRIEFER{tag}"), "reason": "chargebacks"}),
        ),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["value"], format!("griefer{tag}"));
    let user_ban = r.body["id"].as_str().unwrap().to_string();
    let r = order(vec![
        json!({"product_id": req, "cycle": "one_time", "gift_to": format!("Griefer{tag}")}),
    ])
    .await;
    assert_eq!(code(&r), "error.store_banned");
    let r = call(
        &app,
        "POST",
        bans,
        Some(json!({"kind": "email", "value": bob.email.to_uppercase()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let ban_id = r.body["id"].as_str().unwrap().to_string();
    let r = order(vec![line(&req)]).await;
    assert_eq!(code(&r), "error.store_banned");
    let list = call(&app, "GET", bans, None, &admin.h()).await;
    assert!(list.body["items"].as_array().unwrap().len() >= 2);
    call(
        &app,
        "DELETE",
        &format!("{bans}/{user_ban}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("{bans}/{ban_id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(order(vec![line(&req)]).await.status, StatusCode::OK);

    // Announcement bar and per-category layout on the public storefront.
    let mut cfg = call(&app, "GET", "/api/v1/admin/storefront", None, &admin.h())
        .await
        .body;
    cfg["announcement"] = json!("Double rank weekend!");
    cfg["announcement_tone"] = json!("sale");
    cfg["announcement_link"] = json!("/store");
    cfg["group_layouts"] = json!({ gid.clone(): "grid" });
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/storefront",
        Some(cfg.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pubsf = call(&app, "GET", "/api/v1/public/storefront", None, &[]).await;
    assert_eq!(pubsf.body["announcement"], "Double rank weekend!");
    assert_eq!(pubsf.body["group_layouts"][&gid], "grid");
    cfg["announcement_tone"] = json!("party");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/storefront",
            Some(cfg),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Module off again: limits no longer apply.
    sqlx::query("UPDATE feature_modules SET enabled = false WHERE id = 'gaming_storefront'")
        .execute(&st.pool)
        .await
        .unwrap();
    assert_eq!(order(vec![line(&a)]).await.status, StatusCode::OK);
}

/// A gift is delivered to the recipient's name, never the buyer's account;
/// commands that need more than a name (e.g. a FiveM license) fail instead of
/// waiting for the buyer to link one.
#[tokio::test]
async fn gift_delivers_to_the_recipient() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    enable(
        &app,
        &admin,
        &["game_profiles", "game_delivery", "gaming_storefront"],
    )
    .await;
    let mut servers = vec![];
    for (game, slug) in [("minecraft_java", "mc"), ("fivem", "rp")] {
        let r = call(&app, "POST", "/api/v1/admin/game/servers", Some(json!({"game": game, "name": slug, "slug": format!("{slug}-{tag}"), "transport": "agent"})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        servers.push(r.body["id"].as_str().unwrap().to_string());
    }
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    for s in &servers {
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/game/servers/{s}/products"),
            Some(json!({"product_ids": [pid]})),
            &admin.h(),
        )
        .await;
    }
    let r = call(&app, "PUT", &format!("/api/v1/admin/game/delivery/products/{pid}/commands"), Some(json!({"commands": [
        {"server_id": servers[0], "trigger": "purchase", "command": "lp user {username} parent add vip"},
        {"server_id": servers[1], "trigger": "purchase", "command": "add_principal identifier.license:{license} group.vip"}
    ]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // The buyer's own linked name must not be used.
    call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "minecraft_java", "field": "username", "value": "Buyer"})),
        &alice.h(),
    )
    .await;
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}, "gift_to": "Notch"}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let sid: uuid::Uuid = ap.body["services"][0].as_str().unwrap().parse().unwrap();
    let mut rows: Vec<(String, String, String, String)> = vec![];
    for _ in 0..100 {
        rows = sqlx::query_as("SELECT player, command, status, last_error FROM game_deliveries WHERE service_id = $1 ORDER BY command")
            .bind(sid).fetch_all(&st.pool).await.unwrap();
        if rows.len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(rows.len(), 2, "{rows:?}");
    let mc = rows.iter().find(|r| r.1.starts_with("lp user")).unwrap();
    assert_eq!(
        (mc.0.as_str(), mc.1.as_str(), mc.2.as_str()),
        ("Notch", "lp user Notch parent add vip", "pending")
    );
    let rp = rows.iter().find(|r| r.1.is_empty()).unwrap();
    assert_eq!(rp.2, "failed");
    assert!(rp.3.contains("gift"), "{rp:?}");
}
