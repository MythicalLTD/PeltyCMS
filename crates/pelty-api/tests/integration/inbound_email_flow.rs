use crate::common::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::Engine;
use hmac::{Hmac, Mac};
use http_body_util::BodyExt;
use pelty_api::{mailbox::parse_message, pipe};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

fn signature(token: &str, timestamp: &str, raw: &str) -> String {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(raw.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Registering a session does not create the lazily provisioned customer row.
/// Materialize it through the client API before exercising known-client intake.
async fn customer_fixture(app: &axum::Router, state: &pelty_api::AppState) -> Sess {
    let client = sess(app, state, "customer").await;
    let profile = call(app, "GET", "/api/v1/client/profile", None, &client.h()).await;
    assert_eq!(
        profile.status,
        StatusCode::OK,
        "customer fixture profile: {:?}",
        profile.body
    );
    let matched = pelty_database::support::customer_by_email(&state.pool, &client.email)
        .await
        .expect("fixture customer email lookup");
    assert!(
        matched.is_some(),
        "fixture sender {} has no customer record after profile creation",
        client.email
    );
    client
}

#[tokio::test]
async fn authenticated_department_is_not_re_resolved_by_address() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let client = customer_fixture(&app, &state).await;
    let mut departments = Vec::new();
    let address = format!("shared-{}@example.test", Uuid::new_v4());
    for _ in 0..2 {
        let id = pelty_database::tickets::upsert_department(
            &state.pool,
            None,
            "Shared mailbox fixture",
            "",
            true,
            0,
        )
        .await
        .unwrap()
        .id;
        sqlx::query("UPDATE ticket_departments SET email=$2 WHERE id=$1")
            .bind(id)
            .bind(&address)
            .execute(&state.pool)
            .await
            .unwrap();
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/support/departments/{id}/inbound-token"),
            None,
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        departments.push((id, r.body["token"].as_str().unwrap().to_string()));
    }
    let ts = time::OffsetDateTime::now_utc().unix_timestamp().to_string();
    let mut tickets = Vec::new();
    for (department, token) in &departments {
        let message_id = format!("{}@example.test", Uuid::new_v4());
        let raw = format!(
            "From: {}\nTo: {address}\nMessage-ID: <{message_id}>\nSubject: Shared address\n\nHelp",
            client.email
        );
        let (status, stats) = deliver(&app, *department, token, &raw, &ts).await;
        assert_eq!(status, StatusCode::OK, "{stats}");
        assert_eq!(
            stats["opened"],
            1,
            "department={department}; stats={stats}; parsed={:?}",
            parse_message(&raw, &address)
        );
        let (ticket, actual_department): (Uuid, Uuid) = sqlx::query_as("SELECT t.id,t.department_id FROM tickets t JOIN ticket_pipe_log l ON l.ticket_id=t.id WHERE l.message_id=$1")
            .bind(message_id).fetch_one(&state.pool).await.unwrap();
        assert_eq!(actual_department, *department);
        tickets.push(ticket);
    }
    let cross_department = format!("From: {}\nMessage-ID: <{}>\nIn-Reply-To: <ticket.{}.{}@pelty.invalid>\nSubject: Cross department\n\nReply", client.email, Uuid::new_v4(), tickets[0], Uuid::new_v4());
    assert_eq!(
        deliver(
            &app,
            departments[1].0,
            &departments[1].1,
            &cross_department,
            &ts
        )
        .await
        .1["rejected"],
        1
    );

    let source = pipe::MockSource::default();
    let mailbox_message_id = Uuid::new_v4().to_string();
    source.0.lock().unwrap().push(parse_message(&format!("From: {}\nTo: {address}\nMessage-ID: <{mailbox_message_id}>\nSubject: Bound mailbox\n\nHelp", client.email), ""));
    assert_eq!(
        pipe::process_in_department(&state, &source, departments[1].0)
            .await
            .unwrap()
            .opened,
        1
    );
    let actual_department: Uuid = sqlx::query_scalar("SELECT t.department_id FROM tickets t JOIN ticket_pipe_log l ON l.ticket_id=t.id WHERE l.message_id=$1")
        .bind(mailbox_message_id).fetch_one(&state.pool).await.unwrap();
    assert_eq!(actual_department, departments[1].0);

    sqlx::query("UPDATE ticket_departments SET is_active=false WHERE id=$1")
        .bind(departments[1].0)
        .execute(&state.pool)
        .await
        .unwrap();
    let reply = parse_message(&format!("From: {}\nMessage-ID: <{}>\nIn-Reply-To: <ticket.{}.{}@pelty.invalid>\nSubject: Inactive\n\nReply", client.email, Uuid::new_v4(), tickets[1], Uuid::new_v4()), "");
    assert_eq!(
        pipe::process_messages_in_department(&state, vec![reply], departments[1].0)
            .await
            .unwrap()
            .rejected,
        1
    );
}

async fn deliver(
    app: &axum::Router,
    dep: Uuid,
    token: &str,
    raw: &str,
    timestamp: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/public/support/inbound/{dep}"))
                .header("content-type", "message/rfc822")
                .header("x-pelty-timestamp", timestamp)
                .header("x-pelty-signature", signature(token, timestamp, raw))
                .body(Body::from(raw.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "inbound response: status={status}; invalid JSON: {error}; body={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, body)
}

#[tokio::test]
async fn signed_intake_threading_guests_limits_and_loops() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let client = customer_fixture(&app, &state).await;
    let other = customer_fixture(&app, &state).await;
    let dep = pelty_database::tickets::upsert_department(
        &state.pool,
        None,
        "Inbound fixture",
        "",
        true,
        0,
    )
    .await
    .unwrap()
    .id;
    let address = format!("{dep}@example.test");
    sqlx::query("UPDATE ticket_departments SET email=$2 WHERE id=$1")
        .bind(dep)
        .bind(&address)
        .execute(&state.pool)
        .await
        .unwrap();
    let url = format!("/api/v1/admin/support/departments/{dep}/inbound-token");
    assert_eq!(
        call(&app, "POST", &url, None, &client.h()).await.status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "POST", &url, None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let token = r.body["token"].as_str().unwrap();
    let settings = call(
        &app,
        "GET",
        "/api/v1/admin/support/departments",
        None,
        &admin.h(),
    )
    .await;
    assert!(!settings.body.to_string().contains(token));
    let unique = Uuid::new_v4().to_string();
    let raw = include_str!("../fixtures/inbound-mixed.eml")
        .replace("ana@example.test", &client.email)
        .replace("fixture-mixed", &unique);
    let ts = time::OffsetDateTime::now_utc().unix_timestamp().to_string();
    assert_eq!(
        deliver(&app, dep, "wrong", &raw, &ts).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        deliver(&app, dep, token, &raw, "1").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, stats) = deliver(&app, dep, token, &raw, &ts).await;
    assert_eq!(status, StatusCode::OK, "{stats}");
    assert_eq!(
        stats["opened"],
        1,
        "department={dep}; stats={stats}; parsed={:?}",
        parse_message(&raw, &address)
    );
    assert_eq!(deliver(&app, dep, token, &raw, &ts).await.1["duplicate"], 1);
    let tid: Uuid = sqlx::query_scalar("SELECT ticket_id FROM ticket_pipe_log WHERE message_id=$1")
        .bind(format!("{unique}@example.test"))
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let outbox: Vec<pelty_database::jobs::Job> =
        sqlx::query_as("SELECT * FROM jobs WHERE kind=$1 AND payload->>'ticket_id'=$2")
            .bind(pelty_database::support_ext::INBOUND_NOTIFY_JOB)
            .bind(tid.to_string())
            .fetch_all(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        outbox.len(),
        1,
        "duplicate delivery must not enqueue a second outbox job"
    );
    assert_eq!(outbox[0].status, "queued");
    let claimed: pelty_database::jobs::Job = sqlx::query_as(
        "UPDATE jobs SET status='running', attempts=attempts+1 WHERE id=$1 RETURNING *",
    )
    .bind(outbox[0].id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    pelty_api::worker::run_claimed(&state, &[claimed])
        .await
        .unwrap();
    assert_eq!(
        pelty_database::jobs::get(&state.pool, outbox[0].id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "succeeded"
    );
    let messages = pelty_database::tickets::messages(&state.pool, tid, true)
        .await
        .unwrap();
    assert_eq!(messages[0].body, "Please help!");
    let files = pelty_database::ticket_attachments::for_ticket(&state.pool, tid, true)
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].filename, "diagnostic.txt");
    let reply = include_str!("../fixtures/inbound-reply.eml")
        .replace("ana@example.test", &client.email)
        .replace("fixture-mixed", &unique)
        .replace("fixture-reply", &Uuid::new_v4().to_string());
    assert_eq!(deliver(&app, dep, token, &reply, &ts).await.1["replied"], 1);
    let outgoing_reply = format!(
        "From: {}
Message-ID: <{}>
In-Reply-To: <ticket.{tid}.{}@pelty.invalid>
Subject: Changed again

More information",
        client.email,
        Uuid::new_v4(),
        Uuid::new_v4()
    );
    assert_eq!(
        deliver(&app, dep, token, &outgoing_reply, &ts).await.1["replied"],
        1
    );
    let forged_outgoing = outgoing_reply.replace(&client.email, &other.email).replace(
        "Message-ID:",
        &format!("Message-ID: <{}>\nX-Old-Message-ID:", Uuid::new_v4()),
    );
    assert_eq!(
        deliver(&app, dep, token, &forged_outgoing, &ts).await.1["rejected"],
        1
    );
    let forged = reply.replace(&client.email, &other.email).replace(
        "Message-ID:",
        &format!("Message-ID: <{}>\nX-Old-Message-ID:", Uuid::new_v4()),
    );
    assert_eq!(
        deliver(&app, dep, token, &forged, &ts).await.1["rejected"],
        1
    );

    let guest_raw = raw
        .replace(&client.email, "visitor@example.test")
        .replace(&unique, &Uuid::new_v4().to_string());
    assert_eq!(
        deliver(&app, dep, token, &guest_raw, &ts).await.1["rejected"],
        1
    );
    sqlx::query("UPDATE ticket_departments SET pipe_unknown_sender='guest' WHERE id=$1")
        .bind(dep)
        .execute(&state.pool)
        .await
        .unwrap();
    let guest_raw = guest_raw.replace(
        "Message-ID:",
        &format!("Message-ID: <{}>\nX-Old-Message-ID:", Uuid::new_v4()),
    );
    assert_eq!(
        deliver(&app, dep, token, &guest_raw, &ts).await.1["opened"],
        1
    );
    let guest: Option<Uuid> = sqlx::query_scalar("SELECT customer_id FROM tickets WHERE department_id=$1 AND guest_email='visitor@example.test'").bind(dep).fetch_one(&state.pool).await.unwrap();
    assert!(guest.is_none());
    let guest_message_id = parse_message(&guest_raw, &address).message_id;
    let guest_reply = format!(
        "From: visitor@example.test
Message-ID: <{}>
References: <{guest_message_id}>
Subject: Guest follow-up

Thanks",
        Uuid::new_v4()
    );
    assert_eq!(
        deliver(&app, dep, token, &guest_reply, &ts).await.1["replied"],
        1
    );
    let automatic = include_str!("../fixtures/inbound-auto.eml")
        .replace("fixture-auto", &Uuid::new_v4().to_string());
    assert_eq!(
        deliver(&app, dep, token, &automatic, &ts).await.1["rejected"],
        1
    );
    let mut oversized = parse_message(&raw, &address);
    oversized.message_id = Uuid::new_v4().to_string();
    oversized.attachments[0].data =
        vec![b'x'; pelty_api::routes::ticket_attachments::max_bytes() + 1];
    assert_eq!(
        pipe::process_messages(&state, vec![oversized])
            .await
            .unwrap()
            .rejected,
        1
    );
    // Both the pending job and domain changes obey transaction rollback.
    let mut tx = state.pool.begin().await.unwrap();
    let rollback_key = format!("inbound-test:{}", Uuid::new_v4());
    let rolled_back_job = pelty_database::jobs::enqueue_in(
        &mut tx,
        pelty_database::support_ext::INBOUND_NOTIFY_JOB,
        json!({}),
        &rollback_key,
        5,
    )
    .await
    .unwrap()
    .unwrap();
    tx.rollback().await.unwrap();
    assert!(pelty_database::jobs::get(&state.pool, rolled_back_job)
        .await
        .unwrap()
        .is_none());

    let rollback_id = Uuid::new_v4().to_string();
    let customer = pelty_database::support::customer_by_email(&state.pool, &client.email)
        .await
        .unwrap();
    let bad_files = [pelty_database::support_ext::InboundAttachment {
        filename: "x".repeat(201),
        content_type: "text/plain".into(),
        data: b"log".to_vec(),
    }];
    let failed = pelty_database::support_ext::ingest_email(
        &state.pool,
        pelty_database::support_ext::InboundTicket {
            message_id: &rollback_id,
            sender: &client.email,
            subject: "Rollback fixture",
            body: "Help",
            department: Some(dep),
            customer,
            number: None,
            references: &[],
            reject: false,
            spam: false,
            attachments: &bad_files,
        },
    )
    .await;
    assert!(failed.is_err());
    let claimed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ticket_pipe_log WHERE message_id=$1)")
            .bind(&rollback_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(
        !claimed,
        "a failed intake must roll back the Message-ID claim"
    );

    let rotated = call(&app, "POST", &url, Some(json!({})), &admin.h()).await;
    assert_eq!(rotated.status, StatusCode::OK);
    assert_eq!(
        deliver(&app, dep, token, &raw, &ts).await.0,
        StatusCode::UNAUTHORIZED
    );
}
