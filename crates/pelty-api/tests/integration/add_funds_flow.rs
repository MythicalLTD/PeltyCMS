use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn add_funds_limits_and_credit_on_payment() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let dave = sess(&app, &st, "customer").await;
    let _ = call(&app, "GET", "/api/v1/client/profile", None, &dave.h()).await;
    let url = "/api/v1/client/credit/add-funds";
    // Shared test DB: start from the default (disabled) settings.
    let off = json!({"enabled": false, "min": "10", "max": "500", "max_balance": "1000", "require_active_service": false});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/add-funds",
            Some(off),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    // Disabled: deposits refused.
    assert_eq!(
        call(
            &app,
            "POST",
            url,
            Some(json!({"currency": "EUR", "amount": "20"})),
            &dave.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    // Settings: customers can't change them; bad limits rejected.
    let cfg = json!({"enabled": true, "min": "5", "max": "100", "max_balance": "150", "require_active_service": false});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/add-funds",
            Some(cfg.clone()),
            &dave.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/add-funds",
            Some(json!({"enabled": true, "min": "50", "max": "10"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/add-funds",
            Some(cfg),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", url, None, &dave.h()).await.body["max"],
        "100"
    );

    for (amt, code) in [
        ("4.99", "error.add_funds_below_min"),
        ("100.01", "error.add_funds_above_max"),
        ("10.001", "error.invalid_amount"),
    ] {
        let r = call(
            &app,
            "POST",
            url,
            Some(json!({"currency": "EUR", "amount": amt})),
            &dave.h(),
        )
        .await;
        assert_eq!(
            (r.status, r.body["error"]["code"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(code)),
            "{amt}"
        );
    }
    let r = call(
        &app,
        "POST",
        url,
        Some(json!({"currency": "EUR", "amount": "100"})),
        &dave.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = r.body["invoice_id"].as_str().unwrap().to_string();
    // Unpaid deposit counts towards the cap: 100 + 60 > 150.
    let r = call(
        &app,
        "POST",
        url,
        Some(json!({"currency": "EUR", "amount": "60"})),
        &dave.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.add_funds_balance_cap");
    // Deposit invoice: 100.00, no tax, can't be paid with credit.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{inv}"),
        None,
        &dave.h(),
    )
    .await;
    assert_eq!(
        (
            r.body["invoice"]["status"].as_str(),
            r.body["invoice"]["total"].as_str()
        ),
        (Some("unpaid"), Some("100.0000")),
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/invoices/{inv}/apply-credit"),
        None,
        &dave.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.add_funds_no_credit");
    // Paid (gateway or manual) → credited exactly once.
    let key = format!("af-{inv}"); // unique per run: the test DB is shared
    for k in [&key, &key] {
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/invoices/{inv}/payments"),
            Some(
                json!({"amount": "100", "idempotency_key": k, "reference": k, "gateway": "manual"}),
            ),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    let r = call(&app, "GET", "/api/v1/client/credit", None, &dave.h()).await;
    let bal = r.body["balances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["currency"] == "EUR")
        .expect("EUR balance")
        .clone();
    assert_eq!(
        bal["amount"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::from(100)
    );
    assert!(r.body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "add_funds"));
    // Now 100 credit + 60 > 150, 50 fits.
    assert_eq!(
        call(
            &app,
            "POST",
            url,
            Some(json!({"currency": "EUR", "amount": "60"})),
            &dave.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            url,
            Some(json!({"currency": "EUR", "amount": "50"})),
            &dave.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    // Refunding a deposit takes the credit back; not possible once spent.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{inv}"),
        None,
        &admin.h(),
    )
    .await;
    let pay = r.body["payments"][0]["id"].as_str().unwrap().to_string();
    let cid = r.body["invoice"]["customer_id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{cid}/credit"),
        Some(json!({"currency": "EUR", "amount": "-70", "note": "spent"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let refund = format!("/api/v1/admin/payments/{pay}/refunds");
    let r = call(
        &app,
        "POST",
        &refund,
        Some(json!({"amount": "40", "idempotency_key": "af1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["error"]["code"].as_str()),
        (
            StatusCode::CONFLICT,
            Some("error.add_funds_refund_insufficient_credit")
        )
    );
    assert_eq!(eur(&app, &dave.h()).await, rust_decimal::Decimal::from(30));
    let r = call(
        &app,
        "POST",
        &refund,
        Some(json!({"amount": "30", "idempotency_key": "af2", "to_credit": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(eur(&app, &dave.h()).await, rust_decimal::Decimal::ZERO);
}

async fn eur(app: &axum::Router, h: &[(&str, &str); 2]) -> rust_decimal::Decimal {
    let r = call(app, "GET", "/api/v1/client/credit", None, h).await;
    r.body["balances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["currency"] == "EUR")
        .map(|b| b["amount"].as_str().unwrap().parse().unwrap())
        .unwrap_or_default()
}
