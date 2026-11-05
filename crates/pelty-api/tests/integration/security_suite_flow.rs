//! Captcha, SSO, IP rules, impersonation, policy, merge and WHMCS compat.
//! One test fn: these change global settings that other flows read.
use crate::common;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

async fn call_ip(
    app: &axum::Router,
    method: &str,
    uri: &str,
    ip: &str,
    body: Option<Value>,
    h: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in h {
        b = b.header(*k, *v);
    }
    let mut req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string())),
        None => b.body(Body::empty()),
    }
    .unwrap();
    req.extensions_mut().insert(ConnectInfo::<SocketAddr>(
        format!("{ip}:5000").parse().unwrap(),
    ));
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn form(app: &axum::Router, uri: &str, body: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn loc(cookies: &[String]) -> Option<String> {
    cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .map(|c| c.split(';').next().unwrap().to_string())
}

#[tokio::test]
async fn security_suite() {
    let (app, state) = app().await;
    let mock = pelty_payments::MockTransport::new();
    state.set_http(std::sync::Arc::new(mock.clone()));
    let admin = sess(&app, &state, "admin").await;

    // captcha: internal engine on registration
    // Other flows may have configured Turnstile in the shared DB.
    call(
        &app,
        "PUT",
        "/api/v1/admin/addons/captcha-turnstile/settings",
        Some(json!({"site_key": "", "secret_key": ""})),
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/captcha",
        Some(json!({"mode": "always", "provider": "captcha-turnstile", "forms": ["register"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.captcha_misconfigured",
        "turnstile without secret is refused"
    );
    let r = call(&app, "PUT", "/api/v1/admin/security/captcha", Some(json!({"mode": "always", "provider": "captcha-internal", "forms": ["register", "bogus"]})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.captcha_policy");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/captcha",
        Some(json!({"mode": "always", "provider": "captcha-internal", "forms": ["register"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cfg = call(&app, "GET", "/api/v1/captcha/config", None, &[]).await;
    assert_eq!(
        (cfg.body["enabled"].clone(), cfg.body["engine"].clone()),
        (json!(true), json!("internal"))
    );
    let reg =
        |e: String| json!({"email": e, "password": "Very-Long-Pass-123", "display_name": "C"});
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(reg(email())),
        &[],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.captcha_required");
    let ch = call(&app, "GET", "/api/v1/captcha/challenge", None, &[]).await;
    let q = ch.body["question"].as_str().unwrap().to_string();
    let (a, b) = q.split_once(" + ").unwrap();
    let sum: u32 = a.parse::<u32>().unwrap() + b.parse::<u32>().unwrap();
    let tok = ch.body["token"].as_str().unwrap();
    let wrong = format!("{tok}:{}", sum + 1);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(reg(email())),
        &[("x-captcha-token", wrong.as_str())],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.captcha_failed");
    let right = format!("{tok}:{sum}");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(reg(email())),
        &[("x-captcha-token", right.as_str())],
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(reg(email())),
        &[("x-captcha-token", right.as_str())],
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.captcha_failed",
        "one-time token"
    );

    // captcha: reCAPTCHA v3 via mocked siteverify
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/addons/captcha-recaptcha-v3/settings",
        Some(json!({"site_key": "pub", "secret_key": "sec", "min_score": "0.5"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/captcha",
        Some(json!({"mode": "always", "provider": "captcha-recaptcha-v3", "forms": ["register"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    mock.push(200, r#"{"success":true,"score":0.3}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(reg(email())),
        &[("x-captcha-token", "t1")],
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.captcha_failed",
        "score 0.3 < 0.5"
    );
    mock.push(200, r#"{"success":true,"score":0.9}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(reg(email())),
        &[("x-captcha-token", "t2")],
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let sent = mock.requests();
    assert!(sent
        .iter()
        .any(|q| q.url.contains("recaptcha/api/siteverify")
            && q.body.contains("secret=sec")
            && q.body.contains("response=t2")));
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/captcha",
        Some(json!({"mode": "never", "provider": "", "forms": []})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    // SSO: Google login, register, link and unlink
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/addons/sso-google/settings",
        Some(json!({"client_id": "cid", "client_secret": "csec", "allow_register": true})),
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
            .any(|p| p["id"] == "sso-google"),
        "{:?}",
        prov.body
    );
    let st = call(
        &app,
        "GET",
        "/api/v1/auth/oauth/sso-google/start?format=json",
        None,
        &[],
    )
    .await;
    let url = st.body["url"].as_str().unwrap().to_string();
    assert!(
        url.starts_with("https://accounts.google.com/")
            && url.contains("code_challenge_method=S256")
            && url.contains("client_id=cid"),
        "{url}"
    );
    let state_tok: String = form_urlencoded::parse(url.split_once('?').unwrap().1.as_bytes())
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let sso_mail = email();
    mock.push(200, r#"{"access_token":"at","token_type":"Bearer"}"#);
    mock.push(200, json!({"sub": format!("g-{}", uuid::Uuid::new_v4()), "email": sso_mail, "email_verified": true, "name": "Gina"}).to_string());
    let cb = call(
        &app,
        "GET",
        &format!("/api/v1/auth/oauth/sso-google/callback?code=c1&state={state_tok}"),
        None,
        &[],
    )
    .await;
    assert_eq!(cb.status, StatusCode::SEE_OTHER);
    let cookie = loc(&cb.cookies).expect("session from SSO");
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
        Some(sso_mail.as_str()),
        "{:?}",
        me.body
    );
    // State is single-use.
    let cb = call(
        &app,
        "GET",
        &format!("/api/v1/auth/oauth/sso-google/callback?code=c1&state={state_tok}"),
        None,
        &[],
    )
    .await;
    assert!(cb.cookies.iter().all(|c| !c.starts_with("pelty_session=")));
    let ids = call(
        &app,
        "GET",
        "/api/v1/account/identities",
        None,
        &[("cookie", cookie.as_str())],
    )
    .await;
    let list = ids.body["identities"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    // Staff can't sign in via SSO unless allowed.
    let st = call(
        &app,
        "GET",
        "/api/v1/auth/oauth/sso-google/start?format=json",
        None,
        &[],
    )
    .await;
    let s2: String = form_urlencoded::parse(
        st.body["url"]
            .as_str()
            .unwrap()
            .split_once('?')
            .unwrap()
            .1
            .as_bytes(),
    )
    .find(|(k, _)| k == "state")
    .unwrap()
    .1
    .into_owned();
    mock.push(200, r#"{"access_token":"at"}"#);
    mock.push(200, json!({"sub": format!("ga-{}", uuid::Uuid::new_v4()), "email": admin.email, "email_verified": true}).to_string());
    let cb = call(
        &app,
        "GET",
        &format!("/api/v1/auth/oauth/sso-google/callback?code=c2&state={s2}"),
        None,
        &[],
    )
    .await;
    assert!(
        cb.cookies.iter().all(|c| !c.starts_with("pelty_session=")),
        "existing account not auto-linked"
    );

    // password policy + session settings
    let r = call(&app, "PUT", "/api/v1/admin/security/policy", Some(json!({"password_min_length": 14, "require_symbol": true, "session_idle_minutes": 30, "session_absolute_hours": 12, "login_rate_per_minute": 40})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    pelty_auth::policy::invalidate();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": email(), "password": "Abcdefgh12345", "display_name": "P"})),
        &[],
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "13 chars, no symbol: {:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": email(), "password": "Abcdefgh1234-x", "display_name": "P"})),
        &[],
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    assert_eq!(state.auth_limiter.limit(), 40);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/policy",
        Some(json!({"password_min_length": 7})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/policy",
        Some(json!({})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "reset to defaults: {:?}", r.body);
    pelty_auth::policy::invalidate();

    // IP rules
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/ip-rules",
        Some(json!({"cidr": "not-an-ip", "kind": "ban"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/ip-rules",
        Some(json!({"cidr": "203.0.113.0/24", "kind": "ban", "reason": "abuse"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rule = r.body["id"].as_str().unwrap().to_string();
    let (st, _) = call_ip(
        &app,
        "GET",
        "/api/v1/captcha/config",
        "203.0.113.9",
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call_ip(
        &app,
        "GET",
        "/api/v1/captcha/config",
        "198.51.100.1",
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let r = call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/security/ip-rules/{rule}"),
        None,
        &admin.h(),
    )
    .await;
    assert!(r.status.is_success());
    let (st, _) = call_ip(
        &app,
        "GET",
        "/api/v1/captcha/config",
        "203.0.113.9",
        None,
        &[],
    )
    .await;
    assert_eq!(st, StatusCode::OK, "unbanned");

    // impersonation
    let cust = sess(&app, &state, "customer").await;
    let cid = call(&app, "GET", "/api/v1/client/profile", None, &cust.h()).await;
    let cust_id = cid.body["id"]
        .as_str()
        .or(cid.body["customer"]["id"].as_str())
        .map(str::to_string);
    let cust_id = match cust_id {
        Some(c) => c,
        None => {
            let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
                .bind(&cust.email)
                .fetch_one(&state.pool)
                .await
                .unwrap();
            pelty_database::customers::id_for_user(&state.pool, uid)
                .await
                .unwrap()
                .to_string()
        }
    };
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{cust_id}/login-as"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let imp = loc(&r.cookies).unwrap();
    let ret = r
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_imp_return="))
        .map(|c| c.split(';').next().unwrap().to_string())
        .expect("return cookie");
    let me = call(
        &app,
        "GET",
        "/api/v1/auth/impersonation",
        None,
        &[("cookie", imp.as_str())],
    )
    .await;
    assert_eq!(me.body["active"], true);
    assert_eq!(me.body["admin_email"], admin.email.as_str());
    let both = format!("{imp}; {ret}");
    let csrf = r.body["csrf_token"].as_str().unwrap().to_string();
    let stop = call(
        &app,
        "POST",
        "/api/v1/auth/impersonation/stop",
        None,
        &[("cookie", both.as_str()), ("x-csrf-token", csrf.as_str())],
    )
    .await;
    assert_eq!(stop.body["restored"], true, "{:?}", stop.body);
    let admin_uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&admin.email)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let acts: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_logs WHERE action LIKE 'admin.impersonation%' AND actor_id = $1 ORDER BY id").bind(admin_uid).fetch_all(&state.pool).await.unwrap();
    assert_eq!(acts.len(), 2, "start + end audited: {acts:?}");
    assert_eq!(acts[1], "admin.impersonation_ended");
    // Staff can't be impersonated.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{admin_uid}/login-as"),
        None,
        &admin.h(),
    )
    .await;
    assert!(r.status.is_client_error());

    // close + merge
    let mk =
        |n: &str| json!({"email": email(), "first_name": n, "last_name": "M", "currency": "EUR"});
    let a = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(mk("A")),
        &admin.h(),
    )
    .await;
    let b = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(mk("B")),
        &admin.h(),
    )
    .await;
    let (a, b) = (
        a.body["id"].as_str().unwrap().to_string(),
        b.body["id"].as_str().unwrap().to_string(),
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{a}/notes"),
        Some(json!({"body": "keep me"})),
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{a}/merge"),
        Some(json!({"into": a})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{a}/merge"),
        Some(json!({"into": b})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let notes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM customer_notes WHERE customer_id = $1::uuid")
            .bind(&b)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(-1);
    assert_eq!(notes, 1, "note moved to the surviving customer");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/customers/{b}/close"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["status"], "closed");

    // WHMCS-compatible API
    let k = call(
        &app,
        "POST",
        "/api/v1/api-keys",
        Some(json!({"name": "whmcs", "scopes": ["admin.access", "customers.manage"]})),
        &admin.h(),
    )
    .await;
    let secret = k.body["secret"].as_str().unwrap().to_string();
    let ident = k.body["key"]["prefix"].as_str().unwrap().to_string();
    let (_, v) = form(
        &app,
        "/includes/api.php",
        &format!("action=GetClients&identifier={ident}&secret=wrong&responsetype=json"),
    )
    .await;
    assert_eq!(v["result"], "error");
    let new_mail = email();
    let (_, v) = form(&app, "/includes/api.php", &format!("action=AddClient&identifier={ident}&secret={secret}&firstname=W&lastname=H&email={}&currency=EUR", new_mail.replace('@', "%40"))).await;
    assert_eq!(v["result"], "success", "{v:?}");
    let new_id = v["id"].as_str().unwrap().to_string();
    let (_, v) = form(
        &app,
        "/api/v1/compat/whmcs",
        &format!("action=GetClientsDetails&identifier={ident}&secret={secret}&clientid={new_id}"),
    )
    .await;
    assert_eq!(v["result"], "success");
    assert_eq!(
        v["email"].as_str().or(v["customer"]["email"].as_str()),
        Some(new_mail.as_str()),
        "{v:?}"
    );
    let (_, v) = form(
        &app,
        "/includes/api.php",
        &format!("action=GetClients&identifier={ident}&secret={secret}&limitnum=1"),
    )
    .await;
    assert_eq!(v["numreturned"], 1);
    assert!(v["totalresults"].as_i64().unwrap() >= 3);
    let (_, v) = form(
        &app,
        "/includes/api.php",
        &format!("action=GetInvoices&identifier={ident}&secret={secret}"),
    )
    .await;
    assert_eq!(v["result"], "error", "key lacks billing scope: {v:?}");
    let (_, v) = form(
        &app,
        "/includes/api.php",
        &format!("action=DoesNotExist&identifier={ident}&secret={secret}"),
    )
    .await;
    assert_eq!(v["message"], "Command Not Found");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/addons/integration-whmcs-api/disable",
        None,
        &admin.h(),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let (st, _) = form(
        &app,
        "/includes/api.php",
        &format!("action=GetClients&identifier={ident}&secret={secret}"),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    call(
        &app,
        "POST",
        "/api/v1/admin/addons/integration-whmcs-api/enable",
        None,
        &admin.h(),
    )
    .await;
}
