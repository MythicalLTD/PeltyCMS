//! Real local OpenAI-compatible HTTP endpoint; no AI env mutation or paid calls.
use crate::common::*;
use axum::{
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use pelty_core::config::{AiConfig, Redacted};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn opt_in_permissions_bounded_context_audit_and_no_send() {
    let (app, mut st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let customer = sess(&app, &st, "customer").await;
    Arc::make_mut(&mut st.cfg).ai = AiConfig::default();
    let app = pelty_api::router(st.clone());
    let status = "/api/v1/admin/ticket-ai/status";
    assert_eq!(
        call(&app, "GET", status, None, &[]).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", status, None, &customer.h()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", status, None, &admin.h()).await.body,
        json!({"enabled": false})
    );
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let subject = format!("AI issue {tag}");
    let category: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO kb_categories(slug,name) VALUES ($1,'AI test') RETURNING id",
    )
    .bind(format!("ai-{tag}"))
    .fetch_one(&st.pool)
    .await
    .unwrap();
    for n in 0..5 {
        sqlx::query("INSERT INTO kb_articles(category_id,slug,title,body,is_published) VALUES ($1,$2,$3,$4,$5)")
            .bind(category).bind(format!("ai-{tag}-{n}")).bind(&subject)
            .bind(if n == 4 { "UNPUBLISHED KB MUST NOT LEAVE".into() } else { "Published guidance ".repeat(300) })
            .bind(n < 4).execute(&st.pool).await.unwrap();
    }
    let ticket = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": "00000000-0000-0000-0002-000000000001", "subject": subject, "body": "Customer question"})), &customer.h()).await;
    assert_eq!(ticket.status, StatusCode::OK, "{}", ticket.body);
    let id = ticket.body["ticket"]["id"].as_str().unwrap();
    let path = format!("/api/v1/admin/tickets/{id}/suggest");
    assert_eq!(
        call(&app, "POST", &path, None, &admin.h()).await.body["error"]["code"],
        "error.ai_disabled"
    );
    let tid: uuid::Uuid = id.parse().unwrap();
    sqlx::query("INSERT INTO ticket_messages(ticket_id, is_staff, is_internal, body) VALUES ($1,true,true,'PRIVATE NOTE MUST NOT LEAVE')").bind(tid).execute(&st.pool).await.unwrap();
    for _ in 0..14 {
        sqlx::query("INSERT INTO ticket_messages(ticket_id, is_staff, body) VALUES ($1,false,$2)")
            .bind(tid)
            .bind("ă".repeat(3000))
            .execute(&st.pool)
            .await
            .unwrap();
    }
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM ticket_messages WHERE ticket_id=$1")
        .bind(tid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = received.clone();
    let mock = Router::new().route("/v1/chat/completions", post(move |headers: HeaderMap, Json(payload): Json<Value>| {
        let seen = seen.clone();
        async move {
            assert_eq!(headers["authorization"], "Bearer test-ai-key");
            let mut seen = seen.lock().unwrap();
            seen.push(payload);
            match seen.len() {
                1 => (StatusCode::OK, Json(json!({"choices":[{"message":{"content":" Please check the service status. "}}]}))),
                2 => (StatusCode::UNAUTHORIZED, Json(json!({"secret":"never expose provider details"}))),
                3 => (StatusCode::OK, Json(json!({"choices":[]}))),
                _ => (StatusCode::OK, Json(json!({"choices":[{"message":{"content":"x".repeat(70000)}}]}))),
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });
    Arc::make_mut(&mut st.cfg).ai = AiConfig {
        base_url: Some(format!("http://{address}/v1")),
        api_key: Some(Redacted("test-ai-key".into())),
        model: Some("test-model".into()),
    };
    let app = pelty_api::router(st.clone());
    assert_eq!(
        call(&app, "GET", status, None, &admin.h()).await.body,
        json!({"enabled":true})
    );
    assert_eq!(
        call(&app, "POST", &path, None, &customer.h()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/tickets/{}/suggest", uuid::Uuid::new_v4()),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let result = call(&app, "POST", &path, None, &admin.h()).await;
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body["body"], "Please check the service status.");
    {
        let seen = received.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let payload = &seen[0];
        assert_eq!(payload["model"], "test-model");
        assert_eq!(payload["stream"], false);
        let context: Value =
            serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(context["thread"].as_array().unwrap().len(), 12);
        assert!(context["thread"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["body"].as_str().unwrap().chars().count() <= 2000));
        assert_eq!(context["knowledge_base"].as_array().unwrap().len(), 3);
        assert!(context["knowledge_base"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["body"].as_str().unwrap().chars().count() <= 2000));
        assert!(!context.to_string().contains("UNPUBLISHED KB"));
        assert!(!context.to_string().contains("PRIVATE NOTE"));
        assert!(!context.to_string().contains("customer_email"));
        assert!(context["department"]["name"].is_string());
    }
    for _ in 0..3 {
        let failure = call(&app, "POST", &path, None, &admin.h()).await;
        assert_eq!(failure.status, StatusCode::CONFLICT);
        assert_eq!(failure.body["error"]["code"], "error.ai_unavailable");
        assert!(!failure.body.to_string().contains("never expose"));
    }
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM ticket_messages WHERE ticket_id=$1")
        .bind(tid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    assert_eq!(before, after, "AI never posts a reply");
    let logs: Vec<Value> = sqlx::query_scalar(
        "SELECT metadata FROM audit_logs WHERE target=$1 AND action LIKE 'ticket.ai_%'",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await
    .unwrap();
    assert_eq!(logs.len(), 8);
    assert!(
        logs.iter()
            .all(|v| !v.to_string().contains("test-ai-key")
                && !v.to_string().contains("Please check"))
    );
    server.abort();
}
