//! Captcha + SSO together: remote captcha engines (Turnstile, hCaptcha) on
//! the password login via mocked siteverify, while SSO sign-in (GitHub, with
//! verified email from /user/emails) bypasses the form captcha because the
//! identity provider authenticated the user. Disabling the SSO addon hides it.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

fn state_of(url: &str) -> String {
    form_urlencoded::parse(url.split_once('?').unwrap().1.as_bytes())
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned()
}

#[tokio::test]
async fn captcha_and_sso() {
    let (app, state) = app().await;
    let mock = pelty_payments::MockTransport::new();
    state.set_http(std::sync::Arc::new(mock.clone()));
    let admin = sess(&app, &state, "admin").await;
    // Start from a known policy: the DB is shared with other flows.
    call(
        &app,
        "PUT",
        "/api/v1/admin/security/captcha",
        Some(json!({"mode": "never", "provider": "", "forms": []})),
        &admin.h(),
    )
    .await;
    let pw = "Very-Long-Pass-123";
    let user = email();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": user, "password": pw, "display_name": "U"})),
        &[],
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let login = json!({"email": user, "password": pw});

    // Turnstile on login + register
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/addons/captcha-turnstile/settings",
        Some(json!({"site_key": "ts-pub", "secret_key": "ts-sec"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "PUT", "/api/v1/admin/security/captcha", Some(json!({"mode": "always", "provider": "captcha-turnstile", "forms": ["login", "register"]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cfg = call(&app, "GET", "/api/v1/captcha/config", None, &[]).await;
    assert_eq!(
        (cfg.body["engine"].as_str(), cfg.body["site_key"].as_str()),
        (Some("turnstile"), Some("ts-pub")),
        "{:?}",
        cfg.body
    );
    assert!(cfg.body.get("secret_key").is_none(), "secret never exposed");

    let r = call(&app, "POST", "/api/v1/auth/login", Some(login.clone()), &[]).await;
    assert_eq!(r.body["error"]["code"], "error.captcha_required");
    mock.push(
        200,
        r#"{"success":false,"error-codes":["invalid-input-response"]}"#,
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        &[("x-captcha-token", "bad")],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.captcha_failed");
    mock.push(503, "down");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        &[("x-captcha-token", "t")],
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.captcha_unavailable",
        "fails closed when the provider is down"
    );
    mock.push(200, r#"{"success":true}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        &[("x-captcha-token", "good-ts")],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let sent = mock.requests();
    assert!(sent.iter().any(|q| q.url
        == "https://challenges.cloudflare.com/turnstile/v0/siteverify"
        && q.body.contains("secret=ts-sec")
        && q.body.contains("response=good-ts")));

    // hCaptcha: enterprise risk score is inverted
    call(
        &app,
        "PUT",
        "/api/v1/admin/addons/captcha-hcaptcha/settings",
        Some(json!({"site_key": "hc-pub", "secret_key": "hc-sec", "min_score": "0.5"})),
        &admin.h(),
    )
    .await;
    let r = call(&app, "PUT", "/api/v1/admin/security/captcha", Some(json!({"mode": "always", "provider": "captcha-hcaptcha", "forms": ["login", "register"]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    mock.push(200, r#"{"success":true,"score":0.8}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        &[("x-captcha-token", "risky")],
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.captcha_failed",
        "risk 0.8 = confidence 0.2"
    );
    mock.push(200, r#"{"success":true}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        &[("x-captcha-token", "free-tier")],
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "no score on free tier passes: {:?}",
        r.body
    );
    assert!(mock.requests().iter().any(
        |q| q.url == "https://api.hcaptcha.com/siteverify" && q.body.contains("secret=hc-sec")
    ));

    // SSO (GitHub) while captcha guards login+register
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/addons/sso-github/settings",
        Some(json!({"client_id": "gh-id", "client_secret": "gh-sec", "allow_register": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let prov = call(&app, "GET", "/api/v1/auth/oauth/providers", None, &[]).await;
    assert!(
        prov.body["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "sso-github"),
        "{:?}",
        prov.body
    );
    let st = call(
        &app,
        "GET",
        "/api/v1/auth/oauth/sso-github/start?format=json",
        None,
        &[],
    )
    .await;
    let url = st.body["url"].as_str().unwrap().to_string();
    assert!(
        url.starts_with("https://github.com/login/oauth/authorize")
            && url.contains("client_id=gh-id"),
        "{url}"
    );
    let gh_mail = email();
    let before = mock.requests().len();
    mock.push(200, r#"{"access_token":"gho","token_type":"bearer"}"#);
    mock.push(
        200,
        json!({"id": rand_id(), "login": "octo", "email": null}).to_string(),
    );
    mock.push(200, json!([{"email": "other@x.example", "primary": false, "verified": true}, {"email": gh_mail, "primary": true, "verified": true}]).to_string());
    let cb = call(
        &app,
        "GET",
        &format!(
            "/api/v1/auth/oauth/sso-github/callback?code=gc&state={}",
            state_of(&url)
        ),
        None,
        &[],
    )
    .await;
    assert_eq!(cb.status, StatusCode::SEE_OTHER);
    let cookie = cb
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .map(|c| c.split(';').next().unwrap().to_string())
        .expect("SSO session without captcha token");
    let me = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("cookie", cookie.as_str())],
    )
    .await;
    assert_eq!(
        me.body["user"]["email"]
            .as_str()
            .or(me.body["email"].as_str()),
        Some(gh_mail.as_str()),
        "{:?}",
        me.body
    );
    let calls: Vec<String> = mock.requests()[before..]
        .iter()
        .map(|q| q.url.clone())
        .collect();
    assert_eq!(
        calls,
        vec![
            "https://github.com/login/oauth/access_token",
            "https://api.github.com/user",
            "https://api.github.com/user/emails"
        ],
        "no siteverify during SSO"
    );

    // disabling the addon hides and blocks the provider
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-github/disable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());
    let prov = call(&app, "GET", "/api/v1/auth/oauth/providers", None, &[]).await;
    assert!(prov.body["providers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["id"] != "sso-github"));
    let st = call(
        &app,
        "GET",
        "/api/v1/auth/oauth/sso-github/start?format=json",
        None,
        &[],
    )
    .await;
    assert_eq!(st.status, StatusCode::NOT_FOUND);
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-github/enable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());

    // Reset global policy for other flows.
    for id in ["captcha-turnstile", "captcha-hcaptcha"] {
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/addons/{id}/settings"),
            Some(json!({"site_key": "", "secret_key": ""})),
            &admin.h(),
        )
        .await;
    }
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/captcha",
        Some(json!({"mode": "never", "provider": "", "forms": []})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
}

fn rand_id() -> i64 {
    (uuid::Uuid::new_v4().as_u128() % 1_000_000_000_000) as i64
}
