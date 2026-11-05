use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::json;

async fn referred(app: &axum::Router, ck: &str) -> (String, String) {
    let e = email();
    let reg = call(app, "POST", "/api/v1/auth/register", Some(json!({"email": e, "password": "Very-Long-Pass-123", "display_name": "R"})), &[("cookie", ck)]).await;
    assert_eq!(reg.status, StatusCode::CREATED, "{:?}", reg.body);
    let l = call(app, "POST", "/api/v1/auth/login", Some(json!({"email": e, "password": "Very-Long-Pass-123"})), &[]).await;
    let cookie = l.cookies.iter().find(|c| c.starts_with("pelty_session=")).unwrap().split(';').next().unwrap().to_string();
    (cookie, l.body["csrf_token"].as_str().unwrap().to_string())
}

async fn order_and_pay(app: &axum::Router, admin: &Sess, h: &(String, String), pid: &str, oid: &str, ch: &str) -> (String, Decimal) {
    let rh = [("cookie", h.0.as_str()), ("x-csrf-token", h.1.as_str())];
    let o = call(app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch}}]})), &rh).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"), Some(json!({"amount": total, "idempotency_key": format!("affx-{iid}")})), &admin.h()).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    (iid, total.parse().unwrap())
}

fn commission(d: &serde_json::Value, iid: &str) -> Decimal {
    d["commissions"].as_array().unwrap().iter().find(|c| c["invoice_id"] == iid).expect("commission")["amount"].as_str().unwrap().parse().unwrap()
}

#[tokio::test]
async fn product_rates_tiers_and_materials() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();
    // Shared DB: start with no tiers.
    for t in call(&app, "GET", "/api/v1/admin/affiliates/tiers", None, &ah).await.body["items"].as_array().unwrap().clone() {
        call(&app, "DELETE", &format!("/api/v1/admin/affiliates/tiers/{}", t["id"].as_str().unwrap()), None, &ah).await;
    }
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let aff = sess(&app, &st, "customer").await;
    let en = call(&app, "POST", "/api/v1/client/affiliate", None, &aff.h()).await;
    let code = en.body["affiliate"]["code"].as_str().unwrap().to_string();
    let t = call(&app, "GET", &format!("/api/v1/affiliates/r/{code}"), None, &[]).await;
    let ck = t.cookies.iter().find(|c| c.starts_with("pelty_ref=")).unwrap().split(';').next().unwrap().to_string();

    // Validation + permissions.
    let r = call(&app, "PUT", "/api/v1/admin/affiliates/product-rates", Some(json!({"product_id": pid, "kind": "fixed", "value": "3"})), &ah).await;
    assert_eq!(r.body["error"]["code"], "error.invalid_currency");
    let r = call(&app, "PUT", "/api/v1/admin/affiliates/product-rates", Some(json!({"product_id": pid, "kind": "percent", "value": "101"})), &ah).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "GET", "/api/v1/admin/affiliates/product-rates", None, &aff.h()).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Fixed 3.00 EUR per service replaces the 10% share for that product.
    let r = call(&app, "PUT", "/api/v1/admin/affiliates/product-rates", Some(json!({"product_id": pid, "kind": "fixed", "value": "3.00", "currency": "EUR"})), &ah).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let ra = referred(&app, &ck).await;
    let (i1, t1) = order_and_pay(&app, &admin, &ra, &pid, &oid, &ch[0]).await;
    let d = call(&app, "GET", "/api/v1/client/affiliate", None, &aff.h()).await.body;
    // Base share of tax (if any) stays; with no tax this is exactly 3.00.
    let c1 = commission(&d, &i1);
    assert!(c1 >= Decimal::new(300, 2) && c1 <= Decimal::new(300, 2) + t1 / Decimal::from(10), "{c1}");

    // Percent 50 on the product.
    call(&app, "PUT", "/api/v1/admin/affiliates/product-rates", Some(json!({"product_id": pid, "kind": "percent", "value": "50"})), &ah).await;
    let rates = call(&app, "GET", "/api/v1/admin/affiliates/product-rates", None, &ah).await;
    assert!(rates.body["items"].as_array().unwrap().iter().any(|x| x["product_id"] == pid.as_str() && x["kind"] == "percent"));
    let (i2, t2) = order_and_pay(&app, &admin, &ra, &pid, &oid, &ch[0]).await;
    let d = call(&app, "GET", "/api/v1/client/affiliate", None, &aff.h()).await.body;
    let c2 = commission(&d, &i2);
    assert!(c2 > t2 * Decimal::new(4, 1) && c2 <= t2 / Decimal::from(2), "{c2} of {t2}");

    // Tier: 1+ referral → 20%. Product rate removed → base 20% of total.
    let r = call(&app, "POST", "/api/v1/admin/affiliates/tiers", Some(json!({"name": "Silver", "min_referrals": 1, "commission_percent": "20"})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let dup = call(&app, "POST", "/api/v1/admin/affiliates/tiers", Some(json!({"name": "Dup", "min_referrals": 1, "commission_percent": "5"})), &ah).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let r = call(&app, "DELETE", &format!("/api/v1/admin/affiliates/product-rates/{pid}"), None, &ah).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let (i3, t3) = order_and_pay(&app, &admin, &ra, &pid, &oid, &ch[0]).await;
    let d = call(&app, "GET", "/api/v1/client/affiliate", None, &aff.h()).await.body;
    assert_eq!(commission(&d, &i3), (t3 / Decimal::from(5)).round_dp(2));

    // Materials: banner needs an image; inactive ones hidden from affiliates.
    let r = call(&app, "POST", "/api/v1/admin/affiliates/materials", Some(json!({"kind": "banner", "title": "B"})), &ah).await;
    assert_eq!(r.body["error"]["code"], "error.image_url_required");
    let m = call(&app, "POST", "/api/v1/admin/affiliates/materials", Some(json!({"kind": "banner", "title": "Big banner", "image_url": "https://cdn.example.test/b.png"})), &ah).await;
    assert_eq!(m.status, StatusCode::OK, "{:?}", m.body);
    let mid = m.body["id"].as_str().unwrap().to_string();
    let hid = call(&app, "POST", "/api/v1/admin/affiliates/materials", Some(json!({"kind": "text", "title": "Hidden", "body": "x", "active": false})), &ah).await;
    let cm = call(&app, "GET", "/api/v1/client/affiliate/materials", None, &aff.h()).await;
    assert_eq!(cm.status, StatusCode::OK, "{:?}", cm.body);
    assert_eq!(cm.body["effective_percent"].as_str().unwrap().parse::<Decimal>().unwrap(), Decimal::from(20));
    let ids: Vec<_> = cm.body["items"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.contains(&mid) && !ids.contains(&hid.body["id"].as_str().unwrap().to_string()));
    let other = sess(&app, &st, "customer").await;
    assert_eq!(call(&app, "GET", "/api/v1/client/affiliate/materials", None, &other.h()).await.status, StatusCode::NOT_FOUND);
    let u = call(&app, "PUT", &format!("/api/v1/admin/affiliates/materials/{mid}"), Some(json!({"kind": "text", "title": "Now text", "body": "Buy!"})), &ah).await;
    assert_eq!(u.body["title"], "Now text");
    for id in [mid, hid.body["id"].as_str().unwrap().to_string()] {
        assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/affiliates/materials/{id}"), None, &ah).await.status, StatusCode::NO_CONTENT);
    }
    call(&app, "DELETE", &format!("/api/v1/admin/affiliates/tiers/{}", r.body["id"].as_str().unwrap_or_default()), None, &ah).await;
    for t in call(&app, "GET", "/api/v1/admin/affiliates/tiers", None, &ah).await.body["items"].as_array().unwrap().clone() {
        call(&app, "DELETE", &format!("/api/v1/admin/affiliates/tiers/{}", t["id"].as_str().unwrap()), None, &ah).await;
    }
}
