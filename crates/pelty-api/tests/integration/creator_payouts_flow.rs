//! Live creator payouts against scripted Stripe / PayPal answers
//! (`MockTransport`, no network): Stripe Connect onboarding, signed
//! webhooks, transfers, failed transfers, transfer reversal after a refund,
//! PayPal batches with polling and webhooks, scheduled payouts without
//! approval, method locks, galleries and version-update mails.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use pelty_api::routes::gaming::payouts::{env, sign_stripe};
use pelty_payments::http::MockTransport;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;

const STRIPE_TEST_KEY: &str = "sk_test_mockplatform";
const STRIPE_HOOK: &str = "whsec_mockhook";
const PAYPAL_APP: &str = "mock-paypal-app";
const PAYPAL_APP_PASS: &str = "mock-paypal-pass";

fn dec(v: &Value) -> Decimal {
    v.as_str().unwrap().parse().unwrap()
}

async fn eur(app: &axum::Router, s: &Sess) -> Decimal {
    let d = call(app, "GET", "/api/v1/client/creator", None, &s.h()).await;
    d.body["balances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["currency"] == "EUR")
        .map_or(Decimal::ZERO, |b| dec(&b["available"]))
}

async fn wait_eur(app: &axum::Router, s: &Sess, want: Decimal) {
    for _ in 0..200 {
        if eur(app, s).await == want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("balance {} != {want}", eur(app, s).await);
}

async fn settings(app: &axum::Router, admin: &Sess, v: Value) {
    let r = call(
        app,
        "PUT",
        "/api/v1/admin/modules/creator_marketplace",
        Some(json!({"enabled": true, "settings": v})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

/// Bob buys the listing (50.00 EUR, 20% commission → 40.00 to Carol) and the
/// invoice is paid by staff.
async fn sale(app: &axum::Router, admin: &Sess, bob: &Sess, pid: &str) -> String {
    let o = call(
        app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(
        app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("cp-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    o.body["order"]["id"].as_str().unwrap().to_string()
}

async fn stripe_hook(app: &axum::Router, ev: &Value, sig: Option<&str>) -> Resp {
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    let good = sign_stripe(STRIPE_HOOK, ev.to_string().as_bytes(), ts);
    let sig = sig.map_or(good, str::to_string);
    call(
        app,
        "POST",
        "/api/v1/webhooks/creator-payouts/stripe",
        Some(ev.clone()),
        &[("stripe-signature", sig.as_str())],
    )
    .await
}

fn paypal_headers() -> [(&'static str, &'static str); 5] {
    [
        ("paypal-auth-algo", "SHA256withRSA"),
        (
            "paypal-cert-url",
            "https://api.sandbox.paypal.com/v1/notifications/certs/CERT-mock",
        ),
        ("paypal-transmission-id", "tid-1"),
        ("paypal-transmission-sig", "c2lnbmF0dXJl"),
        ("paypal-transmission-time", "2026-11-10T14:00:00Z"),
    ]
}

fn body_has(t: &MockTransport, url_end: &str, needle: &str) -> bool {
    t.requests()
        .iter()
        .any(|r| r.url.contains(url_end) && r.body.contains(needle))
}

/// Clears the provider env vars when the test ends, even on a panic.
struct EnvGuard;
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for k in [
            env::STRIPE_KEY,
            env::STRIPE_WEBHOOK,
            env::PAYPAL_ID,
            env::PAYPAL_SECRET,
            env::PAYPAL_WEBHOOK,
        ] {
            std::env::remove_var(k);
        }
    }
}

#[tokio::test]
async fn stripe_connect_paypal_payouts_and_reversals() {
    let _serial = CREATOR_PAYOUTS.lock().await;
    let _env = EnvGuard;
    let (app, st) = app().await;
    reset_modules(&st).await;
    let mock = MockTransport::new();
    st.set_http(Arc::new(mock.clone()));
    std::env::set_var(env::STRIPE_KEY, STRIPE_TEST_KEY);
    std::env::set_var(env::STRIPE_WEBHOOK, STRIPE_HOOK);
    std::env::set_var(env::PAYPAL_ID, PAYPAL_APP);
    std::env::set_var(env::PAYPAL_SECRET, PAYPAL_APP_PASS);
    std::env::set_var(env::PAYPAL_WEBHOOK, "WH-mock");
    std::env::remove_var(env::PAYPAL_MODE);
    std::env::remove_var(env::LIVE);

    let admin = sess(&app, &st, "admin").await;
    let carol = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    // A fresh connected-account id per run: the test DB keeps creators from earlier runs.
    let acct = format!("acct_1Mock{tag}");
    let tr_id = format!("tr_1Mock{tag}");
    let trr_id = format!("trr_1Mock{tag}");
    let (batch1, batch2) = (format!("PB_MOCK1{tag}"), format!("PB_MOCK2{tag}"));
    let (item_a, item_b) = (format!("PI_A{tag}"), format!("PI_B{tag}"));
    enable(&app, &admin, &["digital_products"]).await;
    settings(&app, &admin, json!({"mode": "multi_vendor", "commission_percent": "20", "hold_days": 0, "min_payout": "5"})).await;

    // Overview: providers configured in test mode, secrets never echoed.
    let o = call(
        &app,
        "GET",
        "/api/v1/admin/creator-payouts/overview",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(o.body["stripe"]["configured"], true);
    assert_eq!(o.body["stripe"]["test_mode"], true);
    assert_eq!(o.body["paypal"]["test_mode"], true);
    assert!(o.body["stripe"]["webhook_url"]
        .as_str()
        .unwrap()
        .ends_with("/api/v1/webhooks/creator-payouts/stripe"));
    assert!(
        !o.body.to_string().contains(STRIPE_TEST_KEY)
            && !o.body.to_string().contains(PAYPAL_APP_PASS)
    );

    // Creator + approved listing.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator",
        Some(json!({"slug": format!("pay-{tag}"), "display_name": "Pay Dev"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cid = r.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/creators/{cid}/status"),
        Some(json!({"status": "approved"})),
        &admin.h(),
    )
    .await;
    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Scripts", "slug": format!("scr-{tag}")})),
        &admin.h(),
    )
    .await;
    let l = call(&app, "POST", "/api/v1/client/creator/listings", Some(json!({"group_id": g.body["id"], "name": "Payout Script", "slug": format!("payout-script-{tag}"), "description": "x", "currency": "EUR", "cycle": "one_time", "price": "50.00"})), &carol.h()).await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    let pid = l.body["product_id"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-listings/{pid}/review"),
        Some(json!({"approve": true})),
        &admin.h(),
    )
    .await;

    // Gallery: owner only, validated, shown on the resource page.
    let imgs = json!({"images": [{"url": "/uploads/a.png", "caption": "Menu"}, {"url": "https://cdn.example/b.png"}]});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/client/creator/listings/{pid}/gallery"),
            Some(imgs.clone()),
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let bad = json!({"images": [{"url": "javascript:alert(1)"}]});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/client/creator/listings/{pid}/gallery"),
            Some(bad),
            &carol.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/creator/listings/{pid}/gallery"),
        Some(imgs),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let page = call(
        &app,
        "GET",
        &format!("/api/v1/public/marketplace/listings/{pid}"),
        None,
        &[],
    )
    .await;
    assert_eq!(
        page.body["gallery"].as_array().unwrap().len(),
        2,
        "{:?}",
        page.body
    );
    assert_eq!(page.body["gallery"][0]["caption"], "Menu");

    let order = sale(&app, &admin, &bob, &pid).await;
    wait_eur(&app, &carol, Decimal::from(40)).await;

    // ---- Stripe Connect onboarding.
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator/payout-settings",
        Some(json!({"method": "stripe_connect"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    mock.push(200, json!({"id": acct}).to_string());
    mock.push(
        200,
        json!({"url": "https://connect.stripe.com/setup/e/acct_1Mock/abc"}).to_string(),
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator/stripe/onboard",
        None,
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["url"],
        "https://connect.stripe.com/setup/e/acct_1Mock/abc"
    );
    let reqs = mock.requests();
    let created = reqs
        .iter()
        .find(|x| x.url.ends_with("/v1/accounts"))
        .expect("account created");
    assert_eq!(
        created.header("Authorization"),
        Some(format!("Bearer {STRIPE_TEST_KEY}").as_str())
    );
    assert_eq!(
        created.header("Idempotency-Key"),
        Some(format!("pelty-acct-{cid}").as_str())
    );
    assert!(
        created.body.contains("type=express")
            && created
                .body
                .contains("capabilities%5Btransfers%5D%5Brequested%5D=true"),
        "{}",
        created.body
    );
    assert!(
        body_has(&mock, "/v1/account_links", "return_url=")
            && body_has(&mock, "/v1/account_links", "stripe%3Dreturn")
    );
    let d = call(&app, "GET", "/api/v1/client/creator", None, &carol.h()).await;
    assert_eq!(
        d.body["payout"]["stripe"]["status"], "onboarding",
        "{:?}",
        d.body["payout"]
    );

    // Not enabled yet: sending is refused and nothing is debited twice.
    let po = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(json!({"currency": "EUR", "amount": "30"})),
        &carol.h(),
    )
    .await;
    assert_eq!(po.status, StatusCode::OK, "{:?}", po.body);
    let po = po.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po}/send"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.stripe_not_ready",
        "{:?}",
        r.body
    );

    // Signed account.updated webhook: bad signature refused, then enabled, duplicates ignored.
    let ev = json!({"id": format!("evt_acct_{tag}"), "type": "account.updated", "data": {"object": {"id": acct, "capabilities": {"transfers": "active"}, "payouts_enabled": true, "details_submitted": true}}});
    assert_eq!(
        stripe_hook(&app, &ev, Some("t=1,v1=00")).await.status,
        StatusCode::BAD_REQUEST
    );
    let r = stripe_hook(&app, &ev, None).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(stripe_hook(&app, &ev, None).await.body["duplicate"], true);
    let d = call(&app, "GET", "/api/v1/client/creator", None, &carol.h()).await;
    assert_eq!(d.body["payout"]["stripe"]["status"], "enabled");

    // Transfer from the platform balance.
    mock.push(200, json!({"id": tr_id, "object": "transfer"}).to_string());
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po}/send"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "paid");
    assert_eq!(r.body["provider_ref"], tr_id.as_str());
    let tr = mock
        .requests()
        .into_iter()
        .rfind(|x| x.url.ends_with("/v1/transfers"))
        .unwrap();
    assert!(
        tr.body.contains("amount=3000")
            && tr.body.contains("currency=eur")
            && tr.body.contains(&format!("destination={acct}")),
        "{}",
        tr.body
    );
    assert_eq!(
        tr.header("Idempotency-Key"),
        Some(format!("pelty-payout-{po}").as_str())
    );
    wait_eur(&app, &carol, Decimal::from(10)).await;

    // Stripe refuses a transfer: payout fails, money returns to the balance.
    let po2 = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(json!({"currency": "EUR", "amount": "5"})),
        &carol.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    mock.push(
        400,
        json!({"error": {"message": "Insufficient funds in Stripe account"}}).to_string(),
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/creator-payouts/{po2}/send"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.payout_provider_rejected",
        "{:?}",
        r.body
    );
    let list = call(
        &app,
        "GET",
        "/api/v1/client/creator/payouts",
        None,
        &carol.h(),
    )
    .await;
    let failed = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == po2.as_str())
        .unwrap()
        .clone();
    assert_eq!(failed["status"], "failed");
    assert!(failed["note"]
        .as_str()
        .unwrap()
        .contains("Insufficient funds"));
    wait_eur(&app, &carol, Decimal::from(10)).await;

    // Amounts finer than the currency allows are refused.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(json!({"currency": "EUR", "amount": "5.001"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_amount");

    // Refund after the payout: balance 10 - 40 = -30 → reverse 30 from tr_1Mock.
    mock.push(200, json!({"id": trr_id, "amount": 3000}).to_string());
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order}/refund"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    wait_eur(&app, &carol, Decimal::ZERO).await;
    let rev = mock
        .requests()
        .into_iter()
        .rfind(|x| x.url.contains(&format!("/v1/transfers/{tr_id}/reversals")))
        .expect("reversal sent");
    assert!(rev.body.contains("amount=3000"));
    let ledger = call(
        &app,
        "GET",
        "/api/v1/client/creator/ledger",
        None,
        &carol.h(),
    )
    .await;
    assert!(ledger.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "transfer_reversal" && dec(&e["amount"]) == Decimal::from(30)));
    // The same reversal reported by webhook is not booked twice.
    let ev = json!({"id": format!("evt_rev_{tag}"), "type": "transfer.reversed", "data": {"object": {"id": tr_id, "currency": "eur", "metadata": {"payout_id": po}, "reversals": {"data": [{"id": trr_id, "amount": 3000}]}}}});
    assert_eq!(stripe_hook(&app, &ev, None).await.status, StatusCode::OK);
    assert_eq!(eur(&app, &carol).await, Decimal::ZERO);

    // ---- PayPal Payouts.
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator/payout-settings",
        Some(json!({"method": "paypal"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.paypal_email_required");
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator/payout-settings",
        Some(json!({"method": "paypal", "paypal_email": "carol-pp@example.com"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    sale(&app, &admin, &bob, &pid).await;
    wait_eur(&app, &carol, Decimal::from(40)).await;
    let pa = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(json!({"currency": "EUR", "amount": "20"})),
        &carol.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let pb = call(
        &app,
        "POST",
        "/api/v1/client/creator/payouts",
        Some(json!({"currency": "EUR", "amount": "10"})),
        &carol.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    mock.push(
        200,
        json!({"access_token": "A21mock", "token_type": "Bearer"}).to_string(),
    );
    mock.push(
        201,
        json!({"batch_header": {"payout_batch_id": batch1, "batch_status": "PENDING"}}).to_string(),
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/creator-payouts/send-batch",
        Some(json!({"ids": [pa, pb]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["sent"].as_array().unwrap().len(), 2, "{:?}", r.body);
    let batch = mock
        .requests()
        .into_iter()
        .rfind(|x| x.url.ends_with("/v1/payments/payouts"))
        .unwrap();
    assert!(
        batch.url.starts_with("https://api-m.sandbox.paypal.com"),
        "sandbox by default"
    );
    let b: Value = serde_json::from_str(&batch.body).unwrap();
    assert_eq!(
        b["items"].as_array().unwrap().len(),
        2,
        "one batch for both"
    );
    assert!(b["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["amount"]["value"] == "20.00"
            && i["receiver"] == "carol-pp@example.com"
            && i["sender_item_id"] == pa.as_str()));
    assert_eq!(
        batch.header("PayPal-Request-Id"),
        b["sender_batch_header"]["sender_batch_id"].as_str()
    );
    let tok = mock
        .requests()
        .into_iter()
        .rfind(|x| x.url.ends_with("/v1/oauth2/token"))
        .unwrap();
    assert!(tok.header("Authorization").unwrap().starts_with("Basic "));

    // Poll: item A succeeded, item B still pending.
    mock.push(200, json!({"access_token": "A21mock"}).to_string());
    mock.push(200, json!({"batch_header": {"payout_batch_id": batch1}, "items": [
        {"payout_item_id": item_a, "transaction_status": "SUCCESS", "payout_item": {"sender_item_id": pa}},
        {"payout_item_id": item_b, "transaction_status": "UNCLAIMED", "payout_item": {"sender_item_id": pb}}]}).to_string());
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/creator-payouts/run",
        Some(json!({})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["synced"], 1, "{:?}", r.body);
    let list = call(
        &app,
        "GET",
        "/api/v1/client/creator/payouts",
        None,
        &carol.h(),
    )
    .await;
    let st_of = |id: &str| {
        list.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()["status"]
            .clone()
    };
    assert_eq!(st_of(&pa), "paid");
    assert_eq!(st_of(&pb), "processing");

    // Webhook: forged deliveries are refused; a verified RETURNED item fails and credits back.
    let ev = json!({"id": format!("WH-EV-{tag}"), "event_type": "PAYMENT.PAYOUTS-ITEM.RETURNED", "resource": {"payout_item_id": item_b, "transaction_status": "RETURNED", "payout_item": {"sender_item_id": pb}}});
    mock.push(200, json!({"access_token": "A21mock"}).to_string());
    mock.push(200, json!({"verification_status": "FAILURE"}).to_string());
    let r = call(
        &app,
        "POST",
        "/api/v1/webhooks/creator-payouts/paypal",
        Some(ev.clone()),
        &paypal_headers(),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    mock.push(200, json!({"access_token": "A21mock"}).to_string());
    mock.push(200, json!({"verification_status": "SUCCESS"}).to_string());
    let r = call(
        &app,
        "POST",
        "/api/v1/webhooks/creator-payouts/paypal",
        Some(ev),
        &paypal_headers(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let verify = mock
        .requests()
        .into_iter()
        .rfind(|x| {
            x.url
                .ends_with("/v1/notifications/verify-webhook-signature")
        })
        .unwrap();
    assert!(verify.body.contains("\"webhook_id\":\"WH-mock\""));
    wait_eur(&app, &carol, Decimal::from(20)).await;

    // ---- Schedule without approval: staff "run now" creates and sends at once.
    settings(&app, &admin, json!({"mode": "multi_vendor", "commission_percent": "20", "hold_days": 0, "min_payout": "5", "payout_schedule": "weekly", "payout_approval": false})).await;
    mock.push(200, json!({"access_token": "A21mock"}).to_string());
    mock.push(
        201,
        json!({"batch_header": {"payout_batch_id": batch2}}).to_string(),
    );
    mock.push(200, json!({"access_token": "A21mock"}).to_string());
    mock.push(200, json!({"items": []}).to_string());
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/creator-payouts/run",
        Some(json!({"create": true, "send": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["created"].as_u64().unwrap() >= 1, "{:?}", r.body);
    let list = call(
        &app,
        "GET",
        "/api/v1/client/creator/payouts",
        None,
        &carol.h(),
    )
    .await;
    let auto = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["auto"] == true)
        .expect("scheduled payout")
        .clone();
    assert_eq!(dec(&auto["amount"]), Decimal::from(20));
    assert_eq!(auto["status"], "processing");
    assert_eq!(auto["batch_id"], batch2.as_str());
    assert_eq!(eur(&app, &carol).await, Decimal::ZERO);

    // ---- Staff lock the method.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/creators/{cid}/payout"),
        Some(json!({"method": "manual", "locked": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator/payout-settings",
        Some(json!({"method": "stripe_connect"})),
        &carol.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.payout_method_locked");
    let r = call(&app, "PUT", "/api/v1/client/creator", Some(json!({"slug": format!("pay-{tag}"), "display_name": "Pay Dev", "payout_method": "paypal"})), &carol.h()).await;
    assert_eq!(
        r.body["error"]["code"], "error.payout_method_locked",
        "profile cannot bypass the lock"
    );

    // ---- New version: buyers get an update mail.
    let up = call_bytes(&app, &format!("/api/v1/client/creator/listings/{pid}/versions?version=2.0.0&filename=script.zip&changelog=New"), b"PK\x03\x04payload".to_vec(), &carol.h()).await;
    assert_eq!(up.0, StatusCode::OK, "{:?}", up.1);
    let mut mailed = 0i64;
    for _ in 0..100 {
        mailed = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'digital_version_released' AND payload->>'to' = $1")
            .bind(&bob.email)
            .fetch_one(&st.pool)
            .await
            .unwrap();
        if mailed > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(mailed, 1, "bob owns the product and is told once");

    for k in [
        env::STRIPE_KEY,
        env::STRIPE_WEBHOOK,
        env::PAYPAL_ID,
        env::PAYPAL_SECRET,
        env::PAYPAL_WEBHOOK,
    ] {
        std::env::remove_var(k);
    }
    // Unconfigured providers: webhooks refuse, overview says so.
    let ev = json!({"id": "evt_x", "type": "account.updated", "data": {"object": {}}});
    assert_eq!(
        stripe_hook(&app, &ev, None).await.status,
        StatusCode::CONFLICT
    );
    let o = call(
        &app,
        "GET",
        "/api/v1/admin/creator-payouts/overview",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(o.body["stripe"]["configured"], false);
}

/// POST raw bytes (version uploads).
async fn call_bytes(
    app: &axum::Router,
    uri: &str,
    body: Vec<u8>,
    h: &[(&str, &str)],
) -> (StatusCode, Value) {
    use tower::ServiceExt;
    let mut b = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/octet-stream");
    for (k, v) in h {
        b = b.header(*k, *v);
    }
    let res = app
        .clone()
        .oneshot(b.body(axum::body::Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
