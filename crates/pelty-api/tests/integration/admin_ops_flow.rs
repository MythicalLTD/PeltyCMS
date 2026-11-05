use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn prefs_history_online_calendar_emails_backups() {
    let dir = std::env::temp_dir().join(format!("pelty-bk-{}", uuid::Uuid::new_v4()));
    std::env::set_var("PELTY_BACKUP_DIR", &dir);
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;

    // Prefs: defaults, validation, save.
    let p = call(&app, "GET", "/api/v1/admin/prefs", None, &admin.h()).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert!(p.body["prefs"]["widgets"].as_array().unwrap().iter().any(|w| w == "stats"));
    let bad = json!({"widgets": ["stats"], "quick_links": [{"label": "x", "url": "javascript:alert(1)"}]});
    assert_eq!(call(&app, "PUT", "/api/v1/admin/prefs", Some(bad), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let good = json!({"widgets": ["todos", "stats"], "quick_links": [{"label": "Billing", "url": "/admin/billing"}]});
    assert_eq!(call(&app, "PUT", "/api/v1/admin/prefs", Some(good), &admin.h()).await.status, StatusCode::OK);
    let p = call(&app, "GET", "/api/v1/admin/prefs", None, &admin.h()).await;
    assert_eq!(p.body["prefs"]["widgets"], json!(["todos", "stats"]));
    assert_eq!(p.body["prefs"]["quick_links"][0]["url"], "/admin/billing");
    assert_eq!(call(&app, "GET", "/api/v1/admin/prefs", None, &cust.h()).await.status, StatusCode::FORBIDDEN);

    // Login history + staff online include this admin session, not customers.
    let h = call(&app, "GET", "/api/v1/admin/login-history?limit=500", None, &admin.h()).await;
    assert_eq!(h.status, StatusCode::OK, "{:?}", h.body);
    let items = h.body["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["email"] == admin.email.as_str() && i["state"] == "active"));
    assert!(!items.iter().any(|i| i["email"] == cust.email.as_str()));
    let o = call(&app, "GET", "/api/v1/admin/staff-online", None, &admin.h()).await;
    assert!(o.body["items"].as_array().unwrap().iter().any(|i| i["email"] == admin.email.as_str()));

    // Calendar: a to-do due this month shows up.
    let today = time::OffsetDateTime::now_utc().date();
    let month = format!("{}-{:02}", today.year(), today.month() as u8);
    let t = call(&app, "POST", "/api/v1/admin/todos", Some(json!({"title": "cal-check", "due_date": today.to_string()})), &admin.h()).await;
    assert!(t.status.is_success(), "{:?}", t.body);
    let c = call(&app, "GET", &format!("/api/v1/admin/calendar?month={month}"), None, &admin.h()).await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    assert!(c.body["items"].as_array().unwrap().iter().any(|i| i["kind"] == "todo" && i["title"] == "cal-check"));
    assert_eq!(call(&app, "GET", "/api/v1/admin/calendar?month=2026-13", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Email log + resend.
    let email = format!("log-{}@example.test", uuid::Uuid::new_v4());
    let c = call(&app, "POST", "/api/v1/admin/customers", Some(json!({"email": email, "first_name": "Log"})), &admin.h()).await;
    let cid = c.body["id"].as_str().unwrap().to_string();
    pelty_api::mail::enqueue(&st, "password_reset", "en", &email, pelty_api::mail::vars(&[("link", "https://x.test/r")])).await.unwrap();
    let l = call(&app, "GET", &format!("/api/v1/admin/customers/{cid}/emails"), None, &admin.h()).await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    let items = l.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["template"], "password_reset");
    let job = items[0]["id"].as_i64().unwrap();
    // Live-token mail can never be replayed by staff.
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/emails/{job}/resend"), None, &admin.h()).await.status, StatusCode::CONFLICT);
    pelty_api::mail::enqueue(&st, "cart_abandoned", "en", &email, pelty_api::mail::vars(&[("link", "https://x.test/c")])).await.unwrap();
    let l = call(&app, "GET", &format!("/api/v1/admin/customers/{cid}/emails"), None, &admin.h()).await;
    let job = l.body["items"].as_array().unwrap().iter().find(|i| i["template"] == "cart_abandoned").unwrap()["id"].as_i64().unwrap();
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/emails/{job}/resend"), None, &admin.h()).await.status, StatusCode::OK);
    let l = call(&app, "GET", &format!("/api/v1/admin/customers/{cid}/emails"), None, &admin.h()).await;
    assert_eq!(l.body["items"].as_array().unwrap().len(), 3);
    assert_eq!(mails_to(&st, &email).await.len(), 3, "reset + cart + resend delivered");
    assert_eq!(call(&app, "POST", "/api/v1/admin/emails/999999999/resend", None, &admin.h()).await.status, StatusCode::NOT_FOUND);

    // Backups with real pg_dump.
    assert_eq!(call(&app, "POST", "/api/v1/admin/backups", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let b = call(&app, "POST", "/api/v1/admin/backups", None, &admin.h()).await;
    assert_eq!(b.status, StatusCode::OK, "{:?}", b.body);
    let bid = b.body["id"].as_str().unwrap().to_string();
    drain_jobs(&st, pelty_api::backup::JOB_KIND, &bid).await;
    let l = call(&app, "GET", "/api/v1/admin/backups", None, &admin.h()).await;
    let it = l.body["items"].as_array().unwrap().iter().find(|i| i["id"] == bid.as_str()).unwrap().clone();
    // Machines without a usable pg_dump (none on PATH, or a client older than
    // the server) cannot take a real dump: the backup must then fail cleanly
    // with the reason, and the rest of the check is skipped.
    let err = it["error"].as_str().unwrap_or_default().to_string();
    if it["status"] == "failed" && (err.contains("not runnable") || err.contains("version mismatch")) {
        eprintln!("skipping the pg_dump part of admin_ops_flow: {err} (set PELTY_PG_DUMP to a matching pg_dump)");
        assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/backups/{bid}"), None, &admin.h()).await.status, StatusCode::OK);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(it["status"], "done", "{it:?}");
    assert!(it["size_bytes"].as_i64().unwrap() > 1000);
    let d = call_raw(&app, "GET", &format!("/api/v1/admin/backups/{bid}/download"), &admin.h()).await;
    assert_eq!(d.0, StatusCode::OK);
    assert_eq!(&d.1[..5], b"PGDMP", "custom-format dump header");
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/backups/{bid}"), None, &admin.h()).await.status, StatusCode::OK);
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "file removed");
    let _ = std::fs::remove_dir_all(&dir);
}

async fn call_raw(app: &axum::Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> (StatusCode, Vec<u8>) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let mut b = axum::http::Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let res = app.clone().oneshot(b.body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let st = res.status();
    (st, res.into_body().collect().await.unwrap().to_bytes().to_vec())
}
