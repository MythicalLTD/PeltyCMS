use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn order(app: &axum::Router, c: &Sess, pid: &str, oid: &str, choice: &str) -> Value {
    let r = call(
        app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [
            {"product_id": pid, "cycle": "monthly", "config": {oid: choice}}
        ]})),
        &c.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

#[tokio::test]
async fn order_invoice_payment_service_flow_with_ownership() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;

    // Order: 10.00 + 2.50 (US) recurring + 5.00 setup = 17.50, priced server-side.
    let o = order(&app, &alice, &pid, &oid, &ch[1]).await;
    let inv = o["invoice"].clone();
    let iid = inv["id"].as_str().unwrap().to_string();
    assert_eq!(inv["total"], "17.5000");
    assert_eq!(inv["status"], "unpaid");
    assert!(inv["number"].as_str().unwrap().starts_with("INV-"));
    assert_eq!(o["order"]["status"], "pending");

    // Ownership: Bob can't see/pay Alice's invoice, and it isn't in his list.
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/invoices/{iid}"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/invoices/{iid}/pay"),
            Some(json!({"gateway": "manual"})),
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/invoices", None, &bob.h())
            .await
            .body["total"],
        0
    );
    let mine = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{iid}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(mine.status, StatusCode::OK);
    assert_eq!(mine.body["items"].as_array().unwrap().len(), 2);
    assert_eq!(mine.body["balance"], "17.5000");
    // Customers can't reach admin billing.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/invoices", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/invoices/{iid}/payments"),
            Some(json!({"amount": "17.50", "idempotency_key": "x"})),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // Finalized invoice is immutable at the DB level.
    for sql in [
        format!("UPDATE invoices SET total = 1 WHERE id = '{iid}'"),
        format!("UPDATE invoices SET status = 'draft' WHERE id = '{iid}'"),
        format!("DELETE FROM invoices WHERE id = '{iid}'"),
        format!("UPDATE invoice_items SET amount = 0 WHERE invoice_id = '{iid}'"),
        format!("INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, amount) VALUES (gen_random_uuid(), '{iid}', 'x', 1, 1, 1)"),
    ] {
        assert!(pelty_database::raw_execute(&st.pool, &sql).await.is_err(), "must fail: {sql}");
    }

    // Manual pay instructions for the customer.
    pelty_database::raw_execute(&st.pool, "INSERT INTO payment_gateways (id, enabled, config) VALUES ('manual', TRUE, '{\"instructions\":\"IBAN RO49 AAAA\"}')
        ON CONFLICT (id) DO UPDATE SET enabled = TRUE, config = EXCLUDED.config").await.unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{iid}/pay"),
        Some(json!({"gateway": "manual"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["init"]["type"], "instructions");
    assert!(r.body["init"]["text"].as_str().unwrap().contains("IBAN"));
    assert!(
        r.body["init"]["reference"]
            .as_str()
            .unwrap()
            .starts_with("INV-"),
        "human reference, not uuid: {:?}",
        r.body
    );
    assert_eq!(r.body["init"]["currency"], "EUR");
    // Bank details come from system settings; unconfigured => null (no dummy IBAN).
    pelty_database::raw_execute(
        &st.pool,
        "DELETE FROM system_settings WHERE key = 'billing.bank_transfer'",
    )
    .await
    .unwrap();
    let r0 = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{iid}/pay"),
        Some(json!({"gateway": "manual"})),
        &alice.h(),
    )
    .await;
    assert!(r0.body["bank"].is_null());
    let bank = json!({"beneficiary": "Pelty SRL", "iban": "ro49 aaaa 1b31 0075 9384 0000", "bic": "", "bank_name": "Test Bank", "note": ""});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/bank-transfer",
            Some(bank.clone()),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/bank-transfer",
        Some(json!({"iban": "RO00BAD"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (bad.status, bad.body["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("error.invalid_iban"))
    );
    let ok = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/bank-transfer",
        Some(bank),
        &admin.h(),
    )
    .await;
    assert_eq!(ok.body["iban"], "RO49AAAA1B31007593840000", "{:?}", ok.body);
    let r1 = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{iid}/pay"),
        Some(json!({"gateway": "manual"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r1.body["bank"]["iban"], "RO49AAAA1B31007593840000");
    let again = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{iid}/pay"),
        Some(json!({"gateway": "manual"})),
        &alice.h(),
    )
    .await;
    assert_eq!(
        again.body["payment_id"], r.body["payment_id"],
        "pay start is idempotent"
    );

    // Staff records a partial payment, replays it, then pays the rest.
    let pay = |amount: &'static str, key: &'static str| {
        let (app, h, iid) = (
            app.clone(),
            admin.h().map(|(a, b)| (a.to_string(), b.to_string())),
            iid.clone(),
        );
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            call(&app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"), Some(json!({"amount": amount, "idempotency_key": key, "reference": format!("bank-{iid}-{key}")})), &h).await
        }
    };
    let r = pay("10.00", "k1").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["status"], "unpaid");
    assert_eq!(r.body["invoice"]["amount_paid"], "10.0000");
    let r = pay("10.00", "k1").await;
    assert_eq!(r.body["applied"]["duplicate"], true);
    assert_eq!(
        r.body["invoice"]["amount_paid"], "10.0000",
        "replay must not double-credit"
    );
    assert_eq!(
        pay("1.001", "k2").await.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "EUR has 2 decimals"
    );
    let r = pay("7.50", "k3").await;
    assert_eq!(r.body["invoice"]["status"], "paid");
    assert_eq!(r.body["applied"]["invoice_paid"], true);
    assert!(r.body["invoice"]["paid_at"].is_string());

    // Approve → services in one transaction; ownership on services too.
    let order_id = o["order"]["id"].as_str().unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let sid = r.body["services"][0].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/orders/{order_id}/approve"),
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
            "GET",
            &format!("/api/v1/client/services/{sid}"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/services", None, &bob.h())
            .await
            .body["total"],
        0
    );
    let s = call(
        &app,
        "GET",
        &format!("/api/v1/client/services/{sid}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(s.body["status"], "active");
    assert_eq!(s.body["recurring_amount"], "12.5000");
    assert!(s.body["next_due_date"].is_string());

    // Lifecycle: suspend → unsuspend → terminate; terminal stays terminal.
    let act = |a: &'static str| {
        let (app, h, sid) = (
            app.clone(),
            admin.h().map(|(a, b)| (a.to_string(), b.to_string())),
            sid.clone(),
        );
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            call(
                &app,
                "POST",
                &format!("/api/v1/admin/services/{sid}/action"),
                Some(json!({"action": a, "reason": "test"})),
                &h,
            )
            .await
        }
    };
    assert_eq!(act("suspend").await.body["status"], "suspended");
    assert_eq!(act("unsuspend").await.body["status"], "active");
    assert_eq!(
        act("explode").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(act("terminate").await.body["status"], "terminated");
    assert_eq!(act("activate").await.status, StatusCode::CONFLICT);
    assert!(
        pelty_database::raw_execute(
            &st.pool,
            &format!("UPDATE services SET status = 'active' WHERE id = '{sid}'")
        )
        .await
        .is_err(),
        "DB trigger blocks illegal transition"
    );

    // Credit note on the paid invoice.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/credit-note"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["number"].as_str().unwrap().starts_with("CN-"));
    assert_eq!(r.body["total"], "17.5000");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/invoices/{iid}"),
            None,
            &admin.h()
        )
        .await
        .body["invoice"]["status"],
        "refunded"
    );

    // Audit trail exists for the payment.
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'payment.recorded' AND target = $1",
    )
    .bind(&iid)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn cart_validation() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let c = sess(&app, &st, "customer").await;
    let post = |b: Value| {
        let (app, h) = (
            app.clone(),
            c.h().map(|(a, b)| (a.to_string(), b.to_string())),
        );
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            call(&app, "POST", "/api/v1/client/orders", Some(b), &h).await
        }
    };
    let bad = [
        json!({"currency": "EUR", "items": []}),
        json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "weekly", "config": {oid.clone(): ch[0]}}]}),
        json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly"}]}),
        json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): uuid::Uuid::new_v4()}}]}),
        json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {uuid::Uuid::new_v4().to_string(): ch[0]}}]}),
        json!({"currency": "EUR", "items": [{"product_id": uuid::Uuid::new_v4(), "cycle": "monthly"}]}),
        json!({"currency": "USD", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]}),
        json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "quarterly", "config": {oid.clone(): ch[0]}}]}),
    ];
    for b in bad {
        let r = post(b.clone()).await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{b} -> {:?}",
            r.body
        );
    }
    let r = post(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}, "amount": "0.01"}]})).await;
    assert!(r.status.is_client_error(), "client cannot send a price");
    // JPY: exponent 0, annual EUR.
    let r = post(json!({"currency": "JPY", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "1500.0000");
    let r = post(json!({"currency": "EUR", "items": [
        {"product_id": pid, "cycle": "annual", "config": {oid.clone(): ch[0]}},
        {"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[1]}}
    ]}))
    .await;
    assert_eq!(r.body["invoice"]["total"], "117.5000");
    // Cancel voids the unpaid invoice.
    let oid2 = r.body["order"]["id"].as_str().unwrap();
    let iid2 = r.body["invoice"]["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/orders/{oid2}/cancel"),
            Some(json!({})),
            &admin.h()
        )
        .await
        .body["status"],
        "cancelled"
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/invoices/{iid2}"),
            None,
            &c.h()
        )
        .await
        .body["invoice"]["status"],
        "void"
    );
    // Closed customers can't order.
    let cid = call(&app, "GET", "/api/v1/client/profile", None, &c.h())
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/customers/{cid}"),
        Some(json!({"status": "closed"})),
        &admin.h(),
    )
    .await;
    assert_eq!(post(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "annual", "config": {oid: ch[0]}}]})).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn invoice_numbers_are_unique_and_gapless_under_concurrency() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let cid = call(&app, "GET", "/api/v1/client/profile", None, &c.h())
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut handles = Vec::new();
    for i in 0..12 {
        let (app, h, cid) = (
            app.clone(),
            admin.h().map(|(a, b)| (a.to_string(), b.to_string())),
            cid.clone(),
        );
        handles.push(tokio::spawn(async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            call(&app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true,
                "items": [{"description": format!("Item {i}"), "quantity": "3", "unit_price": "0.335", "tax_rate": "19"}]})), &h).await
        }));
    }
    let mut nums = Vec::new();
    for h in handles {
        let r = h.await.unwrap();
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        // 3 × 0.335 = 1.005 → 1.01; tax 19% = 0.1919 → 0.19; total 1.20
        assert_eq!(r.body["invoice"]["total"], "1.2000");
        nums.push(
            r.body["invoice"]["number"].as_str().unwrap()[4..]
                .parse::<i64>()
                .unwrap(),
        );
    }
    nums.sort();
    nums.dedup();
    assert_eq!(nums.len(), 12, "unique numbers");
    // Sequence has no gaps overall.
    let (max, cnt): (i64, i64) = sqlx::query_as("SELECT max(substr(number, 5)::bigint), count(*) FROM invoices WHERE kind = 'invoice' AND number ~ '^INV-[0-9]+$'")
        .fetch_one(&st.pool).await.unwrap();
    assert_eq!(max, cnt, "gapless");

    // Drafts are editable and invisible to the customer until finalized.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/invoices",
        Some(json!({"customer_id": cid, "currency": "EUR",
        "items": [{"description": "Draft", "quantity": "1", "unit_price": "5"}]})),
        &admin.h(),
    )
    .await;
    let did = r.body["invoice"]["id"].as_str().unwrap().to_string();
    assert!(r.body["invoice"]["number"].is_null());
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/invoices/{did}"),
            None,
            &c.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert!(pelty_database::raw_execute(
        &st.pool,
        &format!("UPDATE invoice_items SET description = 'ok' WHERE invoice_id = '{did}'")
    )
    .await
    .is_ok());
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{did}/finalize"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/invoices/{did}/finalize"),
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
            "POST",
            &format!("/api/v1/admin/invoices/{did}/void"),
            None,
            &admin.h()
        )
        .await
        .body["status"],
        "void"
    );
}

#[tokio::test]
async fn stripe_webhook_marks_invoice_paid_idempotently() {
    use hmac::{Hmac, Mac};
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let c = sess(&app, &st, "customer").await;
    let o = order(&app, &c, &pid, &oid, &ch[0]).await;
    let iid = o["invoice"]["id"].as_str().unwrap().to_string();
    std::env::set_var("PELTY_TEST_STRIPE_SK", "sk_test_123");
    std::env::set_var("PELTY_TEST_STRIPE_WHSEC", "whsec_integration");
    pelty_database::raw_execute(
        &st.pool,
        r#"INSERT INTO payment_gateways (id, enabled, secret_refs) VALUES ('stripe', TRUE,
        '{"secret_key":"PELTY_TEST_STRIPE_SK","webhook_secret":"PELTY_TEST_STRIPE_WHSEC"}')
        ON CONFLICT (id) DO UPDATE SET enabled = TRUE, secret_refs = EXCLUDED.secret_refs"#,
    )
    .await
    .unwrap();
    // Simulate a started Stripe checkout (the real call needs the network).
    let inv = pelty_database::invoices::get(&st.pool, iid.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    let p = pelty_database::invoices::create_pending(
        &st.pool,
        &inv,
        "stripe",
        None,
        &format!("t:{iid}"),
    )
    .await
    .unwrap();
    let cs = format!("cs_{}", uuid::Uuid::new_v4().simple());
    pelty_database::invoices::set_provider_ref(&st.pool, p.id, &cs)
        .await
        .unwrap();

    let sign = |t: i64, body: &str| {
        let mut m = Hmac::<sha2::Sha256>::new_from_slice(b"whsec_integration").unwrap();
        m.update(format!("{t}.{body}").as_bytes());
        format!("t={t},v1={}", hex::encode(m.finalize().into_bytes()))
    };
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    for _ in 0..2 {
        // Two distinct events for the same session (Stripe retries / duplicates).
        let evt = format!("evt_{}", uuid::Uuid::new_v4().simple());
        let body = json!({"id": evt, "type": "checkout.session.completed", "data": {"object": {"id": cs}}}).to_string();
        let r = call(&app, "POST", "/api/v1/webhooks/stripe", None, &[]).await;
        assert!(r.status.is_client_error());
        let req = axum::http::Request::post("/api/v1/webhooks/stripe")
            .header("stripe-signature", sign(now, &body))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
        let res = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
    let inv = pelty_database::invoices::get(&st.pool, iid.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inv.status, "paid");
    assert_eq!(inv.amount_paid, inv.total, "credited exactly once");
    let pays = pelty_database::invoices::payments(&st.pool, inv.id)
        .await
        .unwrap();
    assert_eq!(pays.iter().filter(|p| p.applied_at.is_some()).count(), 1);
}

#[tokio::test]
async fn recurring_billing_is_idempotent() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let c = sess(&app, &st, "customer").await;
    let o = order(&app, &c, &pid, &oid, &ch[0]).await;
    let order_id = o["order"]["id"].as_str().unwrap();
    let sid = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await
    .body["services"][0]
        .as_str()
        .unwrap()
        .to_string();
    let today = time::OffsetDateTime::now_utc().date();
    pelty_database::raw_execute(
        &st.pool,
        &format!("UPDATE services SET next_due_date = '{today}' WHERE id = '{sid}'"),
    )
    .await
    .unwrap();
    let count = || async {
        sqlx::query_scalar::<_, i64>(&format!(
            "SELECT count(*) FROM invoice_items WHERE service_id = '{sid}'"
        ))
        .fetch_one(&st.pool)
        .await
        .unwrap()
    };
    let horizon = today + time::Duration::days(7);
    pelty_database::services::bill_due(&st.pool, horizon, 1000, 7)
        .await
        .unwrap();
    assert_eq!(count().await, 1);
    pelty_database::services::bill_due(&st.pool, horizon, 1000, 7)
        .await
        .unwrap();
    assert_eq!(count().await, 1, "due date advanced, no double billing");
    // Even if the due date is rolled back (bug/manual edit), the period is not billed twice.
    pelty_database::raw_execute(
        &st.pool,
        &format!("UPDATE services SET next_due_date = '{today}' WHERE id = '{sid}'"),
    )
    .await
    .unwrap();
    pelty_database::services::bill_due(&st.pool, horizon, 1000, 7)
        .await
        .unwrap();
    assert_eq!(count().await, 1);
    let svc = pelty_database::services::get(&st.pool, sid.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        svc.next_due_date,
        Some(pelty_billing::cycle::add_months(today, 1))
    );
    let r = call(&app, "GET", "/api/v1/client/invoices", None, &c.h()).await;
    assert_eq!(
        r.body["total"], 2,
        "order invoice + renewal visible to owner"
    );
}

#[tokio::test]
async fn dashboards_aggregate_per_customer_and_require_staff() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let carol = sess(&app, &st, "customer").await;
    let dave = sess(&app, &st, "customer").await;

    let empty = call(&app, "GET", "/api/v1/client/dashboard", None, &carol.h()).await;
    assert_eq!(empty.status, StatusCode::OK, "{:?}", empty.body);
    assert_eq!(
        (
            empty.body["active_services"].as_i64(),
            empty.body["unpaid_invoices"].as_i64()
        ),
        (Some(0), Some(0))
    );

    let o1 = order(&app, &carol, &pid, &oid, &ch[0]).await; // 10 + 5 setup = 15
    order(&app, &carol, &pid, &oid, &ch[1]).await; // 17.50
    order(&app, &dave, &pid, &oid, &ch[0]).await; // not Carol's
    let oid1 = o1["order"]["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/orders/{oid1}/approve"),
            Some(json!({})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    let d = call(&app, "GET", "/api/v1/client/dashboard", None, &carol.h())
        .await
        .body;
    assert_eq!(d["unpaid_invoices"], 2);
    assert_eq!(d["active_services"], 1);
    assert_eq!(d["due"][0]["currency"], "EUR");
    assert_eq!(d["due"][0]["amount"], "32.5000");
    assert_eq!(d["upcoming"].as_array().unwrap().len(), 1);
    assert!(d["spend_series"].is_array());

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/dashboard", None, &carol.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let a = call(&app, "GET", "/api/v1/admin/dashboard", None, &admin.h()).await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    assert!(a.body["pending_orders"].as_i64().unwrap() >= 2);
    assert!(a.body["active_services"].as_i64().unwrap() >= 1);
    let eur = a.body["unpaid"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["currency"] == "EUR")
        .unwrap();
    assert!(eur["count"].as_i64().unwrap() >= 3);
    assert!(
        a.body["revenue_month"].is_array()
            && a.body["failed_jobs"].is_i64()
            && a.body["revenue_series"].is_array()
    );
}
