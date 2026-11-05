//! Every captcha-protected form is throttled per form and IP, even with
//! captcha disabled; budgets are independent per form.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

#[tokio::test]
async fn forms_are_rate_limited() {
    let (app, state) = app_with(1000).await;
    let cust = sess(&app, &state, "customer").await;
    // Tighten after the session exists so setup logins don't eat the budget.
    state.auth_limiter.set_limit(3);
    let h = cust.h();
    let open = || {
        call(
            &app,
            "POST",
            "/api/v1/client/tickets",
            Some(json!({"department_id": SUPPORT, "subject": "Rate", "body": "x"})),
            &h,
        )
    };
    for i in 0..3 {
        let r = open().await;
        assert_eq!(r.status, StatusCode::OK, "ticket {i}: {:?}", r.body);
    }
    let r = open().await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.body["error"]["code"], "error.rate_limited");
    // Another form keeps its own budget.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/password/forgot",
        Some(json!({"email": email()})),
        &[],
    )
    .await;
    assert_ne!(r.status, StatusCode::TOO_MANY_REQUESTS, "{:?}", r.body);
}
