use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Minimal SMTP sink: accepts one session and returns the DATA it got.
async fn fake_smtp() -> (u16, tokio::task::JoinHandle<String>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let h = tokio::spawn(async move {
        let (sock, _) = l.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r);
        w.write_all(b"220 fake ESMTP\r\n").await.unwrap();
        let mut data = String::new();
        let mut in_data = false;
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            if in_data {
                if line == ".\r\n" {
                    in_data = false;
                    w.write_all(b"250 queued\r\n").await.unwrap();
                } else {
                    data.push_str(&line);
                }
                continue;
            }
            let up = line.to_ascii_uppercase();
            if up.starts_with("EHLO") || up.starts_with("HELO") {
                w.write_all(b"250 fake\r\n").await.unwrap();
            } else if up.starts_with("DATA") {
                in_data = true;
                w.write_all(b"354 go\r\n").await.unwrap();
            } else if up.starts_with("QUIT") {
                w.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                w.write_all(b"250 ok\r\n").await.unwrap();
            }
        }
        data
    });
    (port, h)
}

#[tokio::test]
async fn smtp_settings_encrypted_and_test_send() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let url = "/api/v1/admin/settings/smtp";
    assert_eq!(call(&app, "GET", url, None, &staff.h()).await.status, StatusCode::FORBIDDEN);

    let r = call(&app, "PUT", url, Some(json!({"enabled": true, "host": "bad host!", "port": 25, "security": "none", "from_address": "a@b.test"})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.smtp_invalid");
    let r = call(&app, "PUT", url, Some(json!({"enabled": true, "host": "127.0.0.1", "port": 25, "security": "ssl3", "from_address": "a@b.test"})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.smtp_invalid");

    let (port, sink) = fake_smtp().await;
    let r = call(&app, "PUT", url, Some(json!({
        "enabled": true, "host": "127.0.0.1", "port": port, "security": "none",
        "username": "", "password": "Sup3r-Secret-PW", "from_address": "billing@pelty.test", "from_name": "Pelty"
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["password_set"], true);
    assert_eq!(r.body["source"], "settings");
    assert!(!r.body.to_string().contains("Sup3r-Secret-PW"));
    let raw: serde_json::Value = sqlx::query_scalar("SELECT value FROM system_settings WHERE key = 'mail.smtp'").fetch_one(&state.pool).await.unwrap();
    assert!(!raw.to_string().contains("Sup3r-Secret-PW"), "password sealed at rest");
    assert_eq!(state.active_mailer().name(), "smtp");

    // Keep password when omitted.
    let r = call(&app, "PUT", url, Some(json!({"enabled": true, "host": "127.0.0.1", "port": port, "security": "none", "from_address": "billing@pelty.test", "from_name": "Pelty"})), &admin.h()).await;
    assert_eq!(r.body["password_set"], true);

    let to = email();
    let r = call(&app, "POST", "/api/v1/admin/settings/smtp/test", Some(json!({"to": to})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["ok"], true, "{:?}", r.body);
    let data = sink.await.unwrap();
    assert!(data.contains(&format!("To: {to}")) && data.contains("Pelty"), "{data}");

    // Unreachable server reports the error instead of failing the request.
    let r = call(&app, "PUT", url, Some(json!({"enabled": false, "host": "127.0.0.1", "port": 1, "security": "none", "clear_password": true, "from_address": "billing@pelty.test"})), &admin.h()).await;
    assert_eq!(r.body["password_set"], false);
    assert_ne!(r.body["source"], "settings");
    assert_eq!(state.active_mailer().name(), "memory");
    let r = call(&app, "POST", "/api/v1/admin/settings/smtp/test", Some(json!({"to": email()})), &admin.h()).await;
    assert_eq!(r.body["ok"], false);
    assert!(r.body["error"].is_string());

    sqlx::query("DELETE FROM system_settings WHERE key = 'mail.smtp'").execute(&state.pool).await.unwrap();
}
