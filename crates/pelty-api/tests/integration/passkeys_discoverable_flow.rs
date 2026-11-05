//! Usernameless passkey sign-in (discoverable credentials / conditional UI)
//! for a customer and a staff member. SoftPasskey has no credential storage,
//! so the test plays the resident-key part itself: it tells the authenticator
//! which credential to use (as a platform authenticator would pick it) and
//! returns the stored user handle; the signature is still SoftPasskey's.
use crate::common;
use axum::http::StatusCode;
use base64::Engine;
use common::*;
use serde_json::{json, Value};
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

fn origin(st: &pelty_api::AppState) -> Url {
    Url::parse(&st.cfg.public_url).unwrap()
}

/// Enrol; returns the credential id (base64url).
async fn enrol(app: &axum::Router, st: &pelty_api::AppState, s: &Sess, key: &mut SoftPasskey) -> String {
    let r = call(app, "POST", "/api/v1/auth/passkeys/register/start", Some(json!({"password": "Very-Long-Pass-123"})), &s.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["options"]["publicKey"]["authenticatorSelection"]["residentKey"], "preferred");
    let opts: CreationChallengeResponse = serde_json::from_value(r.body["options"].clone()).unwrap();
    let cred = key.do_registration(origin(st), opts).unwrap();
    let id = serde_json::to_value(&cred).unwrap()["id"].as_str().unwrap().to_string();
    let f = call(app, "POST", "/api/v1/auth/passkeys/register/finish", Some(json!({"challenge_id": r.body["challenge_id"], "name": "Key", "credential": cred})), &s.h()).await;
    assert_eq!(f.status, StatusCode::OK, "{:?}", f.body);
    id
}

async fn user_handle(st: &pelty_api::AppState, email: &str) -> String {
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(email).fetch_one(&st.pool).await.unwrap();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(id.as_bytes())
}

/// A resident-key assertion for a fresh discover challenge.
async fn assert_with(app: &axum::Router, st: &pelty_api::AppState, key: &mut SoftPasskey, cred_id: &str, handle: &str) -> Value {
    let ds = call(app, "POST", "/api/v1/auth/passkeys/discover/start", None, &[]).await;
    assert_eq!(ds.status, StatusCode::OK, "{:?}", ds.body);
    let allow = &ds.body["options"]["publicKey"]["allowCredentials"];
    assert!(allow.is_null() || allow.as_array().is_some_and(Vec::is_empty), "no credential hint: {allow}");
    let mut o = ds.body["options"].clone();
    o["publicKey"]["allowCredentials"] = json!([{"type": "public-key", "id": cred_id}]);
    let opts: RequestChallengeResponse = serde_json::from_value(o).unwrap();
    let mut a = serde_json::to_value(key.do_authentication(origin(st), opts).unwrap()).unwrap();
    a["response"]["userHandle"] = json!(handle);
    json!({"challenge_id": ds.body["challenge_id"], "credential": a})
}

#[tokio::test]
async fn usernameless_passkey_login_customer_and_staff() {
    let (app, st) = app().await;
    let alice = sess(&app, &st, "customer").await;
    let staff = sess(&app, &st, "admin").await;
    let mut ka = SoftPasskey::new(true);
    let mut ks = SoftPasskey::new(true);
    let ca = enrol(&app, &st, &alice, &mut ka).await;
    let cs = enrol(&app, &st, &staff, &mut ks).await;
    let ha = user_handle(&st, &alice.email).await;
    let hs = user_handle(&st, &staff.email).await;

    // Customer: no e-mail sent anywhere.
    let body = assert_with(&app, &st, &mut ka, &ca, &ha).await;
    let r = call(&app, "POST", "/api/v1/auth/passkeys/discover/finish", Some(body.clone()), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["email"], alice.email.as_str());
    assert!(r.cookies.iter().any(|c| c.starts_with("pelty_session=")));
    let disc: Option<bool> = sqlx::query_scalar("SELECT discoverable FROM passkeys WHERE cred_id = $1").bind(&ca).fetch_one(&st.pool).await.unwrap();
    assert_eq!(disc, Some(true));
    // Single-use challenge.
    assert_eq!(call(&app, "POST", "/api/v1/auth/passkeys/discover/finish", Some(body), &[]).await.status, StatusCode::UNAUTHORIZED);

    // Staff member.
    let body = assert_with(&app, &st, &mut ks, &cs, &hs).await;
    let r = call(&app, "POST", "/api/v1/auth/passkeys/discover/finish", Some(body), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["email"], staff.email.as_str());

    // Alice's key claiming to be the staff member: rejected.
    let body = assert_with(&app, &st, &mut ka, &ca, &hs).await;
    assert_eq!(call(&app, "POST", "/api/v1/auth/passkeys/discover/finish", Some(body), &[]).await.status, StatusCode::UNAUTHORIZED);
    // Unknown user handle.
    let body = assert_with(&app, &st, &mut ka, &ca, &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(uuid::Uuid::new_v4().as_bytes())).await;
    assert_eq!(call(&app, "POST", "/api/v1/auth/passkeys/discover/finish", Some(body), &[]).await.status, StatusCode::UNAUTHORIZED);
    // Challenge kinds don't mix: a discover challenge can't finish an e-mail login.
    let body = assert_with(&app, &st, &mut ka, &ca, &ha).await;
    assert_eq!(call(&app, "POST", "/api/v1/auth/passkeys/login/finish", Some(body), &[]).await.status, StatusCode::UNAUTHORIZED);
    // Disabled account can't sign in.
    sqlx::query("UPDATE users SET is_active = false WHERE email = $1").bind(&alice.email).execute(&st.pool).await.unwrap();
    let body = assert_with(&app, &st, &mut ka, &ca, &ha).await;
    assert_eq!(call(&app, "POST", "/api/v1/auth/passkeys/discover/finish", Some(body), &[]).await.status, StatusCode::UNAUTHORIZED);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'auth.passkey_login' AND metadata->>'discoverable' = 'true' AND actor_id IN (SELECT id FROM users WHERE email = ANY($1))")
        .bind(vec![alice.email.clone(), staff.email.clone()]).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n, 2);
}
