use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn prunes_old_attachments_and_syncs_client_status() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let pool = &st.pool;
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/housekeeping", Some(json!({"prune_attachments_days": -1})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/settings/housekeeping", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "PUT", "/api/v1/admin/settings/housekeeping", Some(json!({"prune_attachments_days": 30, "client_status_sync": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Customer with no services: becomes inactive.
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&cust.email).fetch_one(pool).await.unwrap();
    let cid = pelty_database::customers::id_for_user(pool, uid).await.unwrap();
    // Two closed tickets with an attachment each: one old, one recent.
    let mut msgs = vec![];
    for age in [60, 5] {
        let t = uuid::Uuid::new_v4();
        let m = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO tickets (id, customer_id, department_id, subject, status, closed_at) VALUES ($1,$2,(SELECT id FROM ticket_departments LIMIT 1),'hk','closed', now() - make_interval(days => $3))")
            .bind(t).bind(cid).bind(age).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO ticket_messages (id, ticket_id, author_id, is_staff, body) VALUES ($1,$2,$3,false,'x')").bind(m).bind(t).bind(uid).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO ticket_attachments (uploaded_by, ticket_id, message_id, filename, content_type, size_bytes, data) VALUES ($1,$2,$3,'a.txt','text/plain',1,'x')")
            .bind(uid).bind(t).bind(m).execute(pool).await.unwrap();
        msgs.push(m);
    }
    let r = pelty_database::housekeeping::run(pool).await.unwrap();
    assert!(r.attachments_pruned >= 1);
    let left = |m: uuid::Uuid| async move { sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ticket_attachments WHERE message_id = $1").bind(m).fetch_one(pool).await.unwrap() };
    assert_eq!(left(msgs[0]).await, 0, "old attachment pruned");
    assert_eq!(left(msgs[1]).await, 1, "recent attachment kept");
    let status = || async { sqlx::query_scalar::<_, String>("SELECT status FROM customers WHERE id = $1").bind(cid).fetch_one(pool).await.unwrap() };
    assert_eq!(status().await, "inactive");

    // Log retention: 1..29 days is refused; old log rows go, recent ones stay.
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/housekeeping", Some(json!({"log_retention_days": 7})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mk = |age: i32| async move {
        sqlx::query_scalar::<_, uuid::Uuid>("INSERT INTO email_log (recipient, template, locale, subject, body_text, status, mailer, created_at) VALUES ('hk@example.test', 'x', 'en', 's', 'b', 'sent', 'log', now() - make_interval(days => $1)) RETURNING id")
            .bind(age).fetch_one(pool).await.unwrap()
    };
    let (old, recent) = (mk(400).await, mk(1).await);
    let r = call(&app, "PUT", "/api/v1/admin/settings/housekeeping", Some(json!({"log_retention_days": 365})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rep = pelty_database::housekeeping::run(pool).await.unwrap();
    assert!(rep.log_rows_pruned >= 1);
    let exists = |id: uuid::Uuid| async move { sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM email_log WHERE id = $1)").bind(id).fetch_one(pool).await.unwrap() };
    assert!(!exists(old).await);
    assert!(exists(recent).await);

    let r = call(&app, "PUT", "/api/v1/admin/settings/housekeeping", Some(json!({"prune_attachments_days": 0, "client_status_sync": false, "log_retention_days": 0})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
}
