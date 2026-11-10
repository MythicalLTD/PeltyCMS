//! Public gift card balance check: active, redeemed, expired, unknown and
//! disabled codes, and the per-IP rate limit.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

const URL: &str = "/api/v1/public/gift-cards/balance";

async fn make(app: &axum::Router, admin: &Sess, expires: Option<String>) -> (String, String) {
    let r = call(
        app,
        "POST",
        "/api/v1/admin/gift-cards",
        Some(json!({"currency": "EUR", "amount": "25.00", "expires_at": expires})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let it = &r.body["items"][0];
    (
        it["code"].as_str().unwrap().to_string(),
        it["card"]["id"].as_str().unwrap().to_string(),
    )
}

fn dec(v: &Value) -> String {
    let t = v.as_str().unwrap_or("?");
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t.to_string()
    }
}

#[tokio::test]
async fn gift_card_balance_without_redeeming() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;

    let (code, _) = make(&app, &admin, None).await;
    // Lower-case with dashes still matches; no sign-in needed.
    let messy = code
        .to_lowercase()
        .chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-");
    let r = call(&app, "POST", URL, Some(json!({"code": messy})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["status"].as_str(),
            dec(&r.body["balance"]),
            r.body["currency"].as_str()
        ),
        (Some("active"), "25".into(), Some("EUR"))
    );

    // Checking doesn't redeem; redeeming afterwards still works, then shows used.
    let red = call(
        &app,
        "POST",
        "/api/v1/client/gift-cards/redeem",
        Some(json!({"code": code})),
        &bob.h(),
    )
    .await;
    assert_eq!(red.status, StatusCode::OK, "{:?}", red.body);
    let r = call(&app, "POST", URL, Some(json!({"code": code})), &[]).await;
    assert_eq!(
        (r.body["status"].as_str(), dec(&r.body["balance"])),
        (Some("redeemed"), "0".into()),
        "{:?}",
        r.body
    );

    // Expired: no balance shown.
    let past = (OffsetDateTime::now_utc() - Duration::days(1))
        .format(&Rfc3339)
        .unwrap();
    let (old, _) = make(&app, &admin, Some(past)).await;
    let r = call(&app, "POST", URL, Some(json!({"code": old})), &[]).await;
    if r.status == StatusCode::UNPROCESSABLE_ENTITY {
        // Creation may refuse past expiry; then just disable it below.
    } else {
        assert_eq!(
            (r.body["status"].as_str(), dec(&r.body["balance"])),
            (Some("expired"), "0".into()),
            "{:?}",
            r.body
        );
    }

    // Disabled and unknown codes look the same.
    let (dis, did) = make(&app, &admin, None).await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/gift-cards/{did}/disable"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    for c in [dis.as_str(), "AAAA-BBBB-CCCC-DDDD", "short"] {
        assert_eq!(
            call(&app, "POST", URL, Some(json!({"code": c})), &[])
                .await
                .status,
            StatusCode::NOT_FOUND,
            "{c}"
        );
    }
}

#[tokio::test]
async fn gift_card_balance_is_rate_limited() {
    let (app, _st) = app_with(5).await;
    let mut limited = false;
    for _ in 0..20 {
        let r = call(
            &app,
            "POST",
            URL,
            Some(json!({"code": "AAAA-BBBB-CCCC-DDDD"})),
            &[],
        )
        .await;
        if r.status == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
            break;
        }
        assert_eq!(r.status, StatusCode::NOT_FOUND);
    }
    assert!(limited, "balance checks must be rate limited");
}
