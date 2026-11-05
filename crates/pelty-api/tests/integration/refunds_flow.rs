use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_addon_gateway_stripe::stripe::{HttpRequestSpec, StripeGateway, StripeTransport};
use pelty_payments::{BoxFuture, GatewayConfig, GatewayError, GatewayRegistry, PaymentGateway};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// Offline Stripe: records requests; amount 777 (minor units) is declined.
static SENT: Mutex<Vec<HttpRequestSpec>> = Mutex::new(Vec::new());
struct MockStripe;
impl StripeTransport for MockStripe {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            assert!(req.url.starts_with("https://api.stripe.com/v1/"));
            let declined = req.body.contains("amount=777&") || req.body.ends_with("amount=777");
            SENT.lock().unwrap().push(req);
            Ok(if declined {
                (402, r#"{"error":{"message":"card_declined"}}"#.into())
            } else {
                (
                    200,
                    format!(
                        r#"{{"id":"re_{}","status":"succeeded"}}"#,
                        uuid::Uuid::new_v4().simple()
                    ),
                )
            })
        })
    }
}
fn mock_factory(_: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    Ok(Box::new(StripeGateway::new(
        "sk_test_mock",
        "whsec_mock",
        Box::new(MockStripe),
    )))
}
fn sent_for(pi: &str) -> Vec<HttpRequestSpec> {
    SENT.lock()
        .unwrap()
        .iter()
        .filter(|r| r.body.contains(&format!("payment_intent={pi}")))
        .cloned()
        .collect()
}

fn n(v: &Value) -> String {
    v.as_str()
        .unwrap_or_default()
        .parse::<rust_decimal::Decimal>()
        .map(|d| d.normalize().to_string())
        .unwrap_or_default()
}

async fn setup() -> (axum::Router, pelty_api::AppState) {
    let (_, mut st) = app().await;
    let mut reg = common::gateways(GatewayRegistry::default());
    reg.register(
        pelty_addon_gateway_stripe::stripe::descriptor(),
        mock_factory,
    );
    st.gateways = Arc::new(reg);
    pelty_database::raw_execute(&st.pool, "INSERT INTO payment_gateways (id, enabled) VALUES ('stripe', TRUE) ON CONFLICT (id) DO NOTHING").await.unwrap();
    (pelty_api::router(st.clone()), st)
}

async fn paid_invoice(
    app: &axum::Router,
    admin: &Sess,
    cid: &str,
    price: &str,
    rate: &str,
    gateway: &str,
    pref: &str,
) -> (String, String, String) {
    let r = call(app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true,
        "items": [{"description": "Hosting", "quantity": "1", "unit_price": price, "tax_rate": rate}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = r.body["invoice"]["total"].as_str().unwrap().to_string();
    let r = call(app, "POST", &format!("/api/v1/admin/invoices/{id}/payments"), Some(json!({"amount": total, "idempotency_key": uuid::Uuid::new_v4().to_string(), "reference": pref, "gateway": gateway})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        app,
        "GET",
        &format!("/api/v1/admin/invoices/{id}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["invoice"]["status"], "paid");
    (
        id,
        r.body["payments"][0]["id"].as_str().unwrap().to_string(),
        total,
    )
}

#[tokio::test]
async fn partial_and_full_refunds_via_gateways_issue_credit_notes() {
    let (app, st) = setup().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let aid = call(
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
    let _ = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE"})),
        &bob.h(),
    )
    .await;

    // ---- Stripe (mocked transport): 100 + 19 % = 119.
    let pi = format!("pi_{}", uuid::Uuid::new_v4().simple());
    let (inv, pay, total) = paid_invoice(&app, &admin, &aid, "100.00", "19", "stripe", &pi).await;
    assert_eq!(n(&json!(total)), "119");
    let url = format!("/api/v1/admin/payments/{pay}/refunds");
    assert_eq!(
        call(
            &app,
            "POST",
            &url,
            Some(json!({"amount": "10", "idempotency_key": "x"})),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &url,
            Some(json!({"amount": "0.001", "idempotency_key": "a"})),
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
            &url,
            Some(json!({"amount": "-1", "idempotency_key": "a"})),
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
            &url,
            Some(json!({"amount": "119.01", "idempotency_key": "a"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"amount": "10.00", "reason": "Goodwill ș", "idempotency_key": "k1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (r.body["status"].as_str(), n(&r.body["amount"])),
        (Some("succeeded"), "10".into())
    );
    assert!(r.body["provider_ref"].as_str().unwrap().starts_with("re_"));
    let first = r.body["id"].clone();
    let cn = r.body["credit_note_id"].as_str().unwrap().to_string();
    let reqs = sent_for(&pi);
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].body.contains("amount=1000"));
    assert!(reqs[0]
        .headers
        .iter()
        .any(|(k, v)| k == "Idempotency-Key" && v.starts_with("pelty-refund-")));
    // Replay: same refund, no second gateway call.
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"amount": "10.00", "idempotency_key": "k1"})),
        &admin.h(),
    )
    .await;
    assert_eq!((r.status, r.body["id"].clone()), (StatusCode::OK, first));
    assert_eq!(sent_for(&pi).len(), 1);

    // Credit note: 8.40 net + 1.60 tax = 10.00, numbered CN-.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{cn}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["invoice"]["kind"], "credit_note");
    assert!(r.body["invoice"]["number"]
        .as_str()
        .unwrap()
        .starts_with("CN"));
    assert_eq!(
        (
            n(&r.body["invoice"]["subtotal"]),
            n(&r.body["invoice"]["tax_total"]),
            n(&r.body["invoice"]["total"])
        ),
        ("8.4".into(), "1.6".into(), "10".into())
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["invoice"]["status"], "paid");

    // Declined by the gateway: 400, recorded as failed, capacity released.
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"amount": "7.77", "idempotency_key": "k2"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("error.refund_failed"))
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}/refunds"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["refunds"].as_array().unwrap().len(), 2);
    assert_eq!(r.body["refunds"][1]["status"], "failed");
    assert_eq!(n(&r.body["refundable"][&pay]), "109");

    // Full remainder (no amount) → payment + invoice refunded.
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"idempotency_key": "k3"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, n(&r.body["amount"])),
        (StatusCode::OK, "109".into()),
        "{:?}",
        r.body
    );
    assert!(sent_for(&pi).last().unwrap().body.contains("amount=10900"));
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (
            r.body["invoice"]["status"].as_str(),
            r.body["payments"][0]["status"].as_str()
        ),
        (Some("refunded"), Some("refunded"))
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &url,
            Some(json!({"idempotency_key": "k4"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Client view: refunds + credit notes; errors hidden; owner only; PDF.
    let cu = format!("/api/v1/client/invoices/{inv}/refunds");
    let r = call(&app, "GET", &cu, None, &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["credit_notes"].as_array().unwrap().len(), 2);
    assert!(r.body["refunds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| x["error"].is_null()));
    assert_eq!(
        call(&app, "GET", &cu, None, &bob.h()).await.status,
        StatusCode::NOT_FOUND
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{cn}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    // ---- Manual gateway: offline refund recorded immediately.
    let (_, mpay, _) = paid_invoice(
        &app,
        &admin,
        &aid,
        "50.00",
        "0",
        "manual",
        &format!("bank-{}", uuid::Uuid::new_v4().simple()),
    )
    .await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/payments/{mpay}/refunds"),
        Some(json!({"amount": "20", "idempotency_key": "m"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (StatusCode::OK, Some("succeeded")),
        "{:?}",
        r.body
    );

    // ---- Credit-balance payment: refund returns money to the ledger.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{aid}/credit"),
        Some(json!({"currency": "EUR", "amount": "30"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/invoices",
        Some(
            json!({"customer_id": aid, "currency": "EUR", "finalize": true,
        "items": [{"description": "Addon", "quantity": "1", "unit_price": "30", "tax_rate": "0"}]}),
        ),
        &admin.h(),
    )
    .await;
    let ci = r.body["invoice"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/invoices/{ci}/apply-credit"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{ci}"),
        None,
        &admin.h(),
    )
    .await;
    let cpay = r.body["payments"][0]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/payments/{cpay}/refunds"),
        Some(json!({"idempotency_key": "c"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["status"].as_str()),
        (StatusCode::OK, Some("succeeded")),
        "{:?}",
        r.body
    );
    let r = call(&app, "GET", "/api/v1/client/credit", None, &alice.h()).await;
    assert_eq!(n(&r.body["balances"][0]["amount"]), "30");

    // ---- Concurrency: two 60.00 refunds on a 100.00 payment → one wins.
    let pi2 = format!("pi_{}", uuid::Uuid::new_v4().simple());
    let (_, p2, _) = paid_invoice(&app, &admin, &aid, "100.00", "0", "stripe", &pi2).await;
    let u2 = format!("/api/v1/admin/payments/{p2}/refunds");
    let h = admin.h();
    let (a, b) = tokio::join!(
        call(
            &app,
            "POST",
            &u2,
            Some(json!({"amount": "60", "idempotency_key": "p"})),
            &h
        ),
        call(
            &app,
            "POST",
            &u2,
            Some(json!({"amount": "60", "idempotency_key": "q"})),
            &h
        )
    );
    let mut st2 = [a.status, b.status];
    st2.sort();
    assert_eq!(
        st2,
        [StatusCode::OK, StatusCode::CONFLICT],
        "{:?} {:?}",
        a.body,
        b.body
    );
    assert_eq!(sent_for(&pi2).len(), 1);
}

#[tokio::test]
async fn refund_to_credit_skips_gateway_and_adds_balance() {
    let (app, st) = setup().await;
    let admin = sess(&app, &st, "admin").await;
    let carol = sess(&app, &st, "customer").await;
    let cid = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE"})),
        &carol.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let pi = format!("pi_{}", uuid::Uuid::new_v4().simple());
    let (inv, pay, _) = paid_invoice(&app, &admin, &cid, "40.00", "0", "stripe", &pi).await;
    let url = format!("/api/v1/admin/payments/{pay}/refunds");
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"amount": "15.50", "idempotency_key": "c1", "to_credit": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (
            r.status,
            r.body["status"].as_str(),
            r.body["to_credit"].as_bool()
        ),
        (StatusCode::OK, Some("succeeded"), Some(true)),
        "{:?}",
        r.body
    );
    assert!(r.body["credit_note_id"].is_string());
    assert!(
        sent_for(&pi).is_empty(),
        "no gateway call for a credit refund"
    );
    let bal: rust_decimal::Decimal = sqlx::query_scalar("SELECT COALESCE(sum(amount),0) FROM credit_ledger WHERE customer_id = $1::uuid AND currency = 'EUR'")
        .bind(&cid).fetch_one(&st.pool).await.unwrap();
    assert_eq!(bal, rust_decimal::Decimal::new(1550, 2));
    // Rest goes back through the gateway as usual; payment then fully refunded.
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"idempotency_key": "c2"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, n(&r.body["amount"])),
        (StatusCode::OK, "24.5".into()),
        "{:?}",
        r.body
    );
    assert_eq!(sent_for(&pi).len(), 1);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["payments"][0]["status"], "refunded");
}

#[tokio::test]
async fn reversal_settles_without_calling_gateway() {
    let (app, st) = setup().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let aid = call(
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
    let pi = format!("pi_{}", uuid::Uuid::new_v4().simple());
    let (inv, pay, _) = paid_invoice(&app, &admin, &aid, "50.00", "0", "stripe", &pi).await;
    let url = format!("/api/v1/admin/payments/{pay}/refunds");
    // Not combinable with to_credit.
    assert_eq!(
        call(
            &app,
            "POST",
            &url,
            Some(json!({"reversal": true, "to_credit": true, "idempotency_key": "r0"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"reversal": true, "reason": "Chargeback", "idempotency_key": "r1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["status"].as_str(),
            r.body["reversal"].as_bool(),
            n(&r.body["amount"])
        ),
        (Some("succeeded"), Some(true), "50".into())
    );
    assert!(r.body["credit_note_id"].is_string());
    assert!(
        sent_for(&pi).is_empty(),
        "gateway must not be called for a reversal"
    );
    // Replay returns the same reversal; nothing left to refund afterwards.
    let again = call(
        &app,
        "POST",
        &url,
        Some(json!({"reversal": true, "idempotency_key": "r1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(again.body["id"], r.body["id"]);
    assert_eq!(
        call(
            &app,
            "POST",
            &url,
            Some(json!({"amount": "1", "idempotency_key": "r2"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["invoice"]["status"], "refunded");
    let n_audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'payment.reversed' AND target = $1",
    )
    .bind(&pay)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(n_audit, 1);
}

#[tokio::test]
async fn reversal_can_reopen_invoice_and_suspend_services() {
    let (app, st) = setup().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let aid = call(
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
    // Settings round-trip (defaults are off).
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/settings/reversal",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/settings/reversal",
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    let pi = format!("pi_{}", uuid::Uuid::new_v4().simple());
    let (inv, pay, _) = paid_invoice(&app, &admin, &aid, "40.00", "0", "stripe", &pi).await;
    // An active service billed on this invoice.
    let (gid, pid, sid) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    let tag = uuid::Uuid::new_v4().simple().to_string();
    sqlx::query("INSERT INTO product_groups (id, name, slug) VALUES ($1,'G',$2)")
        .bind(gid)
        .bind(format!("rv-{}", &tag[..10]))
        .execute(&st.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO products (id, group_id, name, slug) VALUES ($1,$2,'P',$3)")
        .bind(pid)
        .bind(gid)
        .bind(format!("rvp-{}", &tag[..10]))
        .execute(&st.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount) VALUES ($1,$2::uuid,$3,'active','monthly','EUR',40)")
        .bind(sid).bind(&aid).bind(pid).execute(&st.pool).await.unwrap();
    // Items of a finalized invoice are immutable; bypass the guard for this
    // fixture only (transaction-local, superuser test DB).
    let mut tx = st.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE invoice_items SET service_id = $1 WHERE invoice_id = $2::uuid")
        .bind(sid)
        .bind(&inv)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let url = format!("/api/v1/admin/payments/{pay}/refunds");
    let r = call(&app, "POST", &url, Some(json!({"reversal": true, "reopen_invoice": true, "suspend_services": true, "reason": "Chargeback", "idempotency_key": "rr1"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["reversal"], true);
    assert!(
        r.body["credit_note_id"].is_null(),
        "reopen mode issues no credit note"
    );
    assert!(sent_for(&pi).is_empty());
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["invoice"]["status"], "unpaid");
    assert_eq!(n(&r.body["invoice"]["amount_paid"]), "0");
    let (status, reason): (String, String) =
        sqlx::query_as("SELECT status, suspend_reason FROM services WHERE id = $1")
            .bind(sid)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), reason.as_str()), ("suspended", "overdue"));
    // Paying again unsuspends through the normal path.
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{inv}/payments"), Some(json!({"amount": "40.00", "idempotency_key": uuid::Uuid::new_v4().to_string(), "reference": format!("again-{}", uuid::Uuid::new_v4().simple()), "gateway": "manual"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let status: String = sqlx::query_scalar("SELECT status FROM services WHERE id = $1")
        .bind(sid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
}
