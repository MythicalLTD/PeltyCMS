use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_api::mailbox::ScriptedConnector;
use serde_json::json;
use std::sync::Arc;

fn pop3_script(c: &ScriptedConnector, msgs: &[String]) {
    let mut steps = vec![
        ("USER", "+OK\r\n".to_string()),
        ("PASS", "+OK\r\n".to_string()),
    ];
    let list: String = (1..=msgs.len()).map(|i| format!("{i} 100\r\n")).collect();
    steps.push(("LIST", format!("+OK\r\n{list}.\r\n")));
    let cmds: Vec<(String, String)> = msgs
        .iter()
        .enumerate()
        .flat_map(|(i, m)| {
            let n = i + 1;
            vec![
                (
                    format!("RETR {n}"),
                    format!("+OK\r\n{}\r\n.\r\n", m.replace("\r\n.", "\r\n..")),
                ),
                (format!("DELE {n}"), "+OK\r\n".to_string()),
            ]
        })
        .collect();
    let mut all: Vec<(String, String)> =
        steps.into_iter().map(|(a, b)| (a.to_string(), b)).collect();
    all.extend(cmds);
    all.push(("QUIT".into(), "+OK\r\n".into()));
    if let Ok(mut g) = c.script.lock() {
        *g = Some(("+OK ready\r\n".into(), all));
    }
}

#[tokio::test]
async fn support_ext_end_to_end() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-departments",
        Some(json!({"name": format!("Pipe {}", uuid::Uuid::new_v4().simple())})),
        &admin.h(),
    )
    .await;
    let dep = r.body["id"].as_str().unwrap().to_string();
    let addr = format!("pipe-{}@example.test", uuid::Uuid::new_v4().simple());
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/ticket-departments/{dep}/sla"),
        Some(json!({"email": addr, "sla_first_response_minutes": 60})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Department settings: validation, write-only sealed password.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/support/departments",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = json!({"auto_reply": true, "pipe_protocol": "smtp"});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/support/departments/{dep}"),
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let bad = json!({"auto_reply": true, "pipe_protocol": "pop3", "pipe_host": "x/y", "pipe_port": 995, "pipe_username": "u"});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/support/departments/{dep}"),
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let ok = json!({"auto_reply": true, "auto_reply_text": "We got it.", "pipe_protocol": "pop3", "pipe_host": "pop.example.test", "pipe_port": 995, "pipe_tls": true, "pipe_username": "u", "pipe_password": "secret"});
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/support/departments/{dep}"),
        Some(ok),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body.get("pipe_password").is_none());
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/support/departments",
        None,
        &admin.h(),
    )
    .await;
    let d = r.body["departments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == dep.as_str())
        .unwrap()
        .clone();
    assert_eq!(d["has_password"], true);
    assert!(d.get("pipe_password").is_none());
    let sealed: String =
        sqlx::query_scalar("SELECT pipe_password FROM ticket_departments WHERE id = $1::uuid")
            .bind(&dep)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(!sealed.is_empty() && !sealed.contains("secret"));

    // Spam rules.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/support/spam-rules",
        Some(json!({"kind": "nope", "value": "x"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let spam_dom = format!("spam{}.test", uuid::Uuid::new_v4().simple());
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/support/spam-rules",
        Some(json!({"kind": "domain", "value": spam_dom})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rule = r.body["id"].as_str().unwrap().to_string();

    // Client-opened tickets get the department auto-reply.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(json!({"department_id": dep, "subject": "Help", "body": "x"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let tid = r.body["ticket"]["id"].as_str().unwrap().to_string();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE payload->>'template' = 'ticket_autoreply' AND payload->>'to' = $1").bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 1);

    // Mail piping through a scripted POP3 server: one new ticket, one spam.
    let c = ScriptedConnector::default();
    let id1 = uuid::Uuid::new_v4().simple().to_string();
    let id2 = uuid::Uuid::new_v4().simple().to_string();
    pop3_script(&c, &[
        format!("Message-ID: <{id1}@x>\r\nFrom: {}\r\nTo: {addr}\r\nSubject: Mail ticket\r\n\r\nPlease help", cust.email),
        format!("Message-ID: <{id2}@x>\r\nFrom: bot@{spam_dom}\r\nTo: {addr}\r\nSubject: Buy now\r\n\r\nspam"),
    ]);
    let mut st2 = state.clone();
    st2.mailbox = Arc::new(c.clone());
    let out = pelty_api::support_ext::run_pipe(&st2, Some(dep.parse().unwrap()))
        .await
        .map_err(|e| e.0)
        .unwrap();
    let d = &out["departments"][0];
    assert_eq!(
        (d["opened"].as_u64(), d["spam"].as_u64()),
        (Some(1), Some(1)),
        "{out:?}"
    );
    assert!(
        c.seen.lock().unwrap().iter().any(|l| l == "PASS secret"),
        "password was unsealed"
    );
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/support/spam-rules",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == rule.as_str())
            .unwrap()["hits"],
        1
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE payload->>'template' = 'ticket_autoreply' AND payload->>'to' = $1").bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    assert_eq!(
        n, 1,
        "piped notifications wait for the committed outbox job"
    );
    let job: pelty_database::jobs::Job = sqlx::query_as(
        "UPDATE jobs SET status='running', attempts=attempts+1 WHERE kind=$1
         AND payload->>'ticket_id'=(SELECT ticket_id::text FROM ticket_pipe_log WHERE message_id=$2)
         RETURNING *",
    )
    .bind(pelty_database::support_ext::INBOUND_NOTIFY_JOB)
    .bind(format!("{id1}@x"))
    .fetch_one(&state.pool)
    .await
    .unwrap();
    pelty_api::worker::run_claimed(&state, &[job])
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE payload->>'template' = 'ticket_autoreply' AND payload->>'to' = $1")
        .bind(&cust.email).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 2, "piped ticket auto-replied through the outbox worker");
    // A failing mailbox is recorded on the department, not fatal.
    let out = pelty_api::support_ext::run_pipe(&st2, Some(dep.parse().unwrap()))
        .await
        .map_err(|e| e.0)
        .unwrap();
    assert!(out["departments"][0]["error"].is_string());
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/support/departments",
        None,
        &admin.h(),
    )
    .await;
    let d = r.body["departments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == dep.as_str())
        .unwrap()
        .clone();
    assert!(!d["pipe_last_error"].as_str().unwrap().is_empty());
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/support/spam-rules/{rule}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    // Time tracking.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{tid}/time"),
        Some(json!({"minutes": 0})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{tid}/time"),
        Some(json!({"minutes": 30, "billable": true, "note": "debug"})),
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{tid}/time"),
        Some(json!({"minutes": 15})),
        &admin.h(),
    )
    .await;
    let e2 = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{tid}/time"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (
            r.body["total_minutes"].as_i64(),
            r.body["billable_minutes"].as_i64()
        ),
        (Some(45), Some(30))
    );
    call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/tickets/{tid}/time/{e2}"),
        None,
        &admin.h(),
    )
    .await;
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{tid}/time"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["total_minutes"], 30);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/tickets/{tid}/time"),
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // Watchers: staff only; client replies notify them.
    let staff2 = sess(&app, &state, "admin").await;
    let staff2_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&staff2.email)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let cust_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&cust.email)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/tickets/{tid}/watchers"),
        Some(json!({"user_ids": [staff2_id, cust_id]})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.body["watchers"],
        json!([staff2_id]),
        "customers are dropped"
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{tid}/reply"),
        Some(json!({"body": "any news?"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE payload->>'template' = 'ticket_staff_notify' AND payload->>'to' = $1").bind(&staff2.email).fetch_one(&state.pool).await.unwrap();
    assert!(n >= 1);

    // Contact form: honeypot -> spam (still 200), real -> new, convert to ticket.
    let r = call(
        &app,
        "POST",
        "/api/v1/public/contact",
        Some(json!({"name": "", "email": "x", "message": ""})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        "/api/v1/public/contact",
        Some(
            json!({"name": "Bot", "email": cust.email, "message": "hi", "website": "http://spam"}),
        ),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(&app, "POST", "/api/v1/public/contact", Some(json!({"name": "Ana", "email": cust.email, "subject": "Pre-sales", "message": "Do you sell X?"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/contact-messages",
        None,
        &admin.h(),
    )
    .await;
    let mine: Vec<_> = r.body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["email"] == cust.email.as_str())
        .cloned()
        .collect();
    assert_eq!(mine.len(), 2);
    assert!(mine.iter().any(|m| m["status"] == "spam"));
    let real = mine.iter().find(|m| m["status"] == "new").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/contact-messages/{real}/ticket"),
        Some(json!({"department_id": dep})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["number"].is_i64());
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/contact-messages/{real}/ticket"),
        Some(json!({"department_id": dep})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/contact-messages",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // RSS feed is well-formed and escapes content.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(json!({"title": "A & B <news>", "body": "x", "is_published": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::get("/api/v1/public/announcements/rss.xml")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/rss+xml"));
    let x =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(x.starts_with("<?xml") && x.contains("<rss version=\"2.0\">"));
    assert!(x.contains("A &amp; B &lt;news&gt;"));

    // Automation settings.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/automation/tickets.escalate/settings",
        Some(json!({"enabled": false, "run_hour": 3})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "run hour only for daily tasks"
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/automation/billing.recurring/settings",
        Some(json!({"enabled": true, "run_hour": 4})),
        &admin.h(),
    )
    .await;
    assert!(r.status == StatusCode::OK || r.status == StatusCode::NOT_FOUND);
    let r = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/automation/{}/settings",
            pelty_api::support_ext::PIPE_JOB
        ),
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "GET", "/api/v1/admin/automation", None, &admin.h()).await;
    let t = r.body["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == pelty_api::support_ext::PIPE_JOB)
        .unwrap()
        .clone();
    assert_eq!(t["enabled"], false);
    let g = pelty_api::routes::automation::gate(&state).await;
    assert!(!g.allows(
        pelty_api::support_ext::PIPE_JOB,
        time::OffsetDateTime::now_utc()
    ));
    call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/automation/{}/settings",
            pelty_api::support_ext::PIPE_JOB
        ),
        Some(json!({"enabled": true})),
        &admin.h(),
    )
    .await;
}
