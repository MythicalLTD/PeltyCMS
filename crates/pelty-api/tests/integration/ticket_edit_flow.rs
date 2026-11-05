use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

#[tokio::test]
async fn staff_edit_and_delete_ticket_replies() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "Help", "body": "First", "priority": "low"})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["ticket"]["id"].as_str().unwrap().to_string();
    let first = r.body["messages"][0]["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", &format!("/api/v1/admin/tickets/{id}/reply"), Some(json!({"body": "Staff typo"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = call(&app, "GET", &format!("/api/v1/admin/tickets/{id}"), None, &admin.h()).await;
    let reply = d.body["messages"][1]["id"].as_str().unwrap().to_string();
    let url = |m: &str| format!("/api/v1/admin/tickets/{id}/messages/{m}");

    // Customers cannot use the staff endpoints.
    assert_eq!(call(&app, "PUT", &url(&reply), Some(json!({"body": "x"})), &cust.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "DELETE", &url(&reply), None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    // Validation and scoping.
    assert_eq!(call(&app, "PUT", &url(&reply), Some(json!({"body": "  "})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", &url(&Uuid::new_v4().to_string()), Some(json!({"body": "x"})), &admin.h()).await.status, StatusCode::NOT_FOUND);

    let r = call(&app, "PUT", &url(&reply), Some(json!({"body": "Staff fixed"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = call(&app, "GET", &format!("/api/v1/client/tickets/{id}"), None, &cust.h()).await;
    assert_eq!(c.body["messages"][1]["body"], "Staff fixed");
    assert!(c.body["messages"][1]["edited_at"].is_string());
    assert!(c.body["messages"][0]["edited_at"].is_null());

    // The opening message can be edited but not deleted.
    assert_eq!(call(&app, "DELETE", &url(&first), None, &admin.h()).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "DELETE", &url(&reply), None, &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "DELETE", &url(&reply), None, &admin.h()).await.status, StatusCode::NOT_FOUND);
    let c = call(&app, "GET", &format!("/api/v1/client/tickets/{id}"), None, &cust.h()).await;
    assert_eq!(c.body["messages"].as_array().unwrap().len(), 1);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action IN ('ticket.message_edited', 'ticket.message_deleted') AND target = $1")
        .bind(&id).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n, 2);
}
