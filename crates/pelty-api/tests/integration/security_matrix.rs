//! Security regression tests: authorization matrix over every OpenAPI route,
//! and API-key brute-force throttling.
use crate::common;
use axum::http::StatusCode;
use common::*;

fn fill(path: &str) -> String {
    // Replace every {param} with a syntactically valid but unknown value.
    let id = uuid::Uuid::new_v4().to_string();
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}').map(|e| start + e).unwrap_or(rest.len() - 1);
        let name = &rest[start + 1..end];
        let v = match name {
            "id" | "tid" | "cid" | "field" | "sid" | "uid" | "pid" => id.clone(),
            "lang" | "locale" => "en".to_string(),
            _ => {
                if path.contains("/jobs/") { "999999999".into() } else { "x".into() }
            }
        };
        out.push_str(&v);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

#[tokio::test]
async fn authorization_matrix_over_openapi() {
    let (app, state) = app().await;
    let spec = call(&app, "GET", "/api/v1/openapi.json", None, &[]).await.body;
    let paths = spec["paths"].as_object().expect("paths");
    assert!(paths.len() > 50, "spec looks truncated: {}", paths.len());
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let mut checked = 0;
    let mut failures = Vec::new();
    let admin_no_csrf = [("cookie", admin.cookie.as_str())];
    let cust_no_csrf = [("cookie", cust.cookie.as_str())];
    let mut csrf_checked = 0;
    for (path, item) in paths {
        let admin_only = path.starts_with("/api/v1/admin/");
        let client_only = path.starts_with("/api/v1/client/");
        for method in ["get", "post", "put", "patch", "delete"] {
            if item.get(method).is_none() {
                continue;
            }
            let m = method.to_uppercase();
            let uri = fill(path);
            let mutating = m != "GET";
            if !admin_only && !client_only {
                // Other routes: only probe mutations that demand a session,
                // and verify they refuse a session cookie without CSRF.
                if !mutating || path.starts_with("/api/v1/webhooks") {
                    continue;
                }
                let anon = call(&app, &m, &uri, None, &[]).await;
                if m == "POST" && path == "/api/v1/public/support/inbound/{id}" {
                    // This provider endpoint authenticates the timestamp and raw body
                    // with a per-department HMAC, never a browser session. A missing
                    // signature returns 401 but must not imply a CSRF requirement.
                    // Cookies must not substitute for the signature; valid/invalid
                    // signed deliveries are covered by inbound_email_flow.
                    let with_cookie = call(&app, &m, &uri, None, &cust_no_csrf).await;
                    if anon.status != StatusCode::UNAUTHORIZED || with_cookie.status != StatusCode::UNAUTHORIZED {
                        failures.push(format!("signed-inbound {m} {path}: anonymous={} cookie={}", anon.status, with_cookie.status));
                    }
                    continue;
                }
                if anon.status != StatusCode::UNAUTHORIZED {
                    continue; // public endpoint (login, register, reset, ...)
                }
                csrf_checked += 1;
                let r = call(&app, &m, &uri, None, &cust_no_csrf).await;
                if r.status != StatusCode::FORBIDDEN || r.body["error"]["code"] != "error.csrf" {
                    failures.push(format!("no-csrf {m} {path} -> {} {}", r.status, r.body["error"]["code"]));
                }
                continue;
            }
            checked += 1;
            let r = call(&app, &m, &uri, None, &[]).await;
            if r.status != StatusCode::UNAUTHORIZED {
                failures.push(format!("anon {m} {path} -> {}", r.status));
            }
            if mutating {
                // A valid session without the CSRF header must be refused
                // before the handler runs (no fixtures needed).
                csrf_checked += 1;
                let h: &[(&str, &str)] = if admin_only { &admin_no_csrf } else { &cust_no_csrf };
                let r = call(&app, &m, &uri, None, h).await;
                if r.status != StatusCode::FORBIDDEN || r.body["error"]["code"] != "error.csrf" {
                    failures.push(format!("no-csrf {m} {path} -> {} {}", r.status, r.body["error"]["code"]));
                }
            }
            if admin_only {
                let r = call(&app, &m, &uri, None, &cust.h()).await;
                if r.status != StatusCode::FORBIDDEN {
                    failures.push(format!("customer {m} {path} -> {}", r.status));
                }
                // Admin reads only: avoid side effects of admin mutations.
                if m == "GET" {
                    let r = call(&app, &m, &uri, None, &admin.h()).await;
                    if matches!(r.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) || r.status.is_server_error() {
                        failures.push(format!("admin {m} {path} -> {}", r.status));
                    }
                }
            } else if m == "GET" {
                let r = call(&app, &m, &uri, None, &cust.h()).await;
                if matches!(r.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) || r.status.is_server_error() {
                    failures.push(format!("customer {m} {path} -> {}", r.status));
                }
            }
        }
    }
    assert!(csrf_checked > 30, "only {csrf_checked} mutating routes checked for CSRF");
    assert!(checked > 40, "only {checked} routes checked");
    assert!(failures.is_empty(), "authz matrix violations:\n{}", failures.join("\n"));
}

#[tokio::test]
async fn csrf_required_for_session_mutations() {
    let (app, state) = app().await;
    let cust = sess(&app, &state, "customer").await;
    let no_csrf = [("cookie", cust.h()[0].1)];
    let r = call(&app, "POST", "/api/v1/client/tickets/00000000-0000-0000-0000-000000000000/close", None, &no_csrf).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    let bad = [("cookie", cust.h()[0].1), ("x-csrf-token", "nope")];
    let r = call(&app, "POST", "/api/v1/auth/logout", None, &bad).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
}

#[tokio::test]
async fn api_key_guessing_is_rate_limited() {
    let (app, _state) = app_with(3).await;
    let mut last = StatusCode::OK;
    for i in 0..6 {
        let bearer = format!("Bearer pelty_guess{i}xxxxxxxxxxxxxxxxxxxxxxxx");
        last = call(&app, "GET", "/api/v1/admin/customers", None, &[("authorization", bearer.as_str())]).await.status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn security_headers_present() {
    let (app, _s) = app().await;
    let req = axum::http::Request::builder().uri("/healthz").body(axum::body::Body::empty()).unwrap();
    let res = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
    let h = res.headers();
    let csp = h["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("frame-ancestors 'none'") && csp.contains("script-src 'self'"), "{csp}");
    assert_eq!(h["x-frame-options"], "DENY");
    assert_eq!(h["referrer-policy"], "strict-origin-when-cross-origin");
    assert!(h.contains_key("permissions-policy"));
    assert_eq!(h["x-content-type-options"], "nosniff");
    // Tests run with cookie_secure = false: no HSTS over plain HTTP.
    assert!(!h.contains_key("strict-transport-security"));
}

#[tokio::test]
async fn pending_uploads_capped_and_orphans_purged() {
    use pelty_database::ticket_attachments as att;
    let (app, state) = app().await;
    let cust = sess(&app, &state, "customer").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    for _ in 0..att::MAX_PENDING_PER_USER {
        att::create_pending(&state.pool, uid, "a.txt", "text/plain", b"hi").await.unwrap();
    }
    assert!(att::create_pending(&state.pool, uid, "a.txt", "text/plain", b"hi").await.is_err(), "cap must hold");
    sqlx::query("UPDATE ticket_attachments SET created_at = now() - interval '2 days' WHERE uploaded_by = $1").bind(uid).execute(&state.pool).await.unwrap();
    let n = att::purge_orphans(&state.pool, 24).await.unwrap();
    assert!(n >= att::MAX_PENDING_PER_USER as u64);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM ticket_attachments WHERE uploaded_by = $1").bind(uid).fetch_one(&state.pool).await.unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn repeated_failed_logins_lock_account_and_notify_admins() {
    let (app, state) = app().await;
    let _admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let login = |pw: &'static str| {
        let app = app.clone();
        let e = cust.email.clone();
        async move { call(&app, "POST", "/api/v1/auth/login", Some(serde_json::json!({"email": e, "password": pw})), &[]).await.status }
    };
    for _ in 0..pelty_database::users::LOCK_AFTER {
        assert_eq!(login("wrong-password-x").await, StatusCode::UNAUTHORIZED);
    }
    // Locked: even the right password is refused, same answer as a wrong one.
    assert_eq!(login("Very-Long-Pass-123").await, StatusCode::UNAUTHORIZED);
    let notices: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'admin_account_locked' AND payload->'vars'->>'email' = $1")
        .bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    assert!(notices >= 1, "admins must be notified");
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs a JOIN users u ON u.id = a.actor_id WHERE u.email = $1 AND a.action = 'auth.account_locked'")
        .bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    assert_eq!(audit, 1);
    // Lock expires -> correct password works and the counter resets.
    sqlx::query("UPDATE users SET locked_until = now() - interval '1 second' WHERE email = $1").bind(&cust.email).execute(&state.pool).await.unwrap();
    assert_eq!(login("Very-Long-Pass-123").await, StatusCode::OK);
    let n: i32 = sqlx::query_scalar("SELECT failed_logins FROM users WHERE email = $1").bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn breached_password_rejected_on_register() {
    let (app, _s) = app().await;
    let r = call(&app, "POST", "/api/v1/auth/register", Some(serde_json::json!({"email": email(), "password": "Qwertyuiop123", "display_name": "X"})), &[]).await;
    assert_eq!(r.body["error"]["code"], "error.password_breached", "{:?}", r.body);
}

#[tokio::test]
async fn revoke_other_sessions_keeps_current() {
    let (app, state) = app().await;
    let a = sess(&app, &state, "customer").await;
    // Second device for the same user.
    let r = call(&app, "POST", "/api/v1/auth/login", Some(serde_json::json!({"email": a.email, "password": "Very-Long-Pass-123"})), &[]).await;
    let other = r.cookies.iter().find(|c| c.starts_with("pelty_session=")).unwrap().split(';').next().unwrap().to_string();
    let r = call(&app, "DELETE", "/api/v1/auth/sessions", None, &a.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["revoked"].as_u64().unwrap() >= 1);
    assert_eq!(call(&app, "GET", "/api/v1/auth/me", None, &a.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/auth/me", None, &[("cookie", other.as_str())]).await.status, StatusCode::UNAUTHORIZED);
}
