use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn sla_policies_state_scan_and_escalation() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let (ah, ch) = (admin.h(), cust.h());
    let mkdep = |n: &str| call(&app, "POST", "/api/v1/admin/ticket-departments", Some(json!({"name": format!("{n} {}", uuid::Uuid::new_v4().simple())})), &ah);
    let dep = mkdep("SLA").await.body["id"].as_str().unwrap().to_string();
    let tier2 = mkdep("Tier2").await.body["id"].as_str().unwrap().to_string();
    let base = "/api/v1/admin/ticket-sla/policies";

    // Staff only and validated.
    assert_eq!(call(&app, "GET", base, None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    for bad in [
        json!({"department_id": dep}),
        json!({"department_id": dep, "first_response_minutes": 0}),
        json!({"department_id": dep, "first_response_minutes": 60, "resolution_minutes": 30}),
        json!({"department_id": dep, "priority": "nope", "first_response_minutes": 60}),
        json!({"department_id": dep, "first_response_minutes": 60, "warn_percent": 100}),
    ] {
        let r = call(&app, "POST", base, Some(bad.clone()), &admin.h()).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad} {:?}", r.body);
    }

    // Department-wide policy plus a more specific urgent one.
    let r = call(&app, "POST", base, Some(json!({"department_id": dep, "first_response_minutes": 60, "resolution_minutes": 600,
        "escalate_priority": "high", "escalate_department": tier2})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pol = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", base, Some(json!({"department_id": dep, "first_response_minutes": 120})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "same scope twice: {:?}", r.body);
    let r = call(&app, "POST", base, Some(json!({"department_id": dep, "priority": "urgent", "first_response_minutes": 15})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let urgent = r.body["id"].as_str().unwrap().to_string();
    let r = call(&app, "PUT", &format!("{base}/{urgent}"), Some(json!({"department_id": dep, "priority": "urgent", "first_response_minutes": 10, "warn_percent": 50})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["warn_percent"], 50);
    let l = call(&app, "GET", base, None, &admin.h()).await;
    assert!(l.body["items"].as_array().unwrap().iter().any(|p| p["id"] == pol));

    // Tickets: one fresh, one in the warning window, one breached, one on hold.
    let mk = |s: &str| call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": dep, "subject": s, "body": "x"})), &ch);
    let tid = |r: Resp| uuid::Uuid::parse_str(r.body["ticket"]["id"].as_str().unwrap()).unwrap();
    let fresh = tid(mk("fresh").await);
    let warn = tid(mk("warn").await);
    let late = tid(mk("late").await);
    let hold = tid(mk("hold").await);
    let age = |t: uuid::Uuid, mins: i32| {
        let pool = st.pool.clone();
        async move { sqlx::query("UPDATE tickets SET created_at = now() - make_interval(mins => $2) WHERE id = $1").bind(t).bind(mins).execute(&pool).await.unwrap(); }
    };
    age(warn, 50).await;
    age(late, 90).await;
    age(hold, 90).await;
    sqlx::query("UPDATE tickets SET status = 'on_hold' WHERE id = $1").bind(hold).execute(&st.pool).await.unwrap();

    let sla = |t: uuid::Uuid| {
        let (app, ah) = (&app, &ah);
        async move { call(app, "GET", &format!("/api/v1/admin/tickets/{t}/sla"), None, ah).await }
    };
    let r = sla(fresh).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["policy_id"], pol);
    assert_eq!(r.body["first_response"]["state"], "ok");
    assert_eq!(r.body["resolution"]["state"], "ok");
    assert_eq!(sla(warn).await.body["first_response"]["state"], "warning");
    assert_eq!(sla(late).await.body["first_response"]["state"], "breached");
    assert_eq!(sla(hold).await.body["paused"], true);
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/tickets/{}/sla", uuid::Uuid::new_v4()), None, &admin.h()).await.status, StatusCode::NOT_FOUND);

    // Urgent priority switches to the specific policy (live retarget).
    sqlx::query("UPDATE tickets SET priority = 'urgent' WHERE id = $1").bind(fresh).execute(&st.pool).await.unwrap();
    let r = sla(fresh).await;
    assert_eq!(r.body["policy_id"], urgent);
    assert!(r.body["resolution"].is_null());
    sqlx::query("UPDATE tickets SET priority = 'medium' WHERE id = $1").bind(fresh).execute(&st.pool).await.unwrap();

    let r = call(&app, "GET", &format!("/api/v1/admin/ticket-sla/at-risk?department_id={dep}"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = r.body["items"].as_array().unwrap();
    let ids: Vec<&str> = items.iter().map(|i| i["ticket_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![late.to_string().as_str(), warn.to_string().as_str()], "most urgent first, no fresh/on-hold");
    assert_eq!(items[0]["state"], "breached");
    assert_eq!(items[1]["state"], "warning");

    // Scan: a warning, a breach with escalation, each only once.
    pelty_api::routes::ticket_sla::run_scan(&st).await.unwrap();
    let events = |t: uuid::Uuid| {
        let pool = st.pool.clone();
        async move { sqlx::query_scalar::<_, String>("SELECT target || ':' || stage FROM ticket_sla_events WHERE ticket_id = $1 ORDER BY 1").bind(t).fetch_all(&pool).await.unwrap() }
    };
    assert_eq!(events(warn).await, vec!["first_response:warning"]);
    assert_eq!(events(late).await, vec!["first_response:breach", "first_response:warning"]);
    assert!(events(fresh).await.is_empty());
    assert!(events(hold).await.is_empty());
    let (pri, d): (String, uuid::Uuid) = sqlx::query_as("SELECT priority, department_id FROM tickets WHERE id = $1").bind(late).fetch_one(&st.pool).await.unwrap();
    assert_eq!(pri, "high");
    assert_eq!(d.to_string(), tier2);
    let tagged: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM ticket_tags WHERE ticket_id = $1 AND tag = 'sla-breached')").bind(late).fetch_one(&st.pool).await.unwrap();
    assert!(tagged);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'ticket.sla_breached' AND target = $1").bind(late.to_string()).fetch_one(&st.pool).await.unwrap();
    assert_eq!(audited, 1);
    pelty_api::routes::ticket_sla::run_scan(&st).await.unwrap();
    assert_eq!(events(late).await.len(), 2, "fires once");
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'ticket.sla_breached' AND target = $1").bind(late.to_string()).fetch_one(&st.pool).await.unwrap();
    assert_eq!(audited, 1);

    // A staff reply meets the first-response target.
    let r = call(&app, "POST", &format!("/api/v1/admin/tickets/{warn}/reply"), Some(json!({"body": "On it"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(sla(warn).await.body["first_response"]["state"], "met");

    // Legacy department SLA endpoint keeps the department policy in sync.
    let r = call(&app, "PUT", &format!("/api/v1/admin/ticket-departments/{tier2}/sla"), Some(json!({"email": "", "sla_first_response_minutes": 30})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let l = call(&app, "GET", base, None, &admin.h()).await;
    assert!(l.body["items"].as_array().unwrap().iter().any(|p| p["department_id"] == tier2 && p["priority"].is_null() && p["first_response_minutes"] == 30));

    for id in [&pol, &urgent] {
        assert_eq!(call(&app, "DELETE", &format!("{base}/{id}"), None, &admin.h()).await.status, StatusCode::OK);
    }
    assert_eq!(call(&app, "DELETE", &format!("{base}/{pol}"), None, &admin.h()).await.status, StatusCode::NOT_FOUND);
}
