use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_payments::MockTransport;
use serde_json::{json, Value};
use std::sync::Arc;

fn n(v: &Value) -> String {
    v.as_str().unwrap_or_default().parse::<rust_decimal::Decimal>().map(|d| d.normalize().to_string()).unwrap_or_default()
}

async fn customer(app: &axum::Router, st: &pelty_api::AppState, country: &str) -> (Sess, String) {
    let c = sess(app, st, "customer").await;
    let r = call(app, "PUT", "/api/v1/client/profile", Some(json!({"country": country})), &c.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    (c, r.body["id"].as_str().unwrap().to_string())
}

async fn invoice(app: &axum::Router, admin: &Sess, cid: &str, lines: &[(&str, &str)]) -> Value {
    let items: Vec<Value> = lines.iter().map(|(d, p)| json!({"description": d, "quantity": "1", "unit_price": p, "tax_rate": "0"})).collect();
    let r = call(app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true, "items": items})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["invoice"].clone()
}

#[tokio::test]
async fn merge_split_duplicate_notes_and_client_merge() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (alice, aid) = customer(&app, &st, "DE").await;
    let (_bob, bid) = customer(&app, &st, "DE").await;
    let a = invoice(&app, &admin, &aid, &[("Hosting", "10.00")]).await;
    let b = invoice(&app, &admin, &aid, &[("Domain", "2.50"), ("SSL", "7.25")]).await;
    let other = invoice(&app, &admin, &bid, &[("X", "1")]).await;
    let (ida, idb) = (a["id"].as_str().unwrap(), b["id"].as_str().unwrap());

    // Validation: needs two, same customer, staff only.
    assert_eq!(call(&app, "POST", "/api/v1/admin/invoices/merge", Some(json!({"invoice_ids": [ida]})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", "/api/v1/admin/invoices/merge", Some(json!({"invoice_ids": [ida, other["id"]]})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", "/api/v1/admin/invoices/merge", Some(json!({"invoice_ids": [ida, idb]})), &alice.h()).await.status, StatusCode::FORBIDDEN);
    // Client may not merge someone else's invoice.
    assert_eq!(call(&app, "POST", "/api/v1/client/invoices/merge", Some(json!({"invoice_ids": [ida, other["id"]]})), &alice.h()).await.status, StatusCode::NOT_FOUND);

    let ml = call(&app, "GET", &format!("/api/v1/admin/customers/{aid}/mergeable-invoices"), None, &admin.h()).await;
    assert_eq!(ml.body["items"].as_array().unwrap().len(), 2, "{:?}", ml.body);
    let r = call(&app, "POST", "/api/v1/admin/invoices/merge", Some(json!({"invoice_ids": [ida, idb]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let m = r.body["invoice"].clone();
    assert_eq!(n(&m["total"]), "19.75");
    assert_eq!(m["status"], "unpaid");
    let mid = m["id"].as_str().unwrap().to_string();
    let g = call(&app, "GET", &format!("/api/v1/admin/invoices/{ida}"), None, &admin.h()).await;
    assert_eq!(g.body["invoice"]["status"], "void", "{:?}", g.body);
    let nt = call(&app, "GET", &format!("/api/v1/admin/invoices/{idb}/notes"), None, &admin.h()).await;
    assert_eq!(nt.body["merged_into"], mid.as_str());
    // Merged sources can't be merged again.
    assert_eq!(call(&app, "POST", "/api/v1/admin/invoices/merge", Some(json!({"invoice_ids": [ida, mid]})), &admin.h()).await.status, StatusCode::CONFLICT);

    // Split the merged invoice: the 2.50 line off.
    let g = call(&app, "GET", &format!("/api/v1/admin/invoices/{mid}"), None, &admin.h()).await;
    let items = g.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 3, "{:?}", g.body);
    let dom = items.iter().find(|i| i["description"] == "Domain").unwrap_or_else(|| panic!("{items:?}"))["id"].clone();
    let all: Vec<Value> = items.iter().map(|i| i["id"].clone()).collect();
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/invoices/{mid}/split"), Some(json!({"item_ids": all})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/invoices/{mid}/split"), Some(json!({"item_ids": []})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{mid}/split"), Some(json!({"item_ids": [dom]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(n(&r.body["invoices"][0]["total"]), "2.5");
    assert_eq!(n(&r.body["invoices"][1]["total"]), "17.25");
    let s1 = r.body["invoices"][0]["id"].as_str().unwrap().to_string();
    let s2 = r.body["invoices"][1]["id"].as_str().unwrap().to_string();

    // Client mass-pay merge of its two split invoices.
    let r = call(&app, "POST", "/api/v1/client/invoices/merge", Some(json!({"invoice_ids": [s1, s2]})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(n(&r.body["invoice"]["total"]), "19.75");
    let fin = r.body["invoice"]["id"].as_str().unwrap().to_string();

    // Voided source exposes merged_into on the client detail payload.
    let voided = call(&app, "GET", &format!("/api/v1/client/invoices/{s1}"), None, &alice.h()).await;
    assert_eq!(voided.status, StatusCode::OK, "{:?}", voided.body);
    assert_eq!(voided.body["merged_into"], fin);
    assert!(voided.body["payments"].is_array());
    let target = call(&app, "GET", &format!("/api/v1/client/invoices/{fin}"), None, &alice.h()).await;
    assert_eq!(target.status, StatusCode::OK, "{:?}", target.body);
    assert!(target.body["payments"].is_array());
    assert!(target.body.get("split_from").is_some() || target.body["split_from"].is_null());

    // Duplicate -> editable draft with the same total; notes.
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{fin}/duplicate"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = call(&app, "GET", &format!("/api/v1/admin/invoices/{}", r.body["id"].as_str().unwrap()), None, &admin.h()).await;
    assert_eq!(d.body["invoice"]["status"], "draft");
    assert_eq!(n(&d.body["invoice"]["total"]), "19.75");
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/invoices/{fin}/notes"), Some(json!({"admin_notes": "called client", "client_notes": "Thanks!"})), &admin.h()).await.status, StatusCode::OK);
    let nt = call(&app, "GET", &format!("/api/v1/admin/invoices/{fin}/notes"), None, &admin.h()).await;
    assert_eq!(nt.body["admin_notes"], "called client");
    assert_eq!(nt.body["client_notes"], "Thanks!");
}

#[tokio::test]
async fn merge_keeps_service_period_claim() {
    // Copies don't re-claim the service period; the original line keeps it.
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (_c, cid) = customer(&app, &st, "DE").await;
    let a = invoice(&app, &admin, &cid, &[("A", "1")]).await;
    let b = invoice(&app, &admin, &cid, &[("B", "2")]).await;
    let r = call(&app, "POST", "/api/v1/admin/invoices/merge", Some(json!({"invoice_ids": [a["id"], b["id"]]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let copies: i64 = sqlx::query_scalar("SELECT count(*) FROM invoice_items WHERE invoice_id = $1::uuid AND copied_from IS NOT NULL")
        .bind(r.body["invoice"]["id"].as_str().unwrap()).fetch_one(&st.pool).await.unwrap();
    assert_eq!(copies, 2);
}

#[tokio::test]
async fn gift_cards_issue_redeem_once() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (alice, aid) = customer(&app, &st, "DE").await;
    let (bob, _) = customer(&app, &st, "DE").await;
    assert_eq!(call(&app, "POST", "/api/v1/admin/gift-cards", Some(json!({"currency": "EUR", "amount": "25"})), &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", "/api/v1/admin/gift-cards", Some(json!({"currency": "EUR", "amount": "0.001"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/gift-cards", Some(json!({"count": 2, "currency": "EUR", "amount": "25.00", "note": "promo"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let code = r.body["items"][0]["code"].as_str().unwrap().to_string();
    let second_id = r.body["items"][1]["card"]["id"].as_str().unwrap().to_string();
    let second = r.body["items"][1]["code"].as_str().unwrap().to_string();
    // Plaintext code is never listed.
    let l = call(&app, "GET", "/api/v1/admin/gift-cards", None, &admin.h()).await;
    assert!(!l.body.to_string().contains(&code));

    let r = call(&app, "POST", "/api/v1/client/gift-cards/redeem", Some(json!({"code": code.to_lowercase()})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(n(&r.body["amount"]), "25");
    assert_eq!(call(&app, "POST", "/api/v1/client/gift-cards/redeem", Some(json!({"code": code})), &bob.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let c = call(&app, "GET", "/api/v1/client/credit", None, &alice.h()).await;
    let eur = c.body["balances"].as_array().unwrap().iter().find(|b| b["currency"] == "EUR").unwrap().clone();
    assert_eq!(n(&eur["amount"]), "25", "{:?}", c.body);
    let _ = aid;
    // Disabled card can't be redeemed.
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/gift-cards/{second_id}/disable"), None, &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "POST", "/api/v1/client/gift-cards/redeem", Some(json!({"code": second})), &bob.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn exchange_rates_update_from_mock_feed() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let code = fresh_currency_code(&app, &admin, 'W').await;
    let r = call(&app, "POST", "/api/v1/admin/currencies", Some(json!({"code": code, "exponent": 2, "enabled": true, "rate": "1", "decimal_sep": ",", "thousands_sep": "."})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", "/api/v1/admin/currencies", Some(json!({"code": "WZZ", "exponent": 2, "enabled": true, "rate": "1", "decimal_sep": ",", "thousands_sep": ","})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/currencies/rates/settings", Some(json!({"enabled": false, "source": "json", "url": "http://insecure", "markup_percent": "0"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/currencies/rates/settings", Some(json!({"enabled": false, "source": "json", "url": "https://rates.example/latest", "markup_percent": "2"})), &admin.h()).await.status, StatusCode::OK);

    // Offline transport -> 409 and the error is recorded.
    assert_eq!(call(&app, "POST", "/api/v1/admin/currencies/rates/update", None, &admin.h()).await.status, StatusCode::CONFLICT);
    let s = call(&app, "GET", "/api/v1/admin/currencies/rates/settings", None, &admin.h()).await;
    assert!(s.body["last_error"].is_string(), "{:?}", s.body);

    let mock = MockTransport::new();
    mock.push(200, format!(r#"{{"base":"USD","rates":{{"EUR":"0.8","{code}":"4"}}}}"#));
    st.set_http(Arc::new(mock.clone()));
    let r = call(&app, "POST", "/api/v1/admin/currencies/rates/update", None, &admin.h()).await;
    st.set_http(Arc::new(pelty_payments::NoTransport));
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["updated"].as_array().unwrap().iter().any(|c| c == code.as_str()));
    assert_eq!(mock.requests()[0].url, "https://rates.example/latest");
    let l = call(&app, "GET", "/api/v1/admin/currencies", None, &admin.h()).await;
    let me = l.body["items"].as_array().unwrap().iter().find(|c| c["code"] == code.as_str()).unwrap().clone();
    // 4 / 0.8 = 5, +2 % markup = 5.1
    assert_eq!(n(&me["rate"]), "5.1");
    assert_eq!(me["decimal_sep"], ",");
    let h = call(&app, "GET", "/api/v1/admin/currencies/rates/history", None, &admin.h()).await;
    assert!(h.body["items"].as_array().unwrap().iter().any(|x| x["code"] == code.as_str()));
    // Full ISO list is present (disabled by default).
    let has: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM currencies WHERE code = 'ISK' AND exponent = 0)").fetch_one(&st.pool).await.unwrap();
    assert!(has);
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/currencies/{code}"), Some(json!({"exponent": 2, "enabled": false, "rate": "5.1", "auto_update": false})), &admin.h()).await.status, StatusCode::OK);
}

#[tokio::test]
async fn vies_verification_gates_reverse_charge() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (alice, aid) = customer(&app, &st, "DE").await;
    // No VAT number yet.
    assert_eq!(call(&app, "POST", "/api/v1/client/vat/verify", None, &alice.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    pelty_database::raw_execute(&st.pool, &format!("UPDATE customers SET vat_number = 'DE136695976' WHERE id = '{aid}'")).await.unwrap();
    // Transport down -> 409, status unchanged.
    assert_eq!(call(&app, "POST", "/api/v1/client/vat/verify", None, &alice.h()).await.status, StatusCode::CONFLICT);
    let mock = MockTransport::new();
    mock.push(200, r#"{"isValid":true,"name":"ACME GMBH","address":"Berlin"}"#);
    mock.push(200, r#"{"isValid":false,"name":"---"}"#);
    st.set_http(Arc::new(mock.clone()));
    let r = call(&app, "POST", &format!("/api/v1/admin/customers/{aid}/vat/verify"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["valid"], true);
    assert_eq!(r.body["name"], "ACME GMBH");
    assert!(mock.requests()[0].url.ends_with("/ms/DE/vat/136695976"));
    let (status, num): (String, String) = sqlx::query_as("SELECT vat_status, vat_checked_num FROM customers WHERE id = $1::uuid").bind(&aid).fetch_one(&st.pool).await.unwrap();
    assert_eq!((status.as_str(), num.as_str()), ("valid", "DE136695976"));
    let r = call(&app, "POST", "/api/v1/client/vat/verify", None, &alice.h()).await;
    st.set_http(Arc::new(pelty_payments::NoTransport));
    assert_eq!(r.body["valid"], false);
    let status: String = sqlx::query_scalar("SELECT vat_status FROM customers WHERE id = $1::uuid").bind(&aid).fetch_one(&st.pool).await.unwrap();
    assert_eq!(status, "invalid");
}

#[tokio::test]
async fn transactions_list_filters_and_totals() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (alice, aid) = customer(&app, &st, "DE").await;
    let a = invoice(&app, &admin, &aid, &[("Hosting", "10.00")]).await;
    let b = invoice(&app, &admin, &aid, &[("Domain", "4.50")]).await;
    for (inv, amt, key) in [(&a, "10.00", "tx-a"), (&b, "4.50", "tx-b")] {
        let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{}/payments", inv["id"].as_str().unwrap()), Some(json!({"amount": amt, "reference": format!("REF-{key}-{}", &aid[..8]), "idempotency_key": format!("{key}-{}", uuid::Uuid::new_v4())})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    let r = call(&app, "GET", &format!("/api/v1/admin/transactions?customer_id={aid}"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["total"], 2, "{:?}", r.body);
    let tot = &r.body["totals"][0];
    assert_eq!((tot["currency"].as_str(), tot["status"].as_str(), n(&tot["amount"])), (Some("EUR"), Some("succeeded"), "14.5".into()), "{:?}", r.body);
    let num = b["number"].as_str().unwrap();
    let r = call(&app, "GET", &format!("/api/v1/admin/transactions?customer_id={aid}&q={num}"), None, &admin.h()).await;
    assert_eq!(r.body["total"], 1, "{:?}", r.body);
    assert_eq!(n(&r.body["items"][0]["amount"]), "4.5");
    let r = call(&app, "GET", &format!("/api/v1/admin/transactions?customer_id={aid}&status=failed"), None, &admin.h()).await;
    assert_eq!(r.body["total"], 0);
    let r = call(&app, "GET", &format!("/api/v1/admin/transactions?customer_id={aid}&from=2999-01-01"), None, &admin.h()).await;
    assert_eq!(r.body["total"], 0);
    assert_eq!(call(&app, "GET", "/api/v1/admin/transactions?status=bogus", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/transactions?from=31-12-2020", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/transactions", None, &alice.h()).await.status, StatusCode::FORBIDDEN);
}
