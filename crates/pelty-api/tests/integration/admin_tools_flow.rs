use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn global_search_health_notes_and_rules() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;

    assert_eq!(call(&app, "GET", "/api/v1/admin/search?q=ab", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/search?q=a", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "GET", "/api/v1/admin/search?q=%25%25", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    for k in ["customers", "invoices", "services", "tickets", "domains", "orders"] {
        assert!(r.body[k].is_array(), "{k}");
    }

    let h = call(&app, "GET", "/api/v1/admin/system-health", None, &admin.h()).await;
    assert_eq!(h.status, StatusCode::OK, "{:?}", h.body);
    assert_eq!(h.body["checks"][0]["status"], "ok");
    assert!(h.body["metrics"]["migrations"].as_i64().unwrap() >= 50);
    assert_eq!(call(&app, "GET", "/api/v1/admin/system-health", None, &cust.h()).await.status, StatusCode::FORBIDDEN);

    assert_eq!(call(&app, "PUT", "/api/v1/admin/my-notes", Some(json!({"body": "call Bob"})), &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/admin/my-notes", None, &admin.h()).await.body["body"], "call Bob");

    let bad = json!({"name": "x", "provider": "slack", "url": "http://insecure", "events": ["invoice.paid"]});
    assert_eq!(call(&app, "POST", "/api/v1/admin/notification-rules", Some(bad), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let body = json!({"name": "Ops", "provider": "discord", "url": "https://discord.example/api/webhooks/1/secret", "events": ["invoice.paid", "ticket.opened"]});
    assert_eq!(call(&app, "POST", "/api/v1/admin/notification-rules", Some(body.clone()), &cust.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "POST", "/api/v1/admin/notification-rules", Some(body), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    let l = call(&app, "GET", "/api/v1/admin/notification-rules", None, &admin.h()).await;
    let it = l.body["items"].as_array().unwrap().iter().find(|i| i["id"] == id.as_str()).unwrap().clone();
    assert!(!it["url_masked"].as_str().unwrap().contains("secret"));
    let p = call(&app, "POST", "/api/v1/admin/notification-rules/preview", Some(json!({"provider": "slack", "event": "invoice.paid"})), &admin.h()).await;
    assert!(p.body["text"].as_str().unwrap().starts_with("*invoice.paid*"));
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/notification-rules/{id}"), None, &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/notification-rules/{id}"), None, &admin.h()).await.status, StatusCode::NOT_FOUND);

    // Client summary: create a customer, then fetch all tabs at once.
    let c = call(&app, "POST", "/api/v1/admin/customers", Some(json!({"email": format!("sum-{}@example.test", uuid::Uuid::new_v4()), "first_name": "Sum", "last_name": "Mary"})), &admin.h()).await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    let cid = c.body["id"].as_str().unwrap().to_string();
    let s = call(&app, "GET", &format!("/api/v1/admin/customers/{cid}/summary"), None, &admin.h()).await;
    assert_eq!(s.status, StatusCode::OK, "{:?}", s.body);
    assert_eq!(s.body["customer"]["first_name"], "Sum");
    assert!(s.body["customer"].get("notes").is_none());
    assert_eq!(s.body["stats"]["amount_due"], "0");
    for k in ["contacts", "services", "domains", "invoices", "transactions", "tickets", "notes", "log", "credit_ledger"] {
        assert!(s.body[k].is_array(), "{k}");
    }
    let r = call(&app, "GET", "/api/v1/admin/search?q=Mary", None, &admin.h()).await;
    assert!(r.body["customers"].as_array().unwrap().iter().any(|x| x["id"] == cid.as_str()));
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/customers/{}/summary", uuid::Uuid::new_v4()), None, &admin.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/customers/{cid}/summary"), None, &cust.h()).await.status, StatusCode::FORBIDDEN);

    // Report library: every report runs on a real DB; CSV export works.
    let lib = call(&app, "GET", "/api/v1/admin/reports/library", None, &admin.h()).await;
    assert_eq!(lib.status, StatusCode::OK);
    let keys: Vec<String> = lib.body["items"].as_array().unwrap().iter().map(|i| i["key"].as_str().unwrap().to_string()).collect();
    assert!(keys.len() >= 18);
    for k in &keys {
        let r = call(&app, "GET", &format!("/api/v1/admin/reports/library/{k}?from=2025-01&to=2026-12"), None, &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{k}: {:?}", r.body);
        let cols = r.body["columns"].as_array().unwrap().len();
        for row in r.body["rows"].as_array().unwrap() { assert_eq!(row.as_array().unwrap().len(), cols, "{k}"); }
    }
    let nc = call(&app, "GET", "/api/v1/admin/reports/library/new_customers", None, &admin.h()).await;
    assert!(!nc.body["rows"].as_array().unwrap().is_empty());
    assert_eq!(call(&app, "GET", "/api/v1/admin/reports/library/nope", None, &admin.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/v1/admin/reports/library/new_customers?format=xml", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/reports/library/new_customers", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
}
