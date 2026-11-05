use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_api::pipe::{self, InboundMail, MockSource};
use serde_json::json;

async fn open(app: &axum::Router, s: &Sess, dep: &str, subject: &str) -> (String, i64) {
    let r = call(
        app,
        "POST",
        "/api/v1/client/tickets",
        Some(json!({"department_id": dep, "subject": subject, "body": "help"})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    (
        r.body["ticket"]["id"].as_str().unwrap().to_string(),
        r.body["ticket"]["number"].as_i64().unwrap(),
    )
}

#[tokio::test]
async fn support_suite_end_to_end() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;

    // Own department so escalation rules don't touch other tests' tickets.
    let dname = format!("Esc {}", uuid::Uuid::new_v4().simple());
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-departments",
        Some(json!({"name": dname})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let dep = r.body["id"].as_str().unwrap().to_string();
    let pipe_addr = format!("help-{}@example.test", uuid::Uuid::new_v4().simple());
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/ticket-departments/{dep}/sla"),
        Some(json!({"email": pipe_addr, "sla_first_response_minutes": 60})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/ticket-departments/{dep}/sla"),
        Some(json!({"email": "", "sla_first_response_minutes": 0})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let (a, a_num) = open(&app, &cust, &dep, "Ticket A").await;
    let (b, _) = open(&app, &cust, &dep, "Ticket B").await;
    let (o, _) = open(&app, &other, &dep, "Other").await;

    // Staff-only endpoints.
    for (m, u) in [
        ("GET", "/api/v1/admin/ticket-canned"),
        ("GET", "/api/v1/admin/ticket-sla"),
        ("GET", "/api/v1/admin/ticket-escalations"),
        ("GET", "/api/v1/admin/ticket-staff"),
    ] {
        assert_eq!(
            call(&app, m, u, None, &cust.h()).await.status,
            StatusCode::FORBIDDEN,
            "{u}"
        );
    }

    // Rating requires a staff response, and only the owner can rate.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{a}/rate"),
        Some(json!({"rating": 5})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.ticket_not_rateable");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{a}/reply"),
        Some(json!({"body": "On it"})),
        &admin.h(),
    )
    .await;
    assert!(r.body["ticket"]["first_response_at"].is_string());
    let fr = r.body["ticket"]["first_response_at"].clone();
    // Internal notes don't count, later replies don't move it.
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{b}/reply"),
        Some(json!({"body": "note", "internal": true})),
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{b}"),
        None,
        &admin.h(),
    )
    .await;
    assert!(r.body["ticket"]["first_response_at"].is_null());
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{a}/reply"),
        Some(json!({"body": "Again"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["first_response_at"], fr);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{a}/rate"),
        Some(json!({"rating": 6})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{a}/rate"),
        Some(json!({"rating": 4, "comment": "good"})),
        &other.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{a}/rate"),
        Some(json!({"rating": 4, "comment": "good"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{a}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["rating"], 4);
    assert_eq!(r.body["tags"], json!([]), "clients never see tags");

    // Tags.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/tickets/{a}/tags"),
        Some(json!({"tags": ["VIP Client", "vip-client", "billing"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["tags"], json!(["billing", "vip-client"]));
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/tickets/{a}/tags"),
        Some(json!({"tags": ["!!"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_tag");
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{a}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["tags"], json!(["billing", "vip-client"]));

    // Canned replies.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-canned",
        Some(json!({"title": "Greeting", "body": "Hello!", "department_id": dep})),
        &admin.h(),
    )
    .await;
    let cid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/ticket-canned/{cid}"),
        Some(json!({"title": "Greeting", "body": "Hi there"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["body"], "Hi there");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-canned",
        Some(json!({"title": " ", "body": "x"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "GET", "/api/v1/admin/ticket-canned", None, &admin.h()).await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == cid));
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/ticket-canned/{cid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/ticket-canned/{cid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Split: move the last staff reply into a new ticket; can't move everything.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{a}"),
        None,
        &admin.h(),
    )
    .await;
    let msgs: Vec<String> = r.body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(msgs.len(), 3);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{a}/split"),
        Some(json!({"message_ids": msgs, "subject": "x"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.ticket_split_messages");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{a}/split"),
        Some(json!({"message_ids": [msgs[2]], "subject": "Split off"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let sid = r.body["ticket_id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{sid}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["subject"], "Split off");
    assert_eq!(r.body["messages"].as_array().unwrap().len(), 1);
    assert!(r.body["ticket"]["first_response_at"].is_string());

    // Merge: other customer's ticket refused; B into A moves messages.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{o}/merge"),
        Some(json!({"target_id": a})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.ticket_merge_customer");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{b}/merge"),
        Some(json!({"target_id": b})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{b}/merge"),
        Some(json!({"target_id": a})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{b}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["status"], "closed");
    assert_eq!(r.body["ticket"]["merged_into"], a.as_str());
    assert_eq!(r.body["messages"].as_array().unwrap().len(), 0);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{a}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["messages"].as_array().unwrap().len(), 4);

    // Escalation: rule on this department, ticket idle 10 minutes.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-escalations",
        Some(json!({"name": "late", "department_id": dep, "after_minutes": 0})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_minutes");
    let r = call(&app, "POST", "/api/v1/admin/ticket-escalations", Some(json!({"name": "late", "department_id": dep, "statuses": ["closed"], "after_minutes": 5})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.invalid_status");
    let r = call(&app, "POST", "/api/v1/admin/ticket-escalations", Some(json!({"name": "late", "department_id": dep, "after_minutes": 5, "set_priority": "urgent", "add_tag": "Escalated"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rule = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["statuses"], json!(["open", "customer_reply"]));
    sqlx::query(
        "UPDATE tickets SET last_reply_at = now() - interval '10 minutes' WHERE id = $1::uuid",
    )
    .bind(&o)
    .execute(&state.pool)
    .await
    .unwrap();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-escalations/run",
        None,
        &admin.h(),
    )
    .await;
    let job = r.body["job_id"].as_i64().unwrap();
    for _ in 0..200 {
        pelty_api::worker::run_once(&state, "test-esc", 20)
            .await
            .unwrap();
        let st: String = sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1")
            .bind(job)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        if st == "done" || st == "succeeded" || st == "completed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{o}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["priority"], "urgent", "escalated by job");
    assert_eq!(r.body["tags"], json!(["escalated"]));
    // Fires once per ticket.
    call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{o}"),
        Some(json!({"priority": "low"})),
        &admin.h(),
    )
    .await;
    pelty_database::support::run_escalations(&state.pool)
        .await
        .unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{o}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["priority"], "low");
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/ticket-escalations/{rule}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    // SLA metrics for the department: A and split ticket responded, rating 4.00.
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/ticket-sla?days=1",
        None,
        &admin.h(),
    )
    .await;
    let row = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["department_id"] == dep.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["responded"], 2);
    assert_eq!(row["within_sla"], 2);
    assert_eq!(row["ratings"], 1);
    assert_eq!(row["avg_rating"], "4.00");
    assert_eq!(row["sla_first_response_minutes"], 60);

    // E-mail piping through the mock source.
    let src = MockSource::default();
    let mid = format!("<{}@mail>", uuid::Uuid::new_v4());
    src.0.lock().unwrap().extend([
        InboundMail {
            message_id: mid.clone(),
            from: format!("Me <{}>", cust.email),
            to: "x@y".into(),
            subject: format!("Re: [{a_num}] Ticket A"),
            body: "Still broken\n\nOn Mon, Staff wrote:\n> On it".into(),
            ..Default::default()
        },
        InboundMail {
            message_id: mid.clone(),
            from: cust.email.clone(),
            to: "x@y".into(),
            subject: format!("Re: [Ticket #{a_num}]"),
            body: "dup".into(),
            ..Default::default()
        },
        InboundMail {
            message_id: format!("<{}@mail>", uuid::Uuid::new_v4()),
            from: "stranger@nowhere.test".into(),
            to: pipe_addr.clone(),
            subject: "spam".into(),
            body: "buy".into(),
            ..Default::default()
        },
        InboundMail {
            message_id: format!("<{}@mail>", uuid::Uuid::new_v4()),
            from: cust.email.clone(),
            to: pipe_addr.to_uppercase(),
            subject: "New via mail".into(),
            body: "Hello desk".into(),
            ..Default::default()
        },
        InboundMail {
            message_id: format!("<{}@mail>", uuid::Uuid::new_v4()),
            from: cust.email.clone(),
            to: "nobody@x".into(),
            subject: "No dept".into(),
            body: "Hello".into(),
            ..Default::default()
        },
    ]);
    let st = pipe::process(&state, &src).await.unwrap();
    assert_eq!(
        st,
        pipe::PipeStats {
            opened: 1,
            replied: 1,
            rejected: 2,
            duplicate: 1,
            spam: 0
        }
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{a}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["ticket"]["status"], "customer_reply");
    assert_eq!(
        r.body["messages"].as_array().unwrap().last().unwrap()["body"],
        "Still broken"
    );
    let r = call(&app, "GET", "/api/v1/client/tickets", None, &cust.h()).await;
    let piped: Vec<_> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["subject"] == "New via mail")
        .collect();
    assert_eq!(piped.len(), 1);
    assert_eq!(piped[0]["department_id"], dep.as_str());
    // Re-delivery of the same mail is ignored.
    src.0.lock().unwrap().push(InboundMail {
        message_id: mid,
        from: cust.email.clone(),
        to: "x@y".into(),
        subject: format!("[Ticket #{a_num}]"),
        body: "again".into(),
        ..Default::default()
    });
    assert_eq!(pipe::process(&state, &src).await.unwrap().duplicate, 1);

    // Staff list contains the admin.
    let r = call(&app, "GET", "/api/v1/admin/ticket-staff", None, &admin.h()).await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["email"] == admin.email.as_str()));
}
