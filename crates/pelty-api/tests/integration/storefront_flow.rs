use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

fn n(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .or_else(|| v.as_f64().map(|f| f.to_string()))
        .unwrap_or_default()
        .parse::<rust_decimal::Decimal>()
        .map(|d| d.normalize().to_string())
        .unwrap_or_default()
}

fn uuid4() -> String {
    uuid::Uuid::new_v4().to_string()
}

// One test touches global tax settings, so the storefront pricing,
// checkout and recovery flow share it.
#[tokio::test]
async fn public_cart_preview_checkout_status_and_recovery() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let code = format!("SF{}", &uuid::Uuid::new_v4().simple().to_string()[..8]).to_uppercase();
    let r = call(&app, "POST", "/api/v1/admin/coupons", Some(json!({"code": code, "kind": "percent", "value": "20", "product_ids": [pid]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "PUT", "/api/v1/admin/settings/tax", Some(json!({"enabled": true, "inclusive": false, "home_country": "RO", "reverse_charge": false})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    pelty_database::raw_execute(&st.pool, "DELETE FROM tax_rules WHERE country = 'XQ'").await.unwrap();
    let r = call(&app, "POST", "/api/v1/admin/tax-rules", Some(json!({"name": "XQ VAT", "country": "XQ", "rate": "20"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Public: no cookie at all.
    assert_eq!(call(&app, "GET", "/api/v1/catalog", None, &[]).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/store/gateways", None, &[]).await.status, StatusCode::OK);
    let cart = uuid4();
    let url = format!("/api/v1/store/carts/{cart}");
    assert_eq!(call(&app, "GET", &url, None, &[]).await.status, StatusCode::NOT_FOUND);
    // Non-v4 ids are refused (the id is the secret token).
    let r = call(&app, "PUT", "/api/v1/store/carts/00000000-0000-0000-0000-000000000001", Some(json!({"currency": "EUR"})), &[]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let item = json!({"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[1]}});
    let r = call(&app, "PUT", &url, Some(json!({"currency": "EUR", "country": "xq", "items": [item], "coupon": "nope"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let p = &r.body["preview"];
    assert_eq!(p["coupon_error"], json!("error.coupon_invalid"));
    // 12.50 (10 + US 2.50) + 5 setup = 17.50, +20 % XQ VAT = 21.00
    assert_eq!((n(&p["subtotal"]), n(&p["tax"]), n(&p["total"])), ("17.5".into(), "3.5".into(), "21".into()), "{p:?}");

    // Real promo: 20 % off the 12.50 recurring line = 2.50.
    let r = call(&app, "PUT", &url, Some(json!({"currency": "EUR", "country": "XQ", "items": [item], "coupon": code, "email": "Cart@Example.test"})), &[]).await;
    let p = r.body["preview"].clone();
    assert_eq!(p["coupon_error"], Value::Null, "{p:?}");
    assert_eq!((n(&p["discount"]), n(&p["tax"]), n(&p["total"])), ("2.5".into(), "3".into(), "18".into()), "{p:?}");
    assert_eq!(r.body["cart"].get("customer_id"), None, "customer id is not exposed");

    // Regression (/cart coupon "malformed"): exactly the fields the browser
    // sends after cartBody() apply the coupon; echoing the stored cart back
    // (id, order_id, updated_at) is still rejected by deny_unknown_fields.
    let browser = json!({"currency": "EUR", "country": "XQ", "items": [item], "coupon": code, "email": null, "locale": "en"});
    let ok = call(&app, "PUT", &url, Some(browser), &[]).await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(n(&ok.body["preview"]["total"]), "18", "coupon lowers the total from 21 to 18");
    let mut echoed = ok.body["cart"].clone();
    echoed["locale"] = json!("en");
    assert!(call(&app, "PUT", &url, Some(echoed), &[]).await.status.is_client_error());

    // Bad items surface as a cart error, not a 500.
    let bad = call(&app, "PUT", &url, Some(json!({"currency": "EUR", "items": [{"product_id": uuid4(), "cycle": "monthly"}]})), &[]).await;
    assert_eq!(bad.status, StatusCode::OK);
    assert!(bad.body["error"].is_string(), "{:?}", bad.body);
    let r = call(&app, "PUT", &url, Some(json!({"currency": "EUR", "country": "XQ", "items": [item], "coupon": code})), &[]).await;
    assert_eq!(n(&r.body["preview"]["total"]), "18");

    // Checkout needs a session.
    assert_eq!(call(&app, "POST", "/api/v1/client/checkout", Some(json!({"cart_id": cart})), &[]).await.status, StatusCode::UNAUTHORIZED);
    let alice = sess(&app, &st, "customer").await;
    let r = call(&app, "POST", "/api/v1/client/checkout", Some(json!({"cart_id": cart, "country": "XQ"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(n(&r.body["invoice"]["total"]), "18", "invoice matches the preview: {:?}", r.body);
    let order = r.body["order"]["id"].as_str().unwrap().to_string();
    // Cart is frozen once converted.
    let again = call(&app, "POST", "/api/v1/client/checkout", Some(json!({"cart_id": cart})), &alice.h()).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "PUT", &url, Some(json!({"currency": "EUR", "items": []})), &[]).await.status, StatusCode::CONFLICT);

    // Order status page: owner only.
    let r = call(&app, "GET", &format!("/api/v1/client/orders/{order}"), None, &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["order"]["status"], json!("pending"));
    assert_eq!(r.body["invoice"]["status"], json!("unpaid"));
    let bob = sess(&app, &st, "customer").await;
    assert_eq!(call(&app, "GET", &format!("/api/v1/client/orders/{order}"), None, &bob.h()).await.status, StatusCode::NOT_FOUND);

    // Recovery: an idle cart with an email gets exactly one reminder.
    let idle = uuid4();
    let mail = email();
    let r = call(&app, "PUT", &format!("/api/v1/store/carts/{idle}"), Some(json!({"currency": "EUR", "items": [item], "email": mail, "locale": "ro"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let empty = uuid4();
    let mail2 = email();
    call(&app, "PUT", &format!("/api/v1/store/carts/{empty}"), Some(json!({"currency": "EUR", "items": [], "email": mail2})), &[]).await;
    pelty_database::raw_execute(&st.pool, &format!("UPDATE carts SET updated_at = now() - interval '3 hours' WHERE id IN ('{idle}','{empty}','{cart}')")).await.unwrap();
    let n1 = pelty_api::routes::store::run_recovery(&st, 120).await.unwrap();
    assert!(n1 >= 1);
    let got = mails_to(&st, &mail).await;
    assert_eq!(got.len(), 1, "one reminder");
    assert!(got[0].text.contains(&idle), "link restores the cart");
    assert!(mails_to(&st, &mail2).await.is_empty(), "empty carts are skipped");
    pelty_api::routes::store::run_recovery(&st, 120).await.unwrap();
    assert_eq!(mails_to(&st, &mail).await.len(), 1, "never twice");
    // The scheduled job kind runs through the worker too.
    assert!(pelty_api::routes::store::schedule_recovery(&st).await.is_ok());
    pelty_database::raw_execute(&st.pool, "DELETE FROM tax_rules WHERE country = 'XQ'").await.unwrap();
}

#[tokio::test]
async fn contacts_permissions_prefs_history_and_home() {
    let (app, st) = app().await;
    let owner = sess(&app, &st, "customer").await;
    // Home works for a fresh account.
    let h = call(&app, "GET", "/api/v1/client/home?locale=ro", None, &owner.h()).await;
    assert_eq!(h.status, StatusCode::OK, "{:?}", h.body);
    assert!(h.body["services"].is_array() && h.body["kb"].is_array() && h.body["announcements"].is_array());
    assert_eq!(h.body["owner"], json!(true));

    let ce = email();
    let bad = call(&app, "POST", "/api/v1/client/subaccounts", Some(json!({"email": ce, "name": "Ann", "password": "Very-Long-Pass-123", "permissions": ["root"]})), &owner.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/client/subaccounts", Some(json!({"email": ce, "name": "Ann", "password": "Very-Long-Pass-123", "permissions": ["invoices"]})), &owner.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "GET", "/api/v1/client/subaccounts", None, &owner.h()).await.body["items"].as_array().unwrap().len(), 1);

    // Contact signs in with its own login.
    let l = call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": ce, "password": "Very-Long-Pass-123"})), &[]).await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    let cookie = l.cookies.iter().find(|c| c.starts_with("pelty_session=")).unwrap().split(';').next().unwrap().to_string();
    let csrf = l.body["csrf_token"].as_str().unwrap().to_string();
    let ch = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    assert_eq!(call(&app, "GET", "/api/v1/client/invoices", None, &ch).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/services", None, &ch).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/client/tickets", None, &ch).await.status, StatusCode::FORBIDDEN);
    // Never the owner's profile, contacts or orders.
    assert_eq!(call(&app, "GET", "/api/v1/client/profile", None, &ch).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/client/subaccounts", None, &ch).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/client/orders", None, &ch).await.status, StatusCode::FORBIDDEN);
    let a = call(&app, "GET", "/api/v1/client/access", None, &ch).await;
    assert_eq!(a.body, json!({"owner": false, "invoices": true, "services": false, "tickets": false, "orders": false, "domains": false}));
    let hc = call(&app, "GET", "/api/v1/client/home", None, &ch).await;
    assert!(hc.body["services"].is_null() && hc.body["invoices"].is_array(), "{:?}", hc.body);

    // Grant services + tickets.
    let r = call(&app, "PUT", &format!("/api/v1/client/subaccounts/{cid}"), Some(json!({"name": "Ann B", "permissions": ["services", "tickets", "services"]})), &owner.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/services", None, &ch).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/tickets", None, &ch).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/invoices", None, &ch).await.status, StatusCode::FORBIDDEN);
    // Another customer can't touch it.
    let eve = sess(&app, &st, "customer").await;
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/client/subaccounts/{cid}"), None, &eve.h()).await.status, StatusCode::NOT_FOUND);
    // Removal ends access at once.
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/client/subaccounts/{cid}"), None, &owner.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/services", None, &ch).await.status, StatusCode::UNAUTHORIZED);

    // Email preferences round-trip.
    let p = call(&app, "GET", "/api/v1/client/email-preferences", None, &owner.h()).await;
    assert_eq!(p.body, json!({"invoices": true, "services": true, "support": true, "marketing": true}));
    let r = call(&app, "PUT", "/api/v1/client/email-preferences", Some(json!({"invoices": true, "services": true, "support": false, "marketing": false})), &owner.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/email-preferences", None, &owner.h()).await.body["marketing"], json!(false));

    // Login history shows the sign-in and a failed attempt.
    call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": owner.email, "password": "wrong-password-xx"})), &[]).await;
    let hist = call(&app, "GET", "/api/v1/client/login-history", None, &owner.h()).await;
    let acts: Vec<&str> = hist.body["items"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert!(acts.contains(&"auth.login") && acts.contains(&"auth.login_failed"), "{acts:?}");
}

#[tokio::test]
async fn admin_manual_order_detail_notes_and_refund() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let prof = call(&app, "GET", "/api/v1/client/profile", None, &alice.h()).await;
    let cust = prof.body["id"].as_str().unwrap().to_string();
    let item = json!({"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}});

    // Staff only.
    let body = json!({"customer_id": cust, "currency": "EUR", "items": [item]});
    assert_eq!(call(&app, "POST", "/api/v1/admin/orders", Some(body.clone()), &alice.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "POST", "/api/v1/admin/orders", Some(body), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let order = r.body["order"]["id"].as_str().unwrap().to_string();
    let inv = r.body["invoice"]["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["order"]["customer_id"], json!(cust));

    // Refund refuses an unpaid order.
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/orders/{order}/refund"), None, &admin.h()).await.status, StatusCode::CONFLICT);

    // Notes.
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/orders/{order}/notes"), Some(json!({"body": "  "})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/orders/{}/notes", uuid4()), Some(json!({"body": "x"})), &admin.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/orders/{order}/notes"), Some(json!({"body": "Called the customer"})), &admin.h()).await.status, StatusCode::OK);

    let d = call(&app, "GET", &format!("/api/v1/admin/orders/{order}"), None, &admin.h()).await;
    assert_eq!(d.status, StatusCode::OK);
    assert_eq!(d.body["customer"]["id"], json!(cust));
    assert_eq!(d.body["invoice"]["id"], json!(inv));
    assert_eq!(d.body["notes"][0]["body"], json!("Called the customer"));
    assert_eq!(d.body["items"].as_array().unwrap().len(), 1);

    // The detail route also resolves the human order number shown in lists.
    let num = d.body["order"]["number"].as_i64().unwrap();
    let byn = call(&app, "GET", &format!("/api/v1/admin/orders/{num}"), None, &admin.h()).await;
    assert_eq!(byn.status, StatusCode::OK, "{:?}", byn.body);
    assert_eq!(byn.body["order"]["id"], d.body["order"]["id"]);
    assert_eq!(call(&app, "GET", "/api/v1/admin/orders/999999999", None, &admin.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/v1/admin/orders/not-an-id", None, &admin.h()).await.status, StatusCode::NOT_FOUND);

    // Pay in full, accept, then refund via credit note.
    let total = d.body["invoice"]["total"].as_str().unwrap().to_string();
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{inv}/payments"), Some(json!({"amount": total, "idempotency_key": uuid4()})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "POST", &format!("/api/v1/admin/orders/{order}/approve"), None, &admin.h()).await;
    assert!(r.status == StatusCode::OK || r.status == StatusCode::CONFLICT, "{:?}", r.body);
    let r = call(&app, "POST", &format!("/api/v1/admin/orders/{order}/refund"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["credit_note"]["kind"], json!("credit_note"));
    let d = call(&app, "GET", &format!("/api/v1/admin/orders/{order}"), None, &admin.h()).await;
    assert_eq!(d.body["order"]["status"], json!("cancelled"));

    // Manual order with immediate approval creates the services.
    let r = call(&app, "POST", "/api/v1/admin/orders", Some(json!({"customer_id": cust, "items": [item], "approve": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["services"].as_array().unwrap().len(), 1);
}
