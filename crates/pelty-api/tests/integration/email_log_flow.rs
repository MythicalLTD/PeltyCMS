use crate::common;
use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn email_log_records_views_resends_and_scopes() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let other = sess(&app, &st, "customer").await;

    pelty_api::mail::enqueue(&st, "cart_abandoned", "en", &cust.email, pelty_api::mail::vars(&[("link", "https://x.test/cart")])).await.unwrap();
    pelty_api::mail::enqueue(&st, "password_reset", "en", &cust.email, pelty_api::mail::vars(&[("link", "https://x.test/secret-token")])).await.unwrap();
    assert_eq!(mails_to(&st, &cust.email).await.len(), 2);

    // Client sees own history, newest first; security mail body is redacted.
    let l = call(&app, "GET", "/api/v1/client/emails", None, &cust.h()).await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    let items = l.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["status"] == "sent"));
    let reset = items.iter().find(|i| i["template"] == "password_reset").unwrap()["id"].as_str().unwrap().to_string();
    let cart = items.iter().find(|i| i["template"] == "cart_abandoned").unwrap()["id"].as_str().unwrap().to_string();
    let d = call(&app, "GET", &format!("/api/v1/client/emails/{reset}"), None, &cust.h()).await;
    assert_eq!(d.body["body_text"], "[redacted]");
    assert!(!d.body.to_string().contains("secret-token"));
    let d = call(&app, "GET", &format!("/api/v1/client/emails/{cart}"), None, &cust.h()).await;
    assert!(d.body["body_text"].as_str().unwrap().contains("https://x.test/cart"), "{:?}", d.body);

    // Another client can't read it, nor list it.
    assert_eq!(call(&app, "GET", &format!("/api/v1/client/emails/{cart}"), None, &other.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/v1/client/emails", None, &other.h()).await.body["total"], 0);
    // Customers can't use the admin log.
    assert_eq!(call(&app, "GET", "/api/v1/admin/email-log", None, &cust.h()).await.status, StatusCode::FORBIDDEN);

    // Admin search + resend; security mails can't be resent.
    let a = call(&app, "GET", &format!("/api/v1/admin/email-log?q={}", cust.email), None, &admin.h()).await;
    assert_eq!(a.body["total"], 2, "{:?}", a.body);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/email-log/{reset}/resend"), None, &admin.h()).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/email-log/{cart}/resend"), None, &admin.h()).await.status, StatusCode::OK);
    let m = mails_to(&st, &cust.email).await;
    assert_eq!(m.len(), 3, "resend delivered");
    assert!(m.last().unwrap().text.contains("https://x.test/cart"));
    assert_eq!(call(&app, "GET", "/api/v1/client/emails", None, &cust.h()).await.body["total"], 3);
}
