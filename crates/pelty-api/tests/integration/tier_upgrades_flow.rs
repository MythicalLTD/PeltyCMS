//! Tier upgrades (module `tier_upgrades`): cumulative credit from owned lower
//! tiers with coupons, timed sales, partial and full refunds, gifts (the
//! recipient's tiers), expired subscriptions, the price-difference mode with
//! a minimum price, other currencies, invoices and in-game removal commands.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

fn dec(v: &Value) -> Decimal {
    v.as_str()
        .map(|s| s.parse().unwrap())
        .or_else(|| v.as_f64().map(|f| Decimal::try_from(f).unwrap()))
        .unwrap_or_else(|| panic!("not a decimal: {v}"))
}
fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

struct Store {
    app: axum::Router,
    admin: Sess,
}

impl Store {
    async fn product(&self, group: &str, name: &str, prices: &[(&str, &str, &str)]) -> String {
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let r = call(
            &self.app,
            "POST",
            "/api/v1/admin/catalog/products",
            Some(json!({"group_id": group, "name": name, "slug": format!("t-{}", &tag[..10])})),
            &self.admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let pid = r.body["id"].as_str().unwrap().to_string();
        let prices: Vec<Value> = prices
            .iter()
            .map(|(c, cy, a)| json!({"currency": c, "cycle": cy, "amount": a}))
            .collect();
        let r = call(
            &self.app,
            "PUT",
            &format!("/api/v1/admin/catalog/products/{pid}/prices"),
            Some(json!({ "prices": prices })),
            &self.admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        pid
    }

    /// Place, pay in full and approve. Returns (order id, invoice json).
    async fn buy(&self, who: &Sess, body: Value) -> (String, Value) {
        let o = call(
            &self.app,
            "POST",
            "/api/v1/client/orders",
            Some(body),
            &who.h(),
        )
        .await;
        assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
        let order = o.body["order"]["id"].as_str().unwrap().to_string();
        let inv = o.body["invoice"]["id"].as_str().unwrap().to_string();
        let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
        if dec(&o.body["invoice"]["total"]) > Decimal::ZERO {
            let r = call(
                &self.app,
                "POST",
                &format!("/api/v1/admin/invoices/{inv}/payments"),
                Some(json!({"amount": total, "idempotency_key": uuid::Uuid::new_v4().to_string()})),
                &self.admin.h(),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        }
        let r = call(
            &self.app,
            "POST",
            &format!("/api/v1/admin/orders/{order}/approve"),
            None,
            &self.admin.h(),
        )
        .await;
        assert!(
            r.status == StatusCode::OK || r.status == StatusCode::CONFLICT,
            "{:?}",
            r.body
        );
        (order, o.body["invoice"].clone())
    }

    async fn credits(&self, who: &Sess, q: &str) -> Value {
        let r = call(
            &self.app,
            "GET",
            &format!("/api/v1/public/tier-upgrades?{q}"),
            None,
            &who.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        r.body["credits"].clone()
    }

    async fn invoice_lines(&self, inv: &str) -> Vec<(String, Decimal)> {
        let r = call(
            &self.app,
            "GET",
            &format!("/api/v1/admin/invoices/{inv}"),
            None,
            &self.admin.h(),
        )
        .await;
        r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["description"].as_str().unwrap().to_string(),
                    dec(&i["unit_price"]),
                )
            })
            .collect()
    }
}

async fn group(app: &axum::Router, admin: &Sess, tag: &str) -> String {
    let r = call(
        app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": format!("Tiers {tag}"), "slug": format!("tiers-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["id"].as_str().unwrap().to_string()
}

/// One test so the module-off checks are not raced by a sibling enabling it.
#[tokio::test]
async fn tier_upgrades() {
    cumulative_credit_through_discounts_refunds_gifts_and_modes().await;
    expired_subscriptions_cancel_old_tier_and_removal_commands().await;
}

async fn cumulative_credit_through_discounts_refunds_gifts_and_modes() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let s = Store {
        app: app.clone(),
        admin,
    };
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let g = group(&app, &s.admin, tag).await;
    let vip = s.product(&g, "VIP", &[("EUR", "one_time", "10.00")]).await;
    let plus = s.product(&g, "VIP+", &[("EUR", "one_time", "20.00")]).await;
    let mvp = s.product(&g, "MVP", &[("EUR", "one_time", "30.00")]).await;
    let one = |p: &str| json!({"product_id": p, "cycle": "one_time"});

    // Module off: no admin API, no credit.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/tier-groups", None, &s.admin.h())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &s.admin, &["tier_upgrades"]).await;
    // Staff only, validated.
    let body = json!({"name": "Ranks", "product_ids": [vip, plus, mvp]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/tier-groups",
            Some(body.clone()),
            &bob.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    for bad in [
        json!({"name": "x", "product_ids": [vip]}),
        json!({"name": "x", "product_ids": [vip, vip]}),
        json!({"name": "x", "credit_mode": "free", "product_ids": [vip, plus]}),
        json!({"name": "x", "min_price": "-1", "product_ids": [vip, plus]}),
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/tier-groups",
            Some(bad),
            &s.admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    }
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/tier-groups",
        Some(body),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let gid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["tiers"][2]["name"], "MVP");
    assert_eq!(r.body["tiers"][2]["rank"], 3);
    // A package is ranked in one group only.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/tier-groups",
        Some(json!({"name": "Other", "product_ids": [vip, mvp]})),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);

    // Nothing owned: no credit, full price.
    assert_eq!(s.credits(&bob, "currency=EUR").await, json!({}));
    let (_, inv) = s
        .buy(&bob, json!({"currency": "EUR", "items": [one(&vip)]}))
        .await;
    assert_eq!(dec(&inv["total"]), d("10"));

    // Owning VIP: VIP+ and MVP get 10 credit; anonymous visitors see none.
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&plus]["amount"]), d("10"));
    assert_eq!(c[&plus]["from_name"], "VIP");
    assert_eq!(dec(&c[&mvp]["amount"]), d("10"));
    assert!(c.get(&vip).is_none());
    let anon = call(
        &app,
        "GET",
        "/api/v1/public/tier-upgrades?currency=EUR",
        None,
        &[],
    )
    .await;
    assert_eq!(anon.body["credits"], json!({}));
    assert_eq!(
        anon.body["groups"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["id"] == gid.as_str())
            .count(),
        1
    );

    // The cart preview shows it before checkout.
    let cart = uuid::Uuid::new_v4();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/store/carts/{cart}"),
        Some(json!({"currency": "EUR", "items": [one(&plus)]})),
        &bob.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(dec(&r.body["preview"]["upgrade_credit"]), d("10"));
    assert_eq!(r.body["preview"]["lines"][0]["upgrade_from"], "VIP");
    assert_eq!(dec(&r.body["preview"]["total"]), d("10"));

    // A coupon applies to the upgrade price: (20 - 10) * 90% = 9.
    let code = format!("UP{}", &tag[..6]).to_uppercase();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/coupons",
        Some(json!({"code": code, "kind": "percent", "value": "10", "product_ids": [plus]})),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let (plus_order, inv) = s
        .buy(
            &bob,
            json!({"currency": "EUR", "items": [one(&plus)], "coupon": code}),
        )
        .await;
    assert_eq!(dec(&inv["total"]), d("9"));
    let lines = s.invoice_lines(inv["id"].as_str().unwrap()).await;
    assert!(
        lines
            .iter()
            .any(|(t, a)| t.starts_with("Upgrade credit: VIP+ (from VIP)") && *a == d("-10")),
        "{lines:?}"
    );
    let (credit, net): (Decimal, Option<Decimal>) = sqlx::query_as(
        "SELECT upgrade_credit, net_amount FROM order_items WHERE order_id = $1::uuid",
    )
    .bind(&plus_order)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!((credit, net), (d("10"), Some(d("9"))));

    // Cumulative: MVP is credited what VIP+ carries (9 paid + 10 credit),
    // not VIP and VIP+ added up (that would count VIP twice).
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("19"));
    assert_eq!(c[&mvp]["from_name"], "VIP+");

    // Partial refund of the VIP+ invoice (half): MVP credit 4.5 + 10.
    let pay: String = sqlx::query_scalar("SELECT p.id::text FROM payments p JOIN orders o ON o.invoice_id = p.invoice_id WHERE o.id = $1::uuid")
        .bind(&plus_order).fetch_one(&st.pool).await.unwrap();
    let r = call(&app, "POST", &format!("/api/v1/admin/payments/{pay}/refunds"), Some(json!({"amount": "4.50", "reversal": true, "idempotency_key": uuid::Uuid::new_v4().to_string()})), &s.admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("14.5"));

    // Two upgrade lines in one cart never spend the same credit twice.
    let cart = uuid::Uuid::new_v4();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/store/carts/{cart}"),
        Some(json!({"currency": "EUR", "items": [one(&mvp), one(&mvp)]})),
        &bob.h(),
    )
    .await;
    assert_eq!(dec(&r.body["preview"]["upgrade_credit"]), d("14.5"));

    // A timed sale applies to the upgrade price: (30 - 14.5) * 50% = 7.75.
    enable(&app, &s.admin, &["timed_sales"]).await;
    let at = |dd: Duration| (OffsetDateTime::now_utc() + dd).format(&Rfc3339).unwrap();
    let r = call(&app, "POST", "/api/v1/admin/sales", Some(json!({"name": format!("Half {tag}"), "percent": "50", "product_ids": [mvp], "starts_at": at(Duration::minutes(-1)), "ends_at": at(Duration::days(1))})), &s.admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let sale = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/store/carts/{cart}"),
        Some(json!({"currency": "EUR", "items": [one(&mvp)]})),
        &bob.h(),
    )
    .await;
    assert_eq!(dec(&r.body["preview"]["sale_discount"]), d("7.75"));
    assert_eq!(dec(&r.body["preview"]["total"]), d("7.75"));
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [one(&mvp)]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(dec(&o.body["invoice"]["total"]), d("7.75"));
    // Cancel that order: its credit is released again.
    let oid = o.body["order"]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{oid}/cancel"),
        Some(json!({})),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/sales/{sale}"),
        None,
        &s.admin.h(),
    )
    .await;
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("14.5"));

    // Refunding the upgrade in full releases it: MVP is credited VIP's 10
    // again, once (not 14.5 + 10).
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{plus_order}/refund"),
        None,
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("10"));
    assert_eq!(c[&mvp]["from_name"], "VIP");

    // Price-difference mode with a minimum price: MVP (30) - VIP list (10)
    // would be 20, but the upgrade costs at least 25, so the credit is 5.
    let r = call(&app, "PUT", &format!("/api/v1/admin/tier-groups/{gid}"), Some(json!({"name": "Ranks", "credit_mode": "difference", "min_price": "25", "product_ids": [vip, plus, mvp]})), &s.admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("5"));
    // VIP+ (20) would cost 10, below the minimum: no credit at all.
    assert!(
        c.get(&plus)
            .map(|v| dec(&v["amount"]))
            .unwrap_or_default()
            .is_zero(),
        "{c}"
    );
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/tier-groups/{gid}"),
        Some(
            json!({"name": "Ranks", "credit_mode": "difference", "product_ids": [vip, plus, mvp]}),
        ),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = s.credits(&bob, "currency=EUR").await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("10"));

    // Gifts use the recipient's tiers, never the buyer's.
    let carol = sess(&app, &st, "customer").await;
    let dave = sess(&app, &st, "customer").await;
    enable(&app, &s.admin, &["game_profiles", "gaming_storefront"]).await;
    let player = format!("Gamer{}", &tag[..6]);
    s.buy(&carol, json!({"currency": "EUR", "items": [{"product_id": vip, "cycle": "one_time", "gift_to": player}]})).await;
    // Dave owns nothing himself...
    assert_eq!(s.credits(&dave, "currency=EUR").await, json!({}));
    // ...but a gift to that player is credited from the player's VIP.
    let c = s
        .credits(&dave, &format!("currency=EUR&gift_to={player}"))
        .await;
    assert_eq!(dec(&c[&plus]["amount"]), d("10"));
    // And Bob's own VIP does not count for a gift to someone else.
    let c = s
        .credits(&bob, &format!("currency=EUR&gift_to=Nobody{}", &tag[..4]))
        .await;
    assert_eq!(c, json!({}));
    let (_, inv) = s.buy(&dave, json!({"currency": "EUR", "items": [{"product_id": plus, "cycle": "one_time", "gift_to": player}]})).await;
    assert_eq!(dec(&inv["total"]), d("10"));
    // When the player links that name, they see the gifted tiers as theirs.
    let erin = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "minecraft_java", "field": "username", "value": player})),
        &erin.h(),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let c = s.credits(&erin, "currency=EUR").await;
    // Difference mode: VIP+ list price.
    assert_eq!(dec(&c[&mvp]["amount"]), d("20"));
    assert_eq!(c[&mvp]["from_name"], "VIP+");

    // Other currencies: the paid value converts at the currency rate.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/tier-groups/{gid}"),
        Some(json!({"name": "Ranks", "product_ids": [vip, plus, mvp]})),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cur = fresh_currency_code(&app, &s.admin, 'T').await;
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/currencies",
        Some(json!({"code": cur, "exponent": 2, "enabled": true, "rate": "2"})),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "PUT", &format!("/api/v1/admin/catalog/products/{mvp}/prices"), Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "30.00"}, {"currency": cur, "cycle": "one_time", "amount": "60.00"}]})), &s.admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = s.credits(&bob, &format!("currency={cur}")).await;
    assert_eq!(dec(&c[&mvp]["amount"]), d("20"));
    let (_, inv) = s
        .buy(&bob, json!({"currency": cur, "items": [one(&mvp)]}))
        .await;
    assert_eq!(dec(&inv["total"]), d("40"));
}

async fn expired_subscriptions_cancel_old_tier_and_removal_commands() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let frank = sess(&app, &st, "customer").await;
    let s = Store {
        app: app.clone(),
        admin,
    };
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    enable(
        &app,
        &s.admin,
        &["tier_upgrades", "game_profiles", "game_delivery"],
    )
    .await;
    let g = group(&app, &s.admin, tag).await;
    let basic = s.product(&g, "Basic", &[("EUR", "monthly", "5.00")]).await;
    let pro = s.product(&g, "Pro", &[("EUR", "monthly", "15.00")]).await;
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/tier-groups",
        Some(json!({"name": "Subs", "revoke_old": true, "product_ids": [basic, pro]})),
        &s.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["cancel_old_recurring"], true);
    // In-game: Basic's expiry command removes the rank.
    let r = call(&app, "POST", "/api/v1/admin/game/servers", Some(json!({"game": "minecraft_java", "name": "mc", "slug": format!("mc-{tag}"), "transport": "agent"})), &s.admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let server = r.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{server}/products"),
        Some(json!({"product_ids": [basic, pro]})),
        &s.admin.h(),
    )
    .await;
    let r = call(&app, "PUT", &format!("/api/v1/admin/game/delivery/products/{basic}/commands"), Some(json!({"commands": [
        {"server_id": server, "trigger": "expiry", "command": "lp user {username} parent remove basic"}
    ]})), &s.admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    call(&app, "POST", "/api/v1/client/game/accounts", Some(json!({"game": "minecraft_java", "field": "username", "value": format!("Frank{}", &tag[..5])})), &frank.h()).await;

    let basic_line =
        json!({"currency": "EUR", "items": [{"product_id": basic, "cycle": "monthly"}]});
    let sid_of = |o: String| {
        let pool = st.pool.clone();
        async move {
            let id: uuid::Uuid = sqlx::query_scalar("SELECT s.id FROM services s JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1::uuid")
                .bind(o).fetch_one(&pool).await.unwrap();
            id
        }
    };
    let (o, _) = s.buy(&frank, basic_line.clone()).await;
    let expired = sid_of(o).await;
    // An expired (terminated) subscription is not owned any more.
    sqlx::query("UPDATE services SET status = 'terminated' WHERE id = $1")
        .bind(expired)
        .execute(&st.pool)
        .await
        .unwrap();
    assert_eq!(s.credits(&frank, "currency=EUR").await, json!({}));
    // Subscribing again makes it count.
    let (o, _) = s.buy(&frank, basic_line).await;
    let basic_sid = sid_of(o).await;
    let c = s.credits(&frank, "currency=EUR").await;
    assert_eq!(dec(&c[&pro]["amount"]), d("5"));

    // Upgrading: the first invoice is 15 - 5; renewals bill the full price,
    // and the old subscription is cancelled so it stops billing.
    let (o, inv) = s
        .buy(
            &frank,
            json!({"currency": "EUR", "items": [{"product_id": pro, "cycle": "monthly"}]}),
        )
        .await;
    assert_eq!(dec(&inv["total"]), d("10"));
    let (st_old,): (String,) = sqlx::query_as("SELECT status FROM services WHERE id = $1")
        .bind(basic_sid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(st_old, "cancelled");
    let (pro_sid, recurring): (uuid::Uuid, Decimal) = sqlx::query_as("SELECT s.id, s.recurring_amount FROM services s JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1::uuid")
        .bind(&o).fetch_one(&st.pool).await.unwrap();
    assert_eq!(recurring, d("15"));
    // The old tier's removal command is queued for the player.
    let mut rows: Vec<(String, String)> = vec![];
    for _ in 0..100 {
        rows = sqlx::query_as("SELECT trigger, command FROM game_deliveries WHERE service_id = $1 AND dedupe_key LIKE $2")
            .bind(basic_sid).bind(format!("%upgrade:{pro_sid}")).fetch_all(&st.pool).await.unwrap();
        if !rows.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        rows,
        vec![(
            "expiry".to_string(),
            format!("lp user Frank{} parent remove basic", &tag[..5])
        )]
    );
    // Owning the top tier: nothing left to upgrade to.
    assert_eq!(s.credits(&frank, "currency=EUR").await, json!({}));
}
