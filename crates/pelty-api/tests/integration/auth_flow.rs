use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn register_login_me_logout() {
    let (app, _) = app().await;
    let e = email();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": e, "password": "Good-Secret-2026!"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let dup = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": e.to_uppercase(), "password": "Good-Secret-2026!"})),
        &[],
    )
    .await;
    assert_eq!(
        dup.status,
        StatusCode::CONFLICT,
        "email must be case-insensitive unique"
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": e, "password": "Good-Secret-2026!"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let set = r
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .expect("cookie");
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax"));
    let cookie = set.split(';').next().unwrap().to_string();
    let csrf = r.body["csrf_token"].as_str().unwrap().to_string();
    let me = call(&app, "GET", "/api/v1/auth/me", None, &[("cookie", &cookie)]).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["roles"], json!(["customer"]));
    let out = call(
        &app,
        "POST",
        "/api/v1/auth/logout",
        None,
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
    )
    .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    let me = call(&app, "GET", "/api/v1/auth/me", None, &[("cookie", &cookie)]).await;
    assert_eq!(
        me.status,
        StatusCode::UNAUTHORIZED,
        "revoked session must be rejected"
    );
}

#[tokio::test]
async fn generic_login_errors() {
    let (app, state) = app().await;
    let (_, _, e) = login_new(&app, &state, "customer").await;
    let wrong = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": e, "password": "nope-nope-nope"})),
        &[],
    )
    .await;
    let unknown = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"email": email(), "password": "nope-nope-nope"})),
        &[],
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.body["error"]["code"], unknown.body["error"]["code"]);
    assert_eq!(
        wrong.body["error"]["message"],
        unknown.body["error"]["message"]
    );
    assert!(unknown.body["error"]["request_id"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn password_policy_enforced() {
    let (app, _) = app().await;
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": email(), "password": "short"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.password_too_short");
}

#[tokio::test]
async fn csrf_required_for_mutations() {
    let (app, state) = app().await;
    let (cookie, csrf, _) = login_new(&app, &state, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/logout",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["error"]["code"], "error.csrf");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/logout",
        None,
        &[("cookie", &cookie), ("x-csrf-token", "bogus")],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/logout",
        None,
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn rbac_admin_endpoints() {
    let (app, state) = app().await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/users", None, &[])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let (cust, _, _) = login_new(&app, &state, "customer").await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/users",
            None,
            &[("cookie", &cust)]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let (adm, _, _) = login_new(&app, &state, "admin").await;
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/users?per_page=1",
        None,
        &[("cookie", &adm)],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["items"].as_array().unwrap().len(), 1);
    assert!(r.body["items"][0].get("password_hash").is_none());
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/audit-logs",
        None,
        &[("cookie", &adm)],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let (staff, _, _) = login_new(&app, &state, "staff").await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/audit-logs",
            None,
            &[("cookie", &staff)]
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn sessions_list_and_revoke() {
    let (app, state) = app().await;
    let (cookie, csrf, _) = login_new(&app, &state, "customer").await;
    let r = call(
        &app,
        "GET",
        "/api/v1/auth/sessions",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    let items = r.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["current"], true);
    assert!(items[0].get("token_hash").is_none() && items[0].get("csrf_token").is_none());
    let id = items[0]["id"].as_str().unwrap();
    let r = call(
        &app,
        "DELETE",
        &format!("/api/v1/auth/sessions/{id}"),
        None,
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", None, &[("cookie", &cookie)])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn auth_rate_limited() {
    let (app, _) = app_with(3).await;
    let mut last = StatusCode::OK;
    for _ in 0..5 {
        last = call(
            &app,
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email": "x@y.zz", "password": "whatever-123"})),
            &[],
        )
        .await
        .status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn audit_log_is_append_only() {
    let (_, state) = app().await;
    pelty_database::audit::record(&state.pool, None, "test.event", None, None, json!({}))
        .await
        .unwrap();
    let upd = sqlx_exec(
        &state,
        "UPDATE audit_logs SET action = 'tampered' WHERE action = 'test.event'",
    )
    .await;
    assert!(upd.is_err());
    let del = sqlx_exec(&state, "DELETE FROM audit_logs").await;
    assert!(del.is_err());
}

async fn sqlx_exec(state: &pelty_api::AppState, q: &str) -> Result<(), ()> {
    pelty_database::raw_execute(&state.pool, q)
        .await
        .map(|_| ())
        .map_err(|_| ())
}

#[tokio::test]
async fn errors_are_translated() {
    let (app, _) = app().await;
    let r = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("accept-language", "ro-RO,ro;q=0.9")],
    )
    .await;
    assert_eq!(r.body["error"]["message"], "Trebuie să te autentifici.");
    let r = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("cookie", "pelty_lang=en"), ("accept-language", "ro")],
    )
    .await;
    assert_eq!(r.body["error"]["message"], "You need to sign in.");
    let r = call(&app, "GET", "/api/v1/nope", None, &[]).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.body["error"]["code"], "error.not_found");
}

#[tokio::test]
async fn health_and_openapi() {
    let (app, _) = app().await;
    assert_eq!(
        call(&app, "GET", "/readyz", None, &[]).await.body["database"],
        "ok"
    );
    let r = call(&app, "GET", "/api/v1/openapi.json", None, &[]).await;
    assert!(r.body["paths"]["/api/v1/auth/login"].is_object());
}

/// A WHMCS hash carried over by `pelty-server import whmcs` signs in with
/// the old password once and is replaced by Argon2id.
#[tokio::test]
async fn imported_whmcs_hash_logs_in_and_is_rehashed() {
    let (app, state) = app().await;
    let e = email();
    state
        .auth
        .register(
            pelty_auth::RegisterInput {
                email: &e,
                password: "Placeholder-Pass-1",
                display_name: "W",
            },
            "client",
            None,
        )
        .await
        .expect("register");
    // md5("a1#Zx" . "Bob-Old-Pass") ":" salt, the pre-bcrypt WHMCS format
    sqlx::query("UPDATE users SET password_hash='whmcs:d85860ab87f9b0dd4ba592810381a625:a1#Zx' WHERE email=$1")
        .bind(&e)
        .execute(&state.pool)
        .await
        .unwrap();
    let login = |pw: &'static str| {
        let (app, e) = (app.clone(), e.clone());
        async move {
            call(
                &app,
                "POST",
                "/api/v1/auth/login",
                Some(json!({"email": e, "password": pw})),
                &[],
            )
            .await
            .status
        }
    };
    assert_eq!(login("bob-old-pass").await, StatusCode::UNAUTHORIZED);
    assert_eq!(login("Bob-Old-Pass").await, StatusCode::OK);
    let h: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE email=$1")
        .bind(&e)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert!(h.starts_with("$argon2id$"), "rehashed on login: {h}");
    assert_eq!(login("Bob-Old-Pass").await, StatusCode::OK);
}
