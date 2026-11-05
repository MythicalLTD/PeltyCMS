use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn macros_crud_and_apply() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let (ah, ch) = (admin.h(), cust.h());
    let mkdep = |n: &str| call(&app, "POST", "/api/v1/admin/ticket-departments", Some(json!({"name": format!("{n} {}", uuid::Uuid::new_v4().simple())})), &ah);
    let dep = mkdep("Mac").await.body["id"].as_str().unwrap().to_string();
    let other = mkdep("Other").await.body["id"].as_str().unwrap().to_string();
    let me = call(&app, "GET", "/api/v1/auth/me", None, &ah).await;
    let uid = me.body["user"]["id"].as_str().or(me.body["id"].as_str()).unwrap().to_string();
    let base = "/api/v1/admin/ticket-macros";

    assert_eq!(call(&app, "GET", base, None, &ch).await.status, StatusCode::FORBIDDEN);
    for (bad, code) in [
        (json!({"name": "Nothing"}), "error.macro_empty"),
        (json!({"name": "x", "set_status": "nope"}), "error.invalid_status"),
        (json!({"name": "x", "assign_mode": "user"}), "error.invalid_assignee"),
        (json!({"name": "x", "assign_mode": "user", "assign_to": uuid::Uuid::new_v4()}), "error.invalid_assignee"),
        (json!({"name": "x", "add_tags": ["no way!"]}), "error.invalid_tag"),
        (json!({"name": "", "set_priority": "high"}), "error.validation"),
    ] {
        let r = call(&app, "POST", base, Some(bad.clone()), &ah).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(r.body["error"]["code"].as_str().or(r.body["code"].as_str()), Some(code), "{bad} {:?}", r.body);
    }

    // Reply + close + tag, scoped to `dep`; a note + escalate for everyone.
    let r = call(&app, "POST", base, Some(json!({"name": "Solved", "department_id": dep,
        "reply_body": "Hi {customer_first_name}, ticket #{ticket_number} is fixed., {staff_name}",
        "set_status": "closed", "add_tags": ["Solved"], "assign_mode": "self"})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let solved = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["add_tags"], json!(["solved"]));
    let r = call(&app, "POST", base, Some(json!({"name": "Escalate", "reply_body": "Escalated to tier 2", "reply_internal": true,
        "set_priority": "urgent", "set_department": other, "assign_mode": "user", "assign_to": uid})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let esc = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", base, Some(json!({"name": "Off", "set_priority": "low", "is_active": false})), &ah).await;
    let off = r.body["id"].as_str().unwrap().to_string();

    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": dep, "subject": "Broken disk", "body": "help"})), &ch).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["ticket"]["id"].as_str().unwrap().to_string();
    let number = r.body["ticket"]["number"].as_i64().unwrap();

    // Offered on the ticket with the reply filled in; inactive ones hidden.
    let r = call(&app, "GET", &format!("/api/v1/admin/tickets/{tid}/macros"), None, &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = r.body["items"].as_array().unwrap();
    let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&solved.as_str()) && ids.contains(&esc.as_str()) && !ids.contains(&off.as_str()));
    let s = items.iter().find(|i| i["id"] == solved).unwrap();
    assert!(s["rendered_body"].as_str().unwrap().contains(&format!("ticket #{number} is fixed")), "{s}");

    // Inactive / other-department macros can't be applied.
    let apply = |m: &str, b: serde_json::Value| {
        let (app, ah, uri) = (&app, &ah, format!("/api/v1/admin/tickets/{tid}/macros/{m}"));
        async move { call(app, "POST", &uri, Some(b), ah).await }
    };
    assert_eq!(apply(&off, json!({})).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Escalate: internal note, priority, department, fixed assignee.
    let r = apply(&esc, json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ticket"]["priority"], "urgent");
    assert_eq!(r.body["ticket"]["department_id"], other);
    assert_eq!(r.body["ticket"]["assigned_to"], uid);
    let note = r.body["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(note["body"], "Escalated to tier 2");
    assert_eq!(note["is_internal"], true);
    // Now in `other`: the department-scoped macro is no longer offered or usable.
    assert_eq!(apply(&solved, json!({})).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    call(&app, "PATCH", &format!("/api/v1/admin/tickets/{tid}"), Some(json!({"department_id": dep, "assigned_to": null})), &ah).await;

    // Solved with an edited reply: public reply, closed, tagged, assigned to me.
    let r = apply(&solved, json!({"body": "Edited: all good now"})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ticket"]["status"], "closed");
    assert_eq!(r.body["ticket"]["assigned_to"], uid);
    assert!(r.body["tags"].as_array().unwrap().iter().any(|t| t == "solved"));
    let msgs = r.body["messages"].as_array().unwrap();
    let last = msgs.last().unwrap();
    assert_eq!(last["body"], "Edited: all good now");
    assert_eq!(last["is_internal"], false);
    let cview = call(&app, "GET", &format!("/api/v1/client/tickets/{tid}"), None, &ch).await;
    let cmsgs = cview.body["messages"].as_array().unwrap();
    assert!(cmsgs.iter().any(|m| m["body"] == "Edited: all good now"));
    assert!(!cmsgs.iter().any(|m| m["body"] == "Escalated to tier 2"), "notes stay internal");

    let l = call(&app, "GET", base, None, &ah).await;
    let row = l.body["items"].as_array().unwrap().iter().find(|m| m["id"] == solved).unwrap().clone();
    assert_eq!(row["uses"], 1);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'ticket.macro_applied' AND target = $1").bind(&tid).fetch_one(&st.pool).await.unwrap();
    assert_eq!(audited, 2);

    // Update + delete.
    let r = call(&app, "PUT", &format!("{base}/{solved}"), Some(json!({"name": "Solved v2", "set_status": "closed"})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["reply_body"], "");
    assert!(r.body["department_id"].is_null());
    for m in [&solved, &esc, &off] {
        assert_eq!(call(&app, "DELETE", &format!("{base}/{m}"), None, &ah).await.status, StatusCode::OK);
    }
    assert_eq!(call(&app, "DELETE", &format!("{base}/{solved}"), None, &ah).await.status, StatusCode::NOT_FOUND);
}
