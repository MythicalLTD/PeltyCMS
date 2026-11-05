use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn notes_fields_and_todos() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let cust = sess(&app, &st, "customer").await;
    let cid = call(&app, "GET", "/api/v1/client/profile", None, &cust.h()).await.body["id"].as_str().unwrap().to_string();

    // Notes: customer cannot touch them; empty note rejected.
    let np = format!("/api/v1/admin/customers/{cid}/notes");
    assert_eq!(call(&app, "POST", &np, Some(json!({"body": "x"})), &cust.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", &np, Some(json!({"body": "  "})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", &np, Some(json!({"body": "VIP client", "sticky": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let nid = r.body["id"].as_str().unwrap().to_string();
    let l = call(&app, "GET", &np, None, &admin.h()).await;
    assert_eq!(l.body["items"][0]["body"], "VIP client");
    assert_eq!(call(&app, "PUT", &format!("{np}/{nid}"), Some(json!({"body": "VIP"})), &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "DELETE", &format!("{np}/{nid}"), None, &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "DELETE", &format!("{np}/{nid}"), None, &admin.h()).await.status, StatusCode::NOT_FOUND);

    // Fields: a public dropdown and an admin-only text field.
    assert_eq!(call(&app, "POST", "/api/v1/admin/client-fields", Some(json!({"name": "x", "field_type": "dropdown"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/client-fields", Some(json!({"name": "Source", "field_type": "dropdown", "options": ["Google", "Friend"], "required": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pubf = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", "/api/v1/admin/client-fields", Some(json!({"name": "Risk", "field_type": "text", "admin_only": true})), &admin.h()).await;
    let privf = r.body["id"].as_str().unwrap().to_string();

    let c = call(&app, "GET", "/api/v1/client/fields", None, &cust.h()).await;
    assert_eq!(c.status, StatusCode::OK);
    assert!(!c.body["fields"].as_array().unwrap().iter().any(|f| f["id"] == privf.as_str()));
    assert_eq!(call(&app, "PUT", "/api/v1/client/fields", Some(json!({"values": {pubf.clone(): "Spam"}})), &cust.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/client/fields", Some(json!({"values": {privf.clone(): "low"}})), &cust.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/client/fields", Some(json!({"values": {pubf.clone(): "Friend"}})), &cust.h()).await.status, StatusCode::OK);
    let fp = format!("/api/v1/admin/customers/{cid}/fields");
    assert_eq!(call(&app, "PUT", &fp, Some(json!({"values": {privf.clone(): "low"}})), &admin.h()).await.status, StatusCode::OK);
    let a = call(&app, "GET", &fp, None, &admin.h()).await;
    assert_eq!(a.body["values"][pubf.as_str()], "Friend");
    assert_eq!(a.body["values"][privf.as_str()], "low");
    let c = call(&app, "GET", "/api/v1/client/fields", None, &cust.h()).await;
    assert!(c.body["values"].get(privf.as_str()).is_none());
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/client-fields/{privf}"), None, &admin.h()).await.status, StatusCode::OK);

    // To-dos: staff may manage; customer may not; bad status rejected.
    assert_eq!(call(&app, "POST", "/api/v1/admin/todos", Some(json!({"title": "Call"})), &cust.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", "/api/v1/admin/todos", Some(json!({"title": "Call", "status": "nope"})), &staff.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/todos", Some(json!({"title": "Call client", "due_date": "2030-01-02"})), &staff.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["id"].as_str().unwrap().to_string();
    let open = call(&app, "GET", "/api/v1/admin/todos?status=open", None, &admin.h()).await;
    assert!(open.body["items"].as_array().unwrap().iter().any(|t| t["id"] == tid.as_str()));
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/todos/{tid}"), Some(json!({"title": "Call client", "status": "done"})), &admin.h()).await.status, StatusCode::OK);
    let open = call(&app, "GET", "/api/v1/admin/todos?status=open", None, &admin.h()).await;
    assert!(!open.body["items"].as_array().unwrap().iter().any(|t| t["id"] == tid.as_str()));
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/todos/{tid}"), None, &staff.h()).await.status, StatusCode::OK);
}
