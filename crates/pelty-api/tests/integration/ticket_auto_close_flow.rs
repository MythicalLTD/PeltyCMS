use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

#[tokio::test]
async fn inactive_tickets_close_and_notify() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let tag = format!("ac{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let ch = cust.h();
    let mk = |s: String| {
        call(
            &app,
            "POST",
            "/api/v1/client/tickets",
            Some(json!({"department_id": SUPPORT, "subject": s, "body": "x"})),
            &ch,
        )
    };
    assert_eq!(mk(format!("idle {tag}")).await.status, StatusCode::OK);
    assert_eq!(mk(format!("hold {tag}")).await.status, StatusCode::OK);
    assert_eq!(mk(format!("fresh {tag}")).await.status, StatusCode::OK);
    let id = |p: &'static str| {
        let pool = st.pool.clone();
        let tag = tag.clone();
        async move {
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM tickets WHERE subject = $1")
                .bind(format!("{p} {tag}"))
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let (idle, hold, fresh) = (id("idle").await, id("hold").await, id("fresh").await);
    sqlx::query("UPDATE tickets SET status = 'answered', last_reply_at = now() - interval '50 hours' WHERE id = $1").bind(idle).execute(&st.pool).await.unwrap();
    sqlx::query("UPDATE tickets SET status = 'on_hold', last_reply_at = now() - interval '50 hours' WHERE id = $1").bind(hold).execute(&st.pool).await.unwrap();
    sqlx::query("UPDATE tickets SET status = 'answered', last_reply_at = now() - interval '10 hours' WHERE id = $1").bind(fresh).execute(&st.pool).await.unwrap();

    // Staff only, validated.
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/ticket-auto-close",
            Some(json!({"hours": 48})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/ticket-auto-close",
            Some(json!({"hours": -1})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/ticket-auto-close",
            Some(json!({"hours": 48, "statuses": ["closed"]})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let g = call(
        &app,
        "GET",
        "/api/v1/admin/settings/ticket-auto-close",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(g.body["statuses"], json!(["answered", "customer_reply"]));

    // Off by default: nothing closes.
    let _ = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/ticket-auto-close",
        Some(json!({"hours": 0})),
        &admin.h(),
    )
    .await;
    pelty_api::routes::support::run_auto_close(&st)
        .await
        .unwrap();
    let status = |t| {
        let pool = st.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT status FROM tickets WHERE id = $1")
                .bind(t)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(status(idle).await, "answered");

    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/ticket-auto-close",
        Some(json!({"hours": 48})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    pelty_api::routes::support::run_auto_close(&st)
        .await
        .unwrap();
    assert_eq!(status(idle).await, "closed");
    assert_eq!(status(hold).await, "on_hold", "on hold is exempt");
    assert_eq!(status(fresh).await, "answered", "not idle long enough");
    let mails = mails_to(&st, &cust.email).await;
    assert!(
        mails.iter().any(|m| m.subject.contains("inactivity")),
        "{:?}",
        mails.iter().map(|m| &m.subject).collect::<Vec<_>>()
    );
    let _ = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/ticket-auto-close",
        Some(json!({"hours": 0})),
        &admin.h(),
    )
    .await;
}
