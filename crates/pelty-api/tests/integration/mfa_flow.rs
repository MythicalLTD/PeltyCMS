use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

fn code(state: &pelty_api::AppState, enc: &[u8], offset: i64) -> String {
    let secret = state.secret_box.as_ref().unwrap().open(enc).unwrap();
    pelty_auth::totp::code_at(
        &secret,
        time::OffsetDateTime::now_utc().unix_timestamp() + offset,
    )
}

async fn secret_enc(state: &pelty_api::AppState, email: &str) -> Vec<u8> {
    sqlx::query_scalar(
        "SELECT m.secret_enc FROM user_mfa m JOIN users u ON u.id = m.user_id WHERE u.email = $1",
    )
    .bind(email)
    .fetch_one(&state.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn totp_enrol_login_backup_disable() {
    let (app, state) = app().await;
    let s = sess(&app, &state, "customer").await;
    let st = call(&app, "GET", "/api/v1/auth/mfa", None, &s.h()).await;
    assert_eq!(st.body["enabled"], false);
    assert_eq!(st.body["available"], true);

    // Enable needs setup first, then a valid code.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/enable",
        Some(json!({"code": "000000"})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/auth/mfa/setup", None, &s.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["otpauth_url"]
        .as_str()
        .unwrap()
        .starts_with("otpauth://totp/"));
    let enc = secret_enc(&state, &s.email).await;
    assert_ne!(
        enc,
        r.body["secret"].as_str().unwrap().as_bytes(),
        "stored sealed"
    );
    let bad = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/enable",
        Some(json!({"code": "12345x"})),
        &s.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/enable",
        Some(json!({"code": code(&state, &enc, 0)})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let codes: Vec<String> = r.body["backup_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(codes.len(), 10);
    let me = call(&app, "GET", "/api/v1/auth/me", None, &s.h()).await;
    assert_eq!(me.body["mfa_enabled"], true);
    // Setup cannot overwrite an enabled secret.
    assert_eq!(
        call(&app, "POST", "/api/v1/auth/mfa/setup", None, &s.h())
            .await
            .status,
        StatusCode::CONFLICT
    );

    // Login now needs a second step; no cookie on step one.
    let login = || {
        call(
            &app,
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email": s.email, "password": "Very-Long-Pass-123"})),
            &[],
        )
    };
    let l = login().await;
    assert_eq!(l.status, StatusCode::OK);
    assert_eq!(l.body["mfa_required"], true);
    assert!(l.cookies.iter().all(|c| !c.starts_with("pelty_session=")));
    let ch = l.body["challenge"].as_str().unwrap().to_string();
    // The code used for enabling is burned (replay protection).
    let reuse = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": ch, "code": code(&state, &enc, 0)})),
        &[],
    )
    .await;
    assert_eq!(reuse.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": ch, "code": code(&state, &enc, 30)})),
        &[],
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert!(ok.cookies.iter().any(|c| c.starts_with("pelty_session=")));
    // Challenge is single use.
    let again = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": ch, "code": codes[0]})),
        &[],
    )
    .await;
    assert_eq!(again.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Backup code login, upper-case and without dash; single use.
    let ch = login().await.body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let bc = codes[1].replace('-', "").to_uppercase();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/mfa/verify",
            Some(json!({"challenge": ch, "code": bc})),
            &[]
        )
        .await
        .status,
        StatusCode::OK
    );
    let ch = login().await.body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/mfa/verify",
            Some(json!({"challenge": ch, "code": codes[1]})),
            &[]
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // 5 attempts max per challenge.
    let ch = login().await.body["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..5 {
        call(
            &app,
            "POST",
            "/api/v1/auth/mfa/verify",
            Some(json!({"challenge": ch, "code": "aaaaa-aaaaa"})),
            &[],
        )
        .await;
    }
    let locked = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": ch, "code": codes[2]})),
        &[],
    )
    .await;
    assert_eq!(locked.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(locked.body["error"]["code"], "error.mfa_challenge_expired");

    // Regenerate requires password + code; old codes die.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/backup-codes",
        Some(json!({"password": "wrong", "code": codes[3]})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/backup-codes",
        Some(json!({"password": "Very-Long-Pass-123", "code": codes[3]})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let fresh = r.body["backup_codes"][0].as_str().unwrap().to_string();
    let st = call(&app, "GET", "/api/v1/auth/mfa", None, &s.h()).await;
    assert_eq!(st.body["backup_codes_remaining"], 10);

    // Disable.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/disable",
        Some(json!({"password": "Very-Long-Pass-123", "code": codes[4]})),
        &s.h(),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "old codes invalid"
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/disable",
        Some(json!({"password": "Very-Long-Pass-123", "code": fresh})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let l = login().await;
    assert!(l.body["mfa_required"].is_null());
    assert!(l.cookies.iter().any(|c| c.starts_with("pelty_session=")));
}

#[tokio::test]
async fn mfa_requires_auth_and_csrf() {
    let (app, state) = app().await;
    assert_eq!(
        call(&app, "POST", "/api/v1/auth/mfa/setup", None, &[])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let s = sess(&app, &state, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/setup",
        None,
        &[("cookie", s.cookie.as_str())],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/mfa/verify",
        Some(json!({"challenge": "nope", "code": "123456"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}
