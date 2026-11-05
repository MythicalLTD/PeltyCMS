use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn referral_cookie_attributes_signup_and_paid_invoice_accrues_commission() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let aff = sess(&app, &st, "customer").await;

    let none = call(&app, "GET", "/api/v1/client/affiliate", None, &aff.h()).await;
    assert_eq!(none.body["enrolled"], false);
    let en = call(&app, "POST", "/api/v1/client/affiliate", None, &aff.h()).await;
    assert_eq!(en.status, StatusCode::OK, "{:?}", en.body);
    let code = en.body["affiliate"]["code"].as_str().unwrap().to_string();
    let aff_id = en.body["affiliate"]["id"].as_str().unwrap().to_string();

    // Unknown code: redirect, no cookie.
    let bad = call(&app, "GET", "/api/v1/affiliates/r/nosuchcode1", None, &[]).await;
    assert_eq!(bad.status, StatusCode::SEE_OTHER);
    assert!(bad.cookies.iter().all(|c| !c.starts_with("pelty_ref=")));
    let t = call(
        &app,
        "GET",
        &format!("/api/v1/affiliates/r/{code}"),
        None,
        &[],
    )
    .await;
    assert_eq!(t.status, StatusCode::SEE_OTHER);
    let ck = t
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_ref="))
        .expect("ref cookie");
    assert!(ck.contains("HttpOnly"));
    let ck = ck.split(';').next().unwrap().to_string();

    // SPA landing endpoint.
    let tj = call(
        &app,
        "POST",
        "/api/v1/affiliates/track",
        Some(json!({"code": code})),
        &[],
    )
    .await;
    assert_eq!(tj.body["valid"], true);
    assert!(tj.cookies.iter().any(|c| c.starts_with("pelty_ref=")));
    let tb = call(
        &app,
        "POST",
        "/api/v1/affiliates/track",
        Some(json!({"code": "zzzzzzzzzz"})),
        &[],
    )
    .await;
    assert_eq!(tb.body["valid"], false);
    assert_eq!(en.body["link"], format!("/r/{code}"));
    let e = email();
    let reg = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": e, "password": "Very-Long-Pass-123", "display_name": "R"})),
        &[("cookie", ck.as_str())],
    )
    .await;
    assert_eq!(reg.status, StatusCode::CREATED, "{:?}", reg.body);
    let l = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": e, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await;
    let cookie = l
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = l.body["csrf_token"].as_str().unwrap().to_string();
    let rh = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];

    // Referred user orders (10 + 5 setup = 15.00 EUR) and admin records payment.
    let o = call(&app, "POST", "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &rh).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("aff-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);

    let d = call(&app, "GET", "/api/v1/client/affiliate", None, &aff.h()).await;
    assert_eq!(d.body["affiliate"]["referrals"], 1);
    let c = &d.body["commissions"][0];
    assert_eq!(c["invoice_id"], iid.as_str());
    // 10% default, exact decimals, rounded to the currency exponent.
    let expect = (total.parse::<rust_decimal::Decimal>().unwrap()
        / rust_decimal::Decimal::from(10))
    .round_dp(2);
    assert_eq!(
        c["amount"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        expect
    );

    // Payout: over balance rejected, valid one accepted, then balance is spent.
    let over = call(
        &app,
        "POST",
        "/api/v1/client/affiliate/payouts",
        Some(json!({"currency": "EUR", "amount": "999"})),
        &aff.h(),
    )
    .await;
    assert_eq!(
        over.body["error"]["code"],
        "error.affiliate.insufficient_balance"
    );
    let ok = call(
        &app,
        "POST",
        "/api/v1/client/affiliate/payouts",
        Some(json!({"currency": "EUR", "amount": expect.to_string()})),
        &aff.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    // Staff with affiliates.manage get an email job.
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'affiliate_payout_requested' AND payload->>'to' = $1")
        .bind(&admin.email).fetch_one(&st.pool).await.unwrap();
    assert_eq!(jobs, 1);
    let again = call(
        &app,
        "POST",
        "/api/v1/client/affiliate/payouts",
        Some(json!({"currency": "EUR", "amount": "0.01"})),
        &aff.h(),
    )
    .await;
    assert_eq!(again.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Admin only.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/affiliates", None, &aff.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let pid_ = ok.body["id"].as_str().unwrap();
    let pr = call(
        &app,
        "POST",
        &format!("/api/v1/admin/affiliates/payouts/{pid_}"),
        Some(json!({"status": "paid", "note": "SEPA"})),
        &admin.h(),
    )
    .await;
    assert_eq!(pr.status, StatusCode::OK, "{:?}", pr.body);
    let twice = call(
        &app,
        "POST",
        &format!("/api/v1/admin/affiliates/payouts/{pid_}"),
        Some(json!({"status": "rejected"})),
        &admin.h(),
    )
    .await;
    assert_eq!(twice.status, StatusCode::CONFLICT);
    let up = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/affiliates/{aff_id}"),
        Some(json!({"commission_percent": "12.5", "status": "active"})),
        &admin.h(),
    )
    .await;
    assert_eq!(up.status, StatusCode::OK, "{:?}", up.body);
}
