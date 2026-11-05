use crate::common;
use axum::http::StatusCode;
use common::*;
use hmac::{Hmac, Mac};
use pelty_payments::http::MockTransport;
use pelty_payments::GatewayRegistry;
use serde_json::{json, Value};
use std::sync::Arc;

fn sign(secret: &str, t: i64, body: &str) -> String {
    let mut m = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    m.update(format!("{t}.{body}").as_bytes());
    format!("t={t},v1={}", hex::encode(m.finalize().into_bytes()))
}

async fn raw_post(
    app: &axum::Router,
    uri: &str,
    headers: &[(&str, String)],
    body: String,
) -> (StatusCode, String) {
    let mut b = axum::http::Request::post(uri).header("content-type", "application/json");
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    let res =
        tower::ServiceExt::oneshot(app.clone(), b.body(axum::body::Body::from(body)).unwrap())
            .await
            .unwrap();
    let st = res.status();
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    (st, String::from_utf8_lossy(&bytes).into_owned())
}

fn ids(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| g["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// One sequential scenario: every step mutates the shared `stripe` row.
#[tokio::test]
async fn admin_config_eligibility_return_flow_and_webhooks() {
    let (_, mut st) = app().await;
    let t = MockTransport::new();
    st.gateways = Arc::new(common::gateways(GatewayRegistry::with_transport(Arc::new(
        t.clone(),
    ))));
    let app = pelty_api::router(st.clone());
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    std::env::set_var("PELTY_TEST_STRIPE_SK", "sk_test_123");
    std::env::set_var("PELTY_TEST_STRIPE_WHSEC", "whsec_integration");
    let refs =
        json!({"secret_key": "PELTY_TEST_STRIPE_SK", "webhook_secret": "PELTY_TEST_STRIPE_WHSEC"});

    // ---- Admin listing exposes schema + secret status, never values; customers are refused.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/gateways", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", "/api/v1/admin/gateways", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    for id in [
        "manual",
        "stripe",
        "paypal",
        "mollie",
        "razorpay",
        "crypto",
        "paddle",
        "adyen",
        "gocardless",
    ] {
        assert!(ids(&r.body).contains(&id.to_string()), "{id} listed");
    }
    assert!(!r.body.to_string().contains("sk_test_123"));
    let stripe = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "stripe")
        .unwrap()
        .clone();
    assert!(stripe["settings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["key"] == "min_amount"));

    // ---- Validation.
    let ah = admin.h();
    macro_rules! put {
        ($b:expr) => {
            call(&app, "PUT", "/api/v1/admin/gateways/stripe", Some($b), &ah)
        };
    }
    let r =
        put!(json!({"enabled": true, "config": {}, "secret_refs": {"secret_key": "lower_case"}}))
            .await;
    assert_eq!(
        (r.status, r.body["error"]["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("error.gateway_secret_ref")
        )
    );
    let r = put!(
        json!({"enabled": true, "config": {"api_base": "http://evil.example"}, "secret_refs": refs})
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    let r =
        put!(json!({"enabled": true, "config": {"min_amount": "abc"}, "secret_refs": refs})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/gateways/nope",
            Some(json!({"enabled": true, "config": {}, "secret_refs": {}})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // ---- Eligibility by currency and amount (invoice 119 EUR).
    let cid = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE"})),
        &alice.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = call(&app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true,
        "items": [{"description": "Hosting", "quantity": "1", "unit_price": "100.00", "tax_rate": "19"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let (eurl, alh) = (format!("/api/v1/client/invoices/{inv}/gateways"), alice.h());
    macro_rules! elig {
        () => {
            call(&app, "GET", &eurl, None, &alh)
        };
    }
    assert_eq!(
        put!(json!({"enabled": true, "config": {"currencies": "USD"}, "secret_refs": refs}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(
        !ids(&elig!().await.body).contains(&"stripe".into()),
        "currency filter"
    );
    assert_eq!(
        put!(json!({"enabled": true, "config": {"max_amount": "50"}, "secret_refs": refs}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(
        !ids(&elig!().await.body).contains(&"stripe".into()),
        "max amount filter"
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{inv}/pay"),
        Some(json!({"gateway": "stripe"})),
        &alice.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["error"]["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("error.gateway_ineligible")
        )
    );
    assert_eq!(put!(json!({"enabled": true, "config": {"min_amount": "10", "max_amount": "1000", "display_name": "Card"}, "secret_refs": refs})).await.status, StatusCode::NO_CONTENT);
    let r = elig!().await;
    assert!(ids(&r.body).contains(&"stripe".into()), "{:?}", r.body);
    assert_eq!(r.body["currency"], "EUR");
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g["id"] == "stripe" && g["name"] == "Card"));
    let bob = sess(&app, &st, "customer").await;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/invoices/{inv}/gateways"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // ---- Checkout over the (mock) transport, then return-URL completion.
    let cs = format!("cs_{}", uuid::Uuid::new_v4().simple());
    t.push(
        200,
        json!({"id": cs, "url": "https://checkout.stripe.com/c/pay/x"}).to_string(),
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{inv}/pay"),
        Some(json!({"gateway": "stripe"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["init"]["url"], "https://checkout.stripe.com/c/pay/x");
    let pid = r.body["payment_id"].as_str().unwrap().to_string();
    let sent = t.requests();
    let create = sent.last().unwrap();
    assert!(create.url.ends_with("/v1/checkout/sessions"));
    assert!(
        create.body.contains(&format!("pay%3Fpayment%3D{pid}"))
            || create.body.contains(&format!("pay?payment={pid}")),
        "{}",
        create.body
    );
    assert_eq!(create.header("authorization"), Some("Bearer sk_test_123"));

    let curl = format!("/api/v1/client/payments/{pid}/complete");
    macro_rules! done { ($who:expr) => { call(&app, "POST", &curl, Some(json!({"query": {"return": "1"}})), &$who.h()) }; }
    assert_eq!(done!(bob).await.status, StatusCode::NOT_FOUND);
    t.push(200, r#"{"status":"open","payment_status":"unpaid"}"#);
    assert_eq!(done!(alice).await.body["status"], "pending");
    t.push(200, r#"{"status":"complete","payment_status":"paid"}"#);
    let r = done!(alice).await;
    assert_eq!(r.body["status"], "succeeded", "{:?}", r.body);
    let n = t.requests().len();
    assert_eq!(done!(alice).await.body["status"], "succeeded", "idempotent");
    assert_eq!(t.requests().len(), n, "no provider call once settled");
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{inv}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(r.body["invoice"]["status"], "paid");

    // ---- Webhook: dispute is recorded and logged; bad signatures are logged too.
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let evt = format!("evt_{}", uuid::Uuid::new_v4().simple());
    let body = json!({"id": evt, "type": "charge.dispute.created", "data": {"object": {"payment_intent": "pi_x", "metadata": {"pelty_payment_id": pid}}}}).to_string();
    let (code, _) = raw_post(
        &app,
        "/api/v1/webhooks/stripe",
        &[("stripe-signature", sign("whsec_integration", now, &body))],
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = raw_post(
        &app,
        "/api/v1/webhooks/stripe",
        &[("stripe-signature", sign("wrong", now, &body))],
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'payment.disputed' AND metadata->>'event_id' = $1").bind(&evt).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n, 1);

    // ---- Test-webhook tool: verifies + parses, never applies.
    macro_rules! tw { ($h:expr, $b:expr) => { call(&app, "POST", "/api/v1/admin/gateways/stripe/test-webhook", Some(json!({"headers": $h, "body": $b})), &ah) }; }
    let tb = json!({"id": "evt_test_1", "type": "charge.refunded", "data": {"object": {"refunded": true, "payment_intent": "pi_t"}}}).to_string();
    let r = tw!(
        json!({"Stripe-Signature": sign("whsec_integration", now, &tb)}),
        &tb
    )
    .await;
    assert_eq!(
        (r.body["ok"].as_bool(), r.body["event"]["kind"].as_str()),
        (Some(true), Some("refunded")),
        "{:?}",
        r.body
    );
    let r = tw!(json!({"Stripe-Signature": sign("nope", now, &tb)}), &tb).await;
    assert_eq!(
        (r.body["ok"].as_bool(), r.body["stage"].as_str()),
        (Some(false), Some("signature"))
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/gateways/stripe/test-webhook",
            Some(json!({"headers": {}, "body": "{}"})),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // ---- Delivery log.
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/gateways/deliveries?gateway=stripe&limit=50",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let items = r.body["items"].as_array().unwrap();
    assert!(items.iter().all(|d| d["gateway"] == "stripe"));
    assert!(items.iter().any(|d| d["event_id"] == evt.as_str()
        && d["kind"] == "disputed"
        && d["signature_ok"] == true));
    assert!(items
        .iter()
        .any(|d| d["signature_ok"] == false && d["is_test"] == false));
    assert!(items
        .iter()
        .any(|d| d["is_test"] == true && d["event_id"] == "evt_test_1"));
    assert!(!r.body.to_string().contains("whsec_integration"));
}

#[tokio::test]
async fn custom_gateway_lifecycle() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let slug = format!("acme{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let id = format!("custom-{slug}");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/gateways/custom",
        Some(json!({"slug": slug, "name": "Acme Pay"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["id"].as_str()),
        (StatusCode::OK, Some(id.as_str())),
        "{:?}",
        r.body
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/gateways/custom",
            Some(json!({"slug": slug, "name": "Again"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/gateways/custom",
            Some(json!({"slug": "Bad Slug!", "name": "x"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let list = call(&app, "GET", "/api/v1/admin/gateways", None, &admin.h()).await;
    let g = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        (
            g["custom"].as_bool(),
            g["enabled"].as_bool(),
            g["kind"].as_str()
        ),
        (Some(true), Some(false), Some("custom_http"))
    );

    // Broken template is refused; a valid one with a webhook secret verifies test payloads.
    let mut cfg = g["config"].clone();
    cfg["create"]["url"] = json!("ftp://x");
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/gateways/{id}"),
        Some(json!({"enabled": true, "config": cfg, "secret_refs": {}})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    std::env::set_var("PELTY_TEST_CUSTOM_WH", "custom_secret");
    let r = call(&app, "PUT", &format!("/api/v1/admin/gateways/{id}"), Some(json!({"enabled": true, "config": g["config"], "secret_refs": {"webhook_secret": "PELTY_TEST_CUSTOM_WH"}})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let body = r#"{"id":"e1","type":"payment.succeeded","data":{"id":"ref_1"}}"#;
    let mut m = Hmac::<sha2::Sha256>::new_from_slice(b"custom_secret").unwrap();
    m.update(body.as_bytes());
    let sig = hex::encode(m.finalize().into_bytes());
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/gateways/{id}/test-webhook"),
        Some(json!({"headers": {"X-Signature": sig}, "body": body})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.body["ok"].as_bool(), r.body["event"]["kind"].as_str()),
        (Some(true), Some("payment_succeeded")),
        "{:?}",
        r.body
    );

    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/gateways/stripe",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/gateways/{id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/gateways/{id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
