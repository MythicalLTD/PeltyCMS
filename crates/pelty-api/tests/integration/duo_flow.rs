use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_api::duo::{jwt_sign, jwt_verify};
use serde_json::{json, Value};

const CID: &str = "DI0123456789ABCDEFGH";
const SEC: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCD";
const HOST: &str = "api-test1234.duosecurity.com";

fn query_param(url: &str, k: &str) -> String {
    let q = url.split_once('?').unwrap().1;
    form_urlencoded::parse(q.as_bytes())
        .find(|(a, _)| a == k)
        .map(|(_, v)| v.into_owned())
        .unwrap()
}
fn body_param(body: &str, k: &str) -> Option<String> {
    form_urlencoded::parse(body.as_bytes())
        .find(|(a, _)| a == k)
        .map(|(_, v)| v.into_owned())
}
fn id_token(email: &str, nonce: &str, result: &str, secret: &str) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    jwt_sign(
        &json!({"iss": format!("https://{HOST}/oauth/v1/token"), "aud": CID, "exp": now + 300, "iat": now, "preferred_username": email, "nonce": nonce, "auth_result": {"result": result, "status": result}}),
        secret,
    )
}

#[tokio::test]
async fn duo_universal_prompt() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let (a2, s2) = (app.clone(), state.clone());
    let res = tokio::spawn(async move { run(a2, s2).await }).await;
    // Always switch Duo off again: the test DB is shared.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/duo",
        Some(json!({"enabled": false, "client_id": "", "api_host": "", "scope": "staff"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    if let Err(e) = res {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn login(app: &axum::Router, e: &str) -> Resp {
    call(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": e, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await
}

async fn run(app: axum::Router, state: pelty_api::AppState) {
    let mock = pelty_payments::MockTransport::new();
    state.set_http(std::sync::Arc::new(mock.clone()));
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/security/duo", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let base = json!({"enabled": true, "client_id": CID, "api_host": HOST, "scope": "all", "client_secret": SEC});
    for (k, v, code) in [
        ("client_id", "short", "error.duo_client_id"),
        ("api_host", "evil.example.com", "error.duo_api_host"),
        ("scope", "nobody", "error.duo_scope"),
        ("client_secret", "x", "error.duo_client_secret"),
    ] {
        let mut b = base.clone();
        b[k] = json!(v);
        let r = call(
            &app,
            "PUT",
            "/api/v1/admin/security/duo",
            Some(b),
            &admin.h(),
        )
        .await;
        assert_eq!(r.body["error"]["code"], code, "{k}: {:?}", r.body);
    }
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/duo",
        Some(base.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["has_secret"], true);
    assert!(r.body.get("client_secret").is_none() && r.body.get("secret_enc").is_none());
    let stored = pelty_database::settings::get(&state.pool, "duo")
        .await
        .unwrap()
        .unwrap();
    assert!(!stored.to_string().contains(SEC), "secret sealed at rest");
    // Test button = health check with a signed client assertion.
    mock.push(200, r#"{"stat":"OK"}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security/duo/test",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ok"], true, "{:?}", r.body);
    let hc = mock.requests().pop().unwrap();
    assert_eq!(hc.url, format!("https://{HOST}/oauth/v1/health_check"));
    let a = jwt_verify(&body_param(&hc.body, "client_assertion").unwrap(), SEC).unwrap();
    assert_eq!(a["iss"], CID);
    assert_eq!(a["aud"], format!("https://{HOST}/oauth/v1/health_check"));

    // Customer login → Duo required; TOTP route refuses the challenge.
    let user = email();
    state
        .auth
        .register(
            pelty_auth::RegisterInput {
                email: &user,
                password: "Very-Long-Pass-123",
                display_name: "D",
            },
            "customer",
            None,
        )
        .await
        .unwrap();
    let r = login(&app, &user).await;
    assert_eq!(r.body["duo_required"], true, "{:?}", r.body);
    assert!(r.cookies.iter().all(|c| !c.starts_with("pelty_session=")));
    let ch = r.body["challenge"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": ch, "code": "123456"})),
        &[],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.duo_required");

    // Duo down → fail closed.
    mock.push(503, "{}");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/duo/start",
        Some(json!({"challenge": ch})),
        &[],
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.duo_unavailable",
        "{:?}",
        r.body
    );

    // Happy path.
    mock.push(200, r#"{"stat":"OK"}"#);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/duo/start",
        Some(json!({"challenge": ch})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let url = r.body["url"].as_str().unwrap().to_string();
    assert!(url.starts_with(&format!("https://{HOST}/oauth/v1/authorize?")));
    assert_eq!(query_param(&url, "client_id"), CID);
    let req = jwt_verify(&query_param(&url, "request"), SEC)
        .expect("request JWT signed with the client secret");
    assert_eq!(req["duo_uname"], user.as_str());
    assert!(req["redirect_uri"]
        .as_str()
        .unwrap()
        .ends_with("/auth/duo/callback"));
    let (st, nonce) = (
        req["state"].as_str().unwrap().to_string(),
        req["nonce"].as_str().unwrap().to_string(),
    );
    // Challenge is spent.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/duo/start",
        Some(json!({"challenge": ch})),
        &[],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.mfa_challenge_expired");

    mock.push(200, json!({"id_token": id_token(&user, &nonce, "allow", SEC), "access_token": "x", "token_type": "Bearer"}).to_string());
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/duo/finish",
        Some(json!({"state": st, "duo_code": "code-1"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.cookies.iter().any(|c| c.starts_with("pelty_session=")));
    let tr = mock.requests().pop().unwrap();
    assert_eq!(tr.url, format!("https://{HOST}/oauth/v1/token"));
    assert_eq!(
        body_param(&tr.body, "grant_type").as_deref(),
        Some("authorization_code")
    );
    assert_eq!(body_param(&tr.body, "code").as_deref(), Some("code-1"));
    assert!(jwt_verify(&body_param(&tr.body, "client_assertion").unwrap(), SEC).is_some());
    // State replay.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/duo/finish",
        Some(json!({"state": st, "duo_code": "code-1"})),
        &[],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.mfa_challenge_expired");

    // Denied, forged, wrong nonce, wrong user: no session.
    let other = email();
    for (tok_for, n, result, secret) in [
        ("self", "nonce", "deny", SEC),
        (
            "self",
            "nonce",
            "allow",
            "forgedforgedforgedforgedforgedforgedforg",
        ),
        ("self", "bad", "allow", SEC),
        ("other", "nonce", "allow", SEC),
    ] {
        let ch = login(&app, &user).await.body["challenge"]
            .as_str()
            .unwrap()
            .to_string();
        mock.push(200, r#"{"stat":"OK"}"#);
        let url = call(
            &app,
            "POST",
            "/api/v1/auth/duo/start",
            Some(json!({"challenge": ch})),
            &[],
        )
        .await
        .body["url"]
            .as_str()
            .unwrap()
            .to_string();
        let req: Value = jwt_verify(&query_param(&url, "request"), SEC).unwrap();
        let nonce = if n == "nonce" {
            req["nonce"].as_str().unwrap().to_string()
        } else {
            n.to_string()
        };
        let who = if tok_for == "self" {
            user.clone()
        } else {
            other.clone()
        };
        mock.push(
            200,
            json!({"id_token": id_token(&who, &nonce, result, secret)}).to_string(),
        );
        let r = call(
            &app,
            "POST",
            "/api/v1/auth/duo/finish",
            Some(json!({"state": req["state"], "duo_code": "c"})),
            &[],
        )
        .await;
        assert_eq!(
            r.body["error"]["code"], "error.duo_denied",
            "{tok_for}/{n}/{result}: {:?}",
            r.body
        );
        assert!(r.cookies.iter().all(|c| !c.starts_with("pelty_session=")));
    }
    let fails: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs a JOIN users u ON u.id = a.actor_id WHERE u.email = $1 AND a.action = 'auth.duo_failed'").bind(&user).fetch_one(&state.pool).await.unwrap();
    assert_eq!(fails, 4);

    // Scope "staff": customers log in with password only.
    let mut b = base.clone();
    b["scope"] = json!("staff");
    b["client_secret"] = json!("");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/security/duo",
        Some(b),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["has_secret"], true,
        "empty secret keeps the stored one"
    );
    let r = login(&app, &user).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["duo_required"].is_null(), "{:?}", r.body);
}
