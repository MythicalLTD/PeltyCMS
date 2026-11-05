//! Passkeys end to end with a software authenticator (webauthn-authenticator-rs
//! SoftPasskey, user verification on): enrol behind password re-entry, login,
//! counter/last-used update, replay rejection, decoy for unknown e-mails,
//! step-up re-auth, removal and staff revocation.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

fn origin(st: &pelty_api::AppState) -> Url {
    Url::parse(&st.cfg.public_url).unwrap()
}

async fn enrol(
    app: &axum::Router,
    st: &pelty_api::AppState,
    s: &Sess,
    key: &mut SoftPasskey,
    name: &str,
) -> Value {
    let r = call(
        app,
        "POST",
        "/api/v1/auth/passkeys/register/start",
        Some(json!({"password": "Very-Long-Pass-123"})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let opts: CreationChallengeResponse =
        serde_json::from_value(r.body["options"].clone()).unwrap();
    let cred = key.do_registration(origin(st), opts).unwrap();
    call(
        app,
        "POST",
        "/api/v1/auth/passkeys/register/finish",
        Some(json!({"challenge_id": r.body["challenge_id"], "name": name, "credential": cred})),
        &s.h(),
    )
    .await
    .body
}

#[tokio::test]
async fn passkey_enrol_login_reauth_and_revoke() {
    let (app, st) = app().await;
    let alice = sess(&app, &st, "customer").await;
    let admin = sess(&app, &st, "admin").await;
    let mut key = SoftPasskey::new(true);

    // Enrolment needs the password again.
    let bad = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/register/start",
        Some(json!({"password": "wrong"})),
        &alice.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.invalid_password");
    let added = enrol(&app, &st, &alice, &mut key, "Laptop").await;
    let pk_id = added["id"].as_str().expect("id").to_string();
    let list = call(&app, "GET", "/api/v1/auth/passkeys", None, &alice.h())
        .await
        .body;
    assert_eq!(
        (
            list["items"].as_array().unwrap().len(),
            list["items"][0]["name"].as_str()
        ),
        (1, Some("Laptop"))
    );
    let stored: Value = sqlx::query_scalar("SELECT credential FROM passkeys WHERE id = $1::uuid")
        .bind(&pk_id)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert!(!stored.to_string().contains("private"), "public key only");

    // Re-registering the same authenticator is excluded by the RP options.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/register/start",
        Some(json!({"password": "Very-Long-Pass-123"})),
        &alice.h(),
    )
    .await;
    assert_eq!(
        r.body["options"]["publicKey"]["excludeCredentials"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    // Passwordless login.
    let ls = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/start",
        Some(json!({"email": alice.email})),
        &[],
    )
    .await;
    assert_eq!(ls.status, StatusCode::OK, "{:?}", ls.body);
    let opts: RequestChallengeResponse =
        serde_json::from_value(ls.body["options"].clone()).unwrap();
    let assertion = key.do_authentication(origin(&st), opts).unwrap();
    let body = json!({"challenge_id": ls.body["challenge_id"], "credential": assertion});
    let lf = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/finish",
        Some(body.clone()),
        &[],
    )
    .await;
    assert_eq!(lf.status, StatusCode::OK, "{:?}", lf.body);
    assert_eq!(lf.body["email"], alice.email.as_str(), "{:?}", lf.body);
    assert!(lf.cookies.iter().any(|c| c.starts_with("pelty_session=")));
    let used: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT last_used_at FROM passkeys WHERE id = $1::uuid")
            .bind(&pk_id)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert!(used.is_some());
    // Single-use challenge: replaying the same assertion fails.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/passkeys/login/finish",
            Some(body),
            &[]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    // Unknown e-mail: same response shape (decoy, stable id), login impossible.
    let d1 = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/start",
        Some(json!({"email": "nobody@example.com"})),
        &[],
    )
    .await
    .body;
    let d2 = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/start",
        Some(json!({"email": "Nobody@example.com"})),
        &[],
    )
    .await
    .body;
    assert_eq!(
        d1["options"]["publicKey"]["allowCredentials"],
        d2["options"]["publicKey"]["allowCredentials"]
    );
    assert_eq!(
        d1["options"]["publicKey"]["allowCredentials"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_ne!(
        d1["options"]["publicKey"]["challenge"],
        d2["options"]["publicKey"]["challenge"]
    );
    // An assertion from alice's key against the decoy challenge is rejected.
    let ls2 = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/start",
        Some(json!({"email": alice.email})),
        &[],
    )
    .await
    .body;
    let a2 = key
        .do_authentication(
            origin(&st),
            serde_json::from_value(ls2["options"].clone()).unwrap(),
        )
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/passkeys/login/finish",
            Some(json!({"challenge_id": d1["challenge_id"], "credential": a2})),
            &[]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    // A different authenticator can't answer alice's challenge.
    let mut other = SoftPasskey::new(true);
    let ls3 = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/start",
        Some(json!({"email": alice.email})),
        &[],
    )
    .await
    .body;
    assert!(other
        .do_authentication(
            origin(&st),
            serde_json::from_value(ls3["options"].clone()).unwrap()
        )
        .is_err());

    // Step-up re-auth with the passkey.
    let sid: uuid::Uuid = sqlx::query_scalar("UPDATE sessions SET reauth_at = NULL, created_at = now() - interval '2 hours' WHERE user_id = (SELECT id FROM users WHERE email = $1) AND revoked_at IS NULL RETURNING id").bind(&alice.email).fetch_all(&st.pool).await.unwrap().into_iter().next().unwrap();
    let rs = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/reauth/start",
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(rs.status, StatusCode::OK, "{:?}", rs.body);
    let a = key
        .do_authentication(
            origin(&st),
            serde_json::from_value(rs.body["options"].clone()).unwrap(),
        )
        .unwrap();
    let rf = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/reauth/finish",
        Some(json!({"challenge_id": rs.body["challenge_id"], "credential": a})),
        &alice.h(),
    )
    .await;
    assert_eq!(rf.status, StatusCode::NO_CONTENT, "{:?}", rf.body);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = (SELECT user_id FROM sessions WHERE id = $1) AND reauth_at > now() - interval '1 minute'").bind(sid).fetch_one(&st.pool).await.unwrap();
    assert!(n >= 1);
    // Someone else's challenge id can't be used.
    let rs = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/reauth/start",
        None,
        &alice.h(),
    )
    .await
    .body;
    let bob = sess(&app, &st, "customer").await;
    let a = key
        .do_authentication(
            origin(&st),
            serde_json::from_value(rs["options"].clone()).unwrap(),
        )
        .unwrap();
    let x = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/reauth/finish",
        Some(json!({"challenge_id": rs["challenge_id"], "credential": a})),
        &bob.h(),
    )
    .await;
    assert_eq!(x.body["error"]["code"], "error.passkey_challenge_expired");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/passkeys/reauth/start",
            None,
            &bob.h()
        )
        .await
        .body["error"]["code"],
        "error.no_passkeys"
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/auth/passkeys/{pk_id}"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Staff list + revoke; customers can't.
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&alice.email)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/users/{uid}/passkeys"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/users/{uid}/passkeys"),
            None,
            &admin.h()
        )
        .await
        .body["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let mut key2 = SoftPasskey::new(true);
    let second = enrol(&app, &st, &alice, &mut key2, "Phone").await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/auth/passkeys/{}", second["id"].as_str().unwrap()),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let rv = call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/users/{uid}/passkeys"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(rv.body["removed"], 1, "{:?}", rv.body);
    // After revocation the old key can't sign in (decoy options only).
    let ls = call(
        &app,
        "POST",
        "/api/v1/auth/passkeys/login/start",
        Some(json!({"email": alice.email})),
        &[],
    )
    .await
    .body;
    assert!(key
        .do_authentication(
            origin(&st),
            serde_json::from_value(ls["options"].clone()).unwrap()
        )
        .is_err());
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action IN ('auth.passkey_added','auth.passkey_login','auth.passkeys_revoked')").fetch_one(&st.pool).await.unwrap();
    assert!(audit >= 3);
}
