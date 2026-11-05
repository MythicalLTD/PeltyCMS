use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::json;

fn uniq(p: &str) -> String {
    format!("{p}{}", &uuid::Uuid::new_v4().simple().to_string()[..8]).to_uppercase()
}
fn dec(v: &serde_json::Value) -> Decimal {
    v.as_str().unwrap().parse().unwrap()
}
async fn cust(app: &axum::Router, st: &pelty_api::AppState) -> (Sess, String) {
    let c = sess(app, st, "customer").await;
    let r = call(app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &c.h()).await;
    let id = r.body["id"].as_str().unwrap().to_string();
    (c, id)
}

#[tokio::test]
async fn promo_addons_limited_cycles_groups_free_domain_and_order_forms() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let (pid2, oid2, ch2) = seed_product(&app, &admin).await;
    let ad = call(&app, "POST", "/api/v1/admin/catalog/addons", Some(json!({"name": "Backups", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "4.00"}], "product_ids": [pid]})), &admin.h()).await;
    assert_eq!(ad.status, StatusCode::OK, "{:?}", ad.body);
    let addon = ad.body["id"].as_str().unwrap().to_string();
    let (alice, _aid) = cust(&app, &st).await;
    let (bob, _) = cust(&app, &st).await;
    let g = call(&app, "POST", "/api/v1/admin/client-groups", Some(json!({"name": uniq("promo")})), &admin.h()).await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let gid = g.body["group"]["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", &format!("/api/v1/admin/client-groups/{gid}/members"), Some(json!({"email": alice.email})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);

    // 50% recurring for 2 invoices, incl. addons, only for the group.
    let code = uniq("HALF");
    let r = call(&app, "POST", "/api/v1/admin/coupons", Some(json!({"code": code, "kind": "percent", "value": "50", "recurring": true, "recurring_cycles": 2, "applies_addons": true, "client_group_ids": [gid]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!((r.body["recurring_cycles"].as_i64(), r.body["applies_addons"].as_bool()), (Some(2), Some(true)));
    let body = json!({"currency": "EUR", "coupon": code, "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}, "addons": [addon]}]});
    let r = call(&app, "POST", "/api/v1/client/orders", Some(body.clone()), &bob.h()).await;
    assert_eq!(r.body["error"]["code"], "error.coupon_not_eligible", "{:?}", r.body);
    let r = call(&app, "POST", "/api/v1/client/orders", Some(body), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // 10 + 5 setup + 4 addon − 50% of (10 + 4) = 12.
    assert_eq!(dec(&r.body["invoice"]["total"]), Decimal::from(12));
    let o1 = r.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(&app, "POST", &format!("/api/v1/admin/orders/{o1}/approve"), None, &admin.h()).await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    let sid = ap.body["services"][0].as_str().unwrap().to_string();
    let svc = |st: &pelty_api::AppState, sid: String| {
        let pool = st.pool.clone();
        async move {
            sqlx::query_as::<_, (Decimal, Option<i32>, time::Date)>("SELECT recurring_amount, promo_cycles_left, next_due_date FROM services WHERE id = $1::uuid")
                .bind(sid).fetch_one(&pool).await.unwrap()
        }
    };
    let (rec, left, due) = svc(&st, sid.clone()).await;
    assert_eq!((rec, left), (Decimal::from(7), Some(1)));
    // Renewal 1 (2nd invoice) still discounted, then the full price returns.
    let n = pelty_database::services::bill_due_ahead(&st.pool, due, 0, 10000, 7).await.unwrap();
    let ren = n.iter().find(|r| r.service_id.to_string() == sid).expect("renewal 1");
    let total: Decimal = sqlx::query_scalar("SELECT total FROM invoices WHERE id = $1").bind(ren.invoice_id).fetch_one(&st.pool).await.unwrap();
    assert_eq!(total, Decimal::from(7));
    let (rec, left, due) = svc(&st, sid.clone()).await;
    assert_eq!((rec, left), (Decimal::from(14), None));
    let n = pelty_database::services::bill_due_ahead(&st.pool, due, 0, 10000, 7).await.unwrap();
    let ren = n.iter().find(|r| r.service_id.to_string() == sid).expect("renewal 2");
    let total: Decimal = sqlx::query_scalar("SELECT total FROM invoices WHERE id = $1").bind(ren.invoice_id).fetch_one(&st.pool).await.unwrap();
    assert_eq!(total, Decimal::from(14));

    // Free domain: rule on the product, eligible TLD + cycle, once per service.
    let tld = format!("t{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let tld2 = format!("u{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    for t in [&tld, &tld2] {
        let r = call(&app, "PUT", "/api/v1/admin/tlds", Some(json!({"tld": t, "currency": "EUR", "register_price": "10.99", "renew_price": "12.50", "transfer_price": "8.00", "min_years": 1, "max_years": 5, "registrar": "mock", "active": true})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    let fd = format!("/api/v1/admin/products/{pid}/free-domain");
    assert_eq!(call(&app, "PUT", &fd, Some(json!({"free_domain": "always"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", &fd, Some(json!({"free_domain": "once"})), &alice.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "PUT", &fd, Some(json!({"free_domain": "on_renewal", "free_domain_tlds": [format!(".{tld}")], "free_domain_cycles": ["monthly", "annual"]})), &admin.h()).await;
    assert_eq!((r.status, r.body["free_domain_tlds"][0].as_str()), (StatusCode::OK, Some(tld.as_str())));
    let contact = json!({"name": "Ana Pop", "email": "ana@example.test", "country": "RO"});
    let dom = |n: &str, t: &str| format!("free-{}{n}.{t}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": dom("a", &tld2), "kind": "register", "contact": contact, "free_service_id": sid})), &alice.h()).await;
    assert_eq!(r.body["error"]["code"], "error.free_domain_not_eligible", "{:?}", r.body);
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": dom("b", &tld), "kind": "register", "contact": contact, "free_service_id": sid})), &bob.h()).await;
    assert_eq!(r.body["error"]["code"], "error.free_domain_not_eligible", "not bob's service");
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": dom("c", &tld), "kind": "register", "years": 2, "contact": contact, "free_service_id": sid})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(dec(&r.body["total"]), "10.99".parse::<Decimal>().unwrap(), "first year free, second paid");
    let did = r.body["domain"]["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": dom("d", &tld), "kind": "register", "contact": contact, "free_service_id": sid})), &alice.h()).await;
    assert_eq!(r.body["error"]["code"], "error.free_domain_used");
    // on_renewal: renewal while the hosting is active is free.
    pelty_database::raw_execute(&st.pool, &format!("UPDATE domains SET status = 'active', expires_on = current_date + 20 WHERE id = '{did}'")).await.unwrap();
    let d = pelty_database::domains::get(&st.pool, did.parse().unwrap()).await.unwrap().unwrap();
    let t = pelty_database::domains::tld(&st.pool, &tld, "EUR").await.unwrap().unwrap();
    let inv = pelty_database::domains::create_renewal(&st.pool, &d, &t, 1).await.unwrap().expect("renewal");
    assert_eq!(inv.total, Decimal::ZERO);

    // Domain promo: only on its TLD.
    let dcode = uniq("DOM");
    let r = call(&app, "POST", "/api/v1/admin/coupons", Some(json!({"code": dcode, "kind": "percent", "value": "20", "applies_domains": true, "tlds": [tld2]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": dom("e", &tld), "kind": "register", "contact": contact, "coupon": dcode})), &bob.h()).await;
    assert_eq!(r.body["error"]["code"], "error.coupon_not_applicable");
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": dom("f", &tld2), "kind": "register", "years": 2, "contact": contact, "coupon": dcode})), &bob.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // 2 × 10.99 = 21.98 − 20% (4.40) = 17.58.
    assert_eq!(dec(&r.body["total"]), "17.58".parse::<Decimal>().unwrap());
    let r = call(&app, "GET", "/api/v1/admin/coupons", None, &admin.h()).await;
    assert_eq!(r.body["items"].as_array().unwrap().iter().find(|c| c["code"] == dcode.as_str()).unwrap()["uses"], 1);

    // Order form templates.
    let slug = uniq("of").to_lowercase();
    let form = json!({"slug": slug, "name": "VPS form", "layout": "wizard", "product_ids": [pid], "steps": ["product", "configure", "review", "checkout"],
        "fields": [{"key": "purpose", "label": "Purpose", "kind": "select", "options": ["web", "game"], "required": true}], "redirect_url": "/thanks"});
    assert_eq!(call(&app, "POST", "/api/v1/admin/order-forms", Some(form.clone()), &alice.h()).await.status, StatusCode::FORBIDDEN);
    let mut bad = form.clone();
    bad["redirect_url"] = json!("//evil.example");
    assert_eq!(call(&app, "POST", "/api/v1/admin/order-forms", Some(bad), &admin.h()).await.body["error"]["code"], "error.order_form_redirect");
    let mut bad = form.clone();
    bad["steps"] = json!(["product"]);
    assert_eq!(call(&app, "POST", "/api/v1/admin/order-forms", Some(bad), &admin.h()).await.body["error"]["code"], "error.order_form_steps");
    let r = call(&app, "POST", "/api/v1/admin/order-forms", Some(form.clone()), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let fid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "POST", "/api/v1/admin/order-forms", Some(form), &admin.h()).await.status, StatusCode::CONFLICT);
    let r = call(&app, "GET", &format!("/api/v1/store/order-forms/{slug}"), None, &[]).await;
    assert_eq!((r.status, r.body["products"].as_array().map(Vec::len)), (StatusCode::OK, Some(1)), "{:?}", r.body);
    let place = |p: &str, o: &str, c: &str, ans: serde_json::Value| json!({"currency": "EUR", "order_form_id": fid, "form_answers": ans, "items": [{"product_id": p, "cycle": "monthly", "config": {o: c}}]});
    let r = call(&app, "POST", "/api/v1/client/orders", Some(place(&pid, &oid, &ch[0], json!({}))), &bob.h()).await;
    assert_eq!(r.body["error"]["code"], "error.order_form_required");
    let r = call(&app, "POST", "/api/v1/client/orders", Some(place(&pid, &oid, &ch[0], json!({"purpose": "mining"}))), &bob.h()).await;
    assert_eq!(r.body["error"]["code"], "error.order_form_field");
    let r = call(&app, "POST", "/api/v1/client/orders", Some(place(&pid2, &oid2, &ch2[0], json!({"purpose": "web"}))), &bob.h()).await;
    assert_eq!(r.body["error"]["code"], "error.order_form_product");
    let r = call(&app, "POST", "/api/v1/client/orders", Some(place(&pid, &oid, &ch[0], json!({"purpose": "game", "extra": "ignored"}))), &bob.h()).await;
    assert_eq!((r.status, r.body["redirect_url"].as_str()), (StatusCode::OK, Some("/thanks")), "{:?}", r.body);
    let oid_new = r.body["order"]["id"].as_str().unwrap().to_string();
    let ans: serde_json::Value = sqlx::query_scalar("SELECT form_answers FROM orders WHERE id = $1::uuid").bind(&oid_new).fetch_one(&st.pool).await.unwrap();
    assert_eq!(ans, json!({"purpose": "game"}));
    // Per-group template.
    let group: uuid::Uuid = sqlx::query_scalar("SELECT group_id FROM products WHERE id = $1::uuid").bind(&pid).fetch_one(&st.pool).await.unwrap();
    assert_eq!(call(&app, "GET", &format!("/api/v1/store/groups/{group}/order-form"), None, &[]).await.status, StatusCode::NOT_FOUND);
    let r = call(&app, "PUT", &format!("/api/v1/admin/product-groups/{group}/order-form"), Some(json!({"order_form_id": fid})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "GET", &format!("/api/v1/store/groups/{group}/order-form"), None, &[]).await;
    assert_eq!(r.body["form"]["slug"], slug.as_str());
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/order-forms/{fid}"), None, &admin.h()).await.status, StatusCode::OK);
}

#[tokio::test]
async fn plan_change_proration_counts_config_options_and_addons() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (alice, _) = cust(&app, &st).await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let group: String = sqlx::query_scalar("SELECT group_id::text FROM products WHERE id = $1::uuid").bind(&pid).fetch_one(&st.pool).await.unwrap();
    let mid = plan(&app, &admin, &group, "VPS M", "15.00").await;
    let low = plan(&app, &admin, &group, "VPS XS", "12.00").await;
    let ad = call(&app, "POST", "/api/v1/admin/catalog/addons", Some(json!({"name": "Backups", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "4.00"}], "product_ids": [pid]})), &admin.h()).await;
    let addon = ad.body["id"].as_str().unwrap().to_string();
    // 10 + 2.50 (US option) + 4 addon = 16.50 / month.
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[1]}, "addons": [addon]}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let order_id = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(&app, "POST", &format!("/api/v1/admin/orders/{order_id}/approve"), None, &admin.h()).await;
    let sid = ap.body["services"][0].as_str().unwrap().to_string();
    let today = time::OffsetDateTime::now_utc().date();
    let next = today + time::Duration::days(10);
    sqlx::query("UPDATE services SET next_due_date = $2 WHERE id = $1::uuid").bind(&sid).bind(next).execute(&st.pool).await.unwrap();
    let rec: Decimal = sqlx::query_scalar("SELECT recurring_amount FROM services WHERE id = $1::uuid").bind(&sid).fetch_one(&st.pool).await.unwrap();
    assert_eq!(rec, "16.50".parse::<Decimal>().unwrap());

    let lc = call(&app, "GET", &format!("/api/v1/client/services/{sid}/lifecycle"), None, &alice.h()).await;
    assert_eq!(lc.status, StatusCode::OK, "{:?}", lc.body);
    let opts = lc.body["options"].as_array().unwrap();
    let up = opts.iter().find(|o| o["product_id"] == mid.as_str()).unwrap();
    let down = opts.iter().find(|o| o["product_id"] == low.as_str()).unwrap();
    // 15 + 4 addon = 19 > 16.50 → upgrade, even though 15 < 16.50.
    assert_eq!((up["kind"].as_str(), dec(&up["amount"])), (Some("upgrade"), Decimal::from(19)));
    let start = pelty_billing::cycle::sub_months(next, 1);
    let p = pelty_billing::proration::prorate("16.50".parse().unwrap(), Decimal::from(19), start, next, today, 2).unwrap();
    assert_eq!(dec(&up["due_now"]), p.net);
    assert!(p.net > Decimal::ZERO);
    // 12 + 4 = 16 < 16.50 → downgrade at the next due date.
    assert_eq!((down["kind"].as_str(), dec(&down["amount"]), dec(&down["due_now"])), (Some("downgrade"), Decimal::from(16), Decimal::ZERO));

    let r = call(&app, "POST", &format!("/api/v1/client/services/{sid}/change"), Some(json!({"product_id": mid})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // A background renewal run may already have invoiced the next period
    // (due in 10 days); that period is charged the full 2.50 difference.
    let ahead: i64 = sqlx::query_scalar("SELECT count(DISTINCT i.period_start) FROM invoice_items i JOIN invoices v ON v.id = i.invoice_id WHERE i.service_id = $1::uuid AND v.status IN ('unpaid','paid') AND i.period_start >= $2")
        .bind(&sid).bind(next).fetch_one(&st.pool).await.unwrap();
    assert_eq!(dec(&r.body["invoice"]["total"]), p.net + Decimal::from(ahead) * Decimal::new(250, 2));
}

async fn plan(app: &axum::Router, admin: &Sess, group: &str, name: &str, amount: &str) -> String {
    let slug = format!("p{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let p = call(app, "POST", "/api/v1/admin/catalog/products", Some(json!({"group_id": group, "name": name, "slug": slug})), &admin.h()).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let id = p.body["id"].as_str().unwrap().to_string();
    let r = call(app, "PUT", &format!("/api/v1/admin/catalog/products/{id}/prices"), Some(json!({"prices": [{"currency": "EUR", "cycle": "monthly", "amount": amount}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    id
}
