use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

#[tokio::test]
async fn ticket_lifecycle_isolation_and_internal_notes() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;

    let deps = call(
        &app,
        "GET",
        "/api/v1/client/ticket-departments",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(deps.status, StatusCode::OK);
    assert!(deps.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"] == SUPPORT));

    // Validation.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(json!({"department_id": SUPPORT, "subject": "  ", "body": "x"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(
            json!({"department_id": SUPPORT, "subject": "Hi", "body": "x", "priority": "nuclear"}),
        ),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(json!({"department_id": uuid::Uuid::new_v4(), "subject": "Hi", "body": "x"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_department");

    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "Server down", "body": "Help <b>please</b>", "priority": "high"})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["ticket"]["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["ticket"]["status"], "open");
    assert_eq!(r.body["messages"][0]["body"], "Help <b>please</b>");

    // Another customer cannot see it (404, not 403).
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/tickets/{id}"),
            None,
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/tickets/{id}/reply"),
            Some(json!({"body": "me too"})),
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/tickets", None, &other.h())
            .await
            .body["total"],
        0
    );
    // Customers can't use the desk.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/tickets", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Staff: internal note (hidden from client, status unchanged), then reply.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{id}/reply"),
        Some(json!({"body": "check node 3", "internal": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["ticket"]["status"], "open");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{id}/reply"),
        Some(json!({"body": "Fixed!"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["status"], "answered");
    assert_eq!(r.body["messages"].as_array().unwrap().len(), 3);
    let mails = mails_to(&state, &cust.email).await;
    assert!(
        mails
            .iter()
            .any(|m| m.subject.contains("Server down") || m.text.contains("Server down")),
        "reply notification"
    );

    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{id}"),
        None,
        &cust.h(),
    )
    .await;
    let msgs = r.body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2, "internal note hidden");
    assert!(msgs.iter().all(|m| m["body"] != "check node 3"));
    assert_eq!(msgs[1]["is_staff"], true);

    // Client replies -> customer_reply; closes; replying reopens.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{id}/reply"),
        Some(json!({"body": "thanks"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["status"], "customer_reply");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{id}/close"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["status"], "closed");
    assert!(r.body["ticket"]["closed_at"].is_string());

    // Admin update + filters.
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{id}"),
        Some(json!({"status": "bogus"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{id}"),
        Some(json!({"status": "open", "priority": "urgent"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["priority"], "urgent");
    assert!(r.body["ticket"]["closed_at"].is_null());
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/tickets?status=active&q=Server%20down",
        None,
        &admin.h(),
    )
    .await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == id.as_str()));
    // Assigning a customer is rejected.
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{id}"),
        Some(json!({"assigned_to": uuid::Uuid::new_v4()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_assignee");
    // CSRF required.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{id}/reply"),
        Some(json!({"body": "x"})),
        &[("cookie", cust.cookie.as_str())],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}
