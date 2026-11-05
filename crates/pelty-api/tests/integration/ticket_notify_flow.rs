use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn staff_mails(state: &pelty_api::AppState, to: &str, ticket: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'ticket_staff_notify' AND payload->>'to' = $1 AND payload->'vars'->>'link' LIKE '%' || $2")
        .bind(to).bind(ticket).fetch_all(&state.pool).await.unwrap()
}

#[tokio::test]
async fn staff_get_mail_for_customer_activity() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let r = call(&app, "POST", "/api/v1/admin/ticket-departments", Some(json!({"name": format!("N {}", uuid::Uuid::new_v4().simple())})), &admin.h()).await;
    let dep = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": dep, "subject": "Disk full", "body": "Please help with my disk"})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["ticket"]["id"].as_str().unwrap().to_string();

    // Assign to this admin; a customer reply mails exactly them with an excerpt.
    let me = call(&app, "GET", "/api/v1/auth/me", None, &admin.h()).await;
    let uid = me.body["user"]["id"].as_str().or(me.body["id"].as_str()).unwrap().to_string();
    let r = call(&app, "PATCH", &format!("/api/v1/admin/tickets/{tid}"), Some(json!({"assigned_to": uid})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let before = staff_mails(&state, &admin.email, &tid).await.len();
    let r = call(&app, "POST", &format!("/api/v1/client/tickets/{tid}/reply"), Some(json!({"body": "Still broken"})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let mails = staff_mails(&state, &admin.email, &tid).await;
    assert_eq!(mails.len(), before + 1);
    let last = mails.iter().find(|m| m["vars"]["message"] == "Still broken").expect("reply mail");
    assert_eq!(last["vars"]["subject"], "Disk full");
    assert!(last["vars"]["link"].as_str().unwrap().contains("/admin/tickets/"));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'ticket_staff_notify' AND payload->'vars'->>'link' LIKE '%' || $1 AND payload->'vars'->>'message' = 'Still broken'")
        .bind(&tid).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 1, "only the assignee is mailed");

    // Staff replies don't notify staff.
    call(&app, "POST", &format!("/api/v1/admin/tickets/{tid}/reply"), Some(json!({"body": "Fixed"})), &admin.h()).await;
    assert!(staff_mails(&state, &admin.email, &tid).await.iter().all(|m| m["vars"]["message"] != "Fixed"));
}
