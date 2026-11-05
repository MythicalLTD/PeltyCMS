//! Passkeys as a second factor: opt-in behind password re-entry, password
//! login → passkey step, passkey as an alternative to TOTP, last-key guard,
//! staff revocation resetting the opt-in, staff list reporting 2FA.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

const PW: &str = "Very-Long-Pass-123";

fn origin(st: &pelty_api::AppState) -> Url {
    Url::parse(&st.cfg.public_url).unwrap()
}

async fn enrol(
    app: &axum::Router,
    st: &pelty_api::AppState,
    s: &Sess,
    key: &mut SoftPasskey,
    code: Option<String>,
) -> String {
    let body = match code {
        Some(c) => json!({"password": PW, "code": c}),
        None => json!({"password": PW}),
    };
    let r = call(
        app,
        "POST",
        "/api/v1/auth/passkeys/register/start",
        Some(body),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let opts: CreationChallengeResponse =
        serde_json::from_value(r.body["options"].clone()).unwrap();
    let cred = key.do_registration(origin(st), opts).unwrap();
    let f = call(
        app,
        "POST",
        "/api/v1/auth/passkeys/register/finish",
        Some(json!({"challenge_id": r.body["challenge_id"], "name": "Key", "credential": cred})),
        &s.h(),
    )
    .await;
    assert_eq!(f.status, StatusCode::OK, "{:?}", f.body);
    f.body["id"].as_str().unwrap().to_string()
}

async fn password_login(app: &axum::Router, email: &str) -> Value {
    let r = call(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": email, "password": PW})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

/// Runs the passkey second step for a pending login challenge.
async fn passkey_step(
    app: &axum::Router,
    st: &pelty_api::AppState,
    key: &mut SoftPasskey,
    challenge: &Value,
) -> Resp {
    let s = call(
        app,
        "POST",
        "/api/v1/auth/mfa/passkey/start",
        Some(json!({"challenge": challenge})),
        &[],
    )
    .await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    let opts: RequestChallengeResponse = serde_json::from_value(s.body["options"].clone()).unwrap();
    let a = key.do_authentication(origin(st), opts).unwrap();
    call(app, "POST", "/api/v1/auth/mfa/passkey/finish", Some(json!({"challenge": challenge, "challenge_id": s.body["challenge_id"], "credential": a})), &[]).await
}

fn totp_at(st: &pelty_api::AppState, enc: &[u8], offset: i64) -> String {
    let secret = st.secret_box.as_ref().unwrap().open(enc).unwrap();
    pelty_auth::totp::code_at(
        &secret,
        time::OffsetDateTime::now_utc().unix_timestamp() + offset,
    )
}

#[tokio::test]
async fn passkey_as_second_factor() {
    let (app, st) = app().await;
    let alice = sess(&app, &st, "customer").await;
    let admin = sess(&app, &st, "admin").await;
    let mut key = SoftPasskey::new(true);

    // Default: a passkey alone does not turn on 2FA.
    let k1 = enrol(&app, &st, &alice, &mut key, None).await;
    let lg = password_login(&app, &alice.email).await;
    assert!(lg.get("mfa_required").is_none(), "{lg:?}");
    let s0 = call(&app, "GET", "/api/v1/auth/mfa/passkey", None, &alice.h())
        .await
        .body;
    assert_eq!(
        (
            s0["enabled"].as_bool(),
            s0["passkeys"].as_i64(),
            s0["totp"].as_bool()
        ),
        (Some(false), Some(1), Some(false))
    );

    // Opt-in needs the password.
    let bad = call(
        &app,
        "PUT",
        "/api/v1/auth/mfa/passkey",
        Some(json!({"enabled": true, "password": "nope"})),
        &alice.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.invalid_password");
    let on = call(
        &app,
        "PUT",
        "/api/v1/auth/mfa/passkey",
        Some(json!({"enabled": true, "password": PW})),
        &alice.h(),
    )
    .await;
    assert_eq!(on.status, StatusCode::OK, "{:?}", on.body);
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", None, &alice.h())
            .await
            .body["passkey_2fa"],
        true
    );

    // Password login now stops at the passkey step.
    let lg = password_login(&app, &alice.email).await;
    assert_eq!(lg["mfa_required"], true, "{lg:?}");
    assert_eq!(lg["methods"], json!(["passkey"]));
    assert!(lg.get("email").is_none());
    // TOTP verify can't complete it.
    let tv = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": lg["challenge"], "code": "123456"})),
        &[],
    )
    .await;
    assert_ne!(tv.status, StatusCode::OK);
    // A different authenticator can't answer.
    let mut other = SoftPasskey::new(true);
    let s = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/passkey/start",
        Some(json!({"challenge": lg["challenge"]})),
        &[],
    )
    .await
    .body;
    assert!(other
        .do_authentication(
            origin(&st),
            serde_json::from_value(s["options"].clone()).unwrap()
        )
        .is_err());
    let done = passkey_step(&app, &st, &mut key, &lg["challenge"]).await;
    assert_eq!(done.status, StatusCode::OK, "{:?}", done.body);
    assert_eq!(done.body["email"], alice.email.as_str());
    assert!(done.cookies.iter().any(|c| c.starts_with("pelty_session=")));
    // The login challenge is single use.
    let again = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/passkey/start",
        Some(json!({"challenge": lg["challenge"]})),
        &[],
    )
    .await;
    assert_eq!(again.body["error"]["code"], "error.mfa_challenge_expired");
    // Bogus challenge.
    let bogus = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/passkey/start",
        Some(json!({"challenge": "x".repeat(40)})),
        &[],
    )
    .await;
    assert_eq!(bogus.body["error"]["code"], "error.mfa_challenge_expired");

    // Last passkey while it is the only 2FA: refused.
    let del = call(
        &app,
        "DELETE",
        &format!("/api/v1/auth/passkeys/{k1}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(
        del.body["error"]["code"], "error.passkey_2fa_last",
        "{:?}",
        del.body
    );
    // With a second key the first can go.
    let mut key2 = SoftPasskey::new(true);
    let k2 = enrol(&app, &st, &alice, &mut key2, None).await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/auth/passkeys/{k1}"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let del = call(
        &app,
        "DELETE",
        &format!("/api/v1/auth/passkeys/{k2}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(del.body["error"]["code"], "error.passkey_2fa_last");

    // Staff revocation clears the keys and the opt-in → password-only again.
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&alice.email)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    let rv = call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/users/{uid}/passkeys"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(rv.status, StatusCode::OK, "{:?}", rv.body);
    assert!(!pelty_database::mfa::passkey_2fa(&st.pool, uid)
        .await
        .unwrap());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM user_passkey_2fa WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert!(password_login(&app, &alice.email)
        .await
        .get("mfa_required")
        .is_none());
    // Opt-in without any passkey is refused.
    let r = call(
        &app,
        "PUT",
        "/api/v1/auth/mfa/passkey",
        Some(json!({"enabled": true, "password": PW})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.no_passkeys");

    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE actor_id = $1 AND action IN ('auth.passkey_2fa_enabled','auth.mfa_verified')").bind(uid).fetch_one(&st.pool).await.unwrap();
    assert!(audit >= 2, "{audit}");
}

#[tokio::test]
async fn passkey_alternative_to_totp_and_staff_status() {
    let (app, st) = app().await;
    let s = sess(&app, &st, "admin").await;
    let admin = sess(&app, &st, "admin").await;
    // TOTP on (the next window's code is used later; the current one is spent).
    assert_eq!(
        call(&app, "POST", "/api/v1/auth/mfa/setup", None, &s.h())
            .await
            .status,
        StatusCode::OK
    );
    let enc: Vec<u8> = sqlx::query_scalar(
        "SELECT m.secret_enc FROM user_mfa m JOIN users u ON u.id = m.user_id WHERE u.email = $1",
    )
    .bind(&s.email)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/enable",
        Some(json!({"code": totp_at(&st, &enc, 0)})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let lg = password_login(&app, &s.email).await;
    assert_eq!(lg["methods"], json!(["totp"]));
    // No passkey yet → passkey step refused.
    let x = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/passkey/start",
        Some(json!({"challenge": lg["challenge"]})),
        &[],
    )
    .await;
    assert_eq!(x.body["error"]["code"], "error.no_passkeys");

    // With a passkey enrolled, TOTP users may use it instead of the code (no opt-in needed).
    let mut key = SoftPasskey::new(true);
    let k = enrol(&app, &st, &s, &mut key, Some(totp_at(&st, &enc, 30))).await;
    let lg = password_login(&app, &s.email).await;
    assert_eq!(lg["methods"], json!(["totp", "passkey"]));
    let done = passkey_step(&app, &st, &mut key, &lg["challenge"]).await;
    assert_eq!(done.status, StatusCode::OK, "{:?}", done.body);
    // Removing the only passkey is fine while TOTP covers 2FA.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/auth/passkeys/{k}"),
            None,
            &s.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // Staff list counts passkey 2FA (opt-in + key) for a staff member without TOTP.
    let b = sess(&app, &st, "admin").await;
    let mut kb = SoftPasskey::new(true);
    enrol(&app, &st, &b, &mut kb, None).await;
    let staff_mfa = |list: &Value, email: &str| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["email"] == email)
            .map(|i| i["mfa_enabled"].clone())
    };
    let l = call(&app, "GET", "/api/v1/admin/staff", None, &admin.h())
        .await
        .body;
    let before = staff_mfa(&l, &b.email);
    let on = call(
        &app,
        "PUT",
        "/api/v1/auth/mfa/passkey",
        Some(json!({"enabled": true, "password": PW})),
        &b.h(),
    )
    .await;
    assert_eq!(on.status, StatusCode::OK, "{:?}", on.body);
    let l = call(&app, "GET", "/api/v1/admin/staff", None, &admin.h())
        .await
        .body;
    assert_eq!(
        (before, staff_mfa(&l, &b.email)),
        (Some(json!(false)), Some(json!(true)))
    );
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&b.email)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(
        pelty_database::mfa::login_methods(&st.pool, uid)
            .await
            .unwrap(),
        vec!["passkey"]
    );
}
