use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_database::jobs;
use serde_json::json;

// Single test: the queue is global, so steps must not interleave.
#[tokio::test]
async fn job_queue_retries_dedupes_and_recovers() {
    let (app, st) = app().await;
    let pool = &st.pool;
    // Drain anything left from earlier runs.
    pelty_database::raw_execute(
        pool,
        "UPDATE jobs SET status = 'succeeded' WHERE status IN ('queued','running')",
    )
    .await
    .unwrap();

    // Idempotency key: second enqueue is a no-op.
    let key = format!("k-{}", uuid::Uuid::new_v4());
    let id = jobs::enqueue(pool, "test.ok", json!({}), None, Some(&key), 3)
        .await
        .unwrap()
        .unwrap();
    assert!(
        jobs::enqueue(pool, "test.ok", json!({}), None, Some(&key), 3)
            .await
            .unwrap()
            .is_none()
    );

    // SKIP LOCKED: two concurrent claimers never get the same job.
    for _ in 0..20 {
        jobs::enqueue(pool, "test.ok", json!({}), None, None, 3)
            .await
            .unwrap();
    }
    let (a, b) = tokio::join!(jobs::claim(pool, "w1", 15), jobs::claim(pool, "w2", 15));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.len() + b.len(), 21);
    assert!(a.iter().all(|x| !b.iter().any(|y| y.id == x.id)));
    for j in a.iter().chain(b.iter()) {
        jobs::complete(pool, j.id, json!({})).await.unwrap();
    }
    assert_eq!(
        jobs::get(pool, id).await.unwrap().unwrap().status,
        "succeeded"
    );

    // Failure → backoff → dead after max attempts → manual retry.
    let f = jobs::enqueue(pool, "test.fail", json!({}), None, None, 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pelty_api::worker::run_once(&st, "t", 10).await.unwrap(), 1);
    let j = jobs::get(pool, f).await.unwrap().unwrap();
    assert_eq!((j.status.as_str(), j.attempts), ("queued", 1));
    assert!(
        j.run_at > time::OffsetDateTime::now_utc() + time::Duration::seconds(5),
        "backoff applied"
    );
    assert_eq!(
        pelty_api::worker::run_once(&st, "t", 10).await.unwrap(),
        0,
        "not ready during backoff"
    );
    pelty_database::raw_execute(
        pool,
        &format!("UPDATE jobs SET run_at = now() WHERE id = {f}"),
    )
    .await
    .unwrap();
    pelty_api::worker::run_once(&st, "t", 10).await.unwrap();
    let j = jobs::get(pool, f).await.unwrap().unwrap();
    assert_eq!(j.status, "dead");
    assert_eq!(j.last_error.as_deref(), Some("intentional failure"));
    assert_eq!(jobs::backoff_secs(1), 10);
    assert_eq!(jobs::backoff_secs(3), 40);
    assert_eq!(jobs::backoff_secs(20), 3600);

    // Stuck running job (crashed worker) is recovered.
    let s = jobs::enqueue(pool, "test.ok", json!({}), None, None, 3)
        .await
        .unwrap()
        .unwrap();
    jobs::claim(pool, "dead-worker", 1).await.unwrap();
    pelty_database::raw_execute(
        pool,
        &format!("UPDATE jobs SET locked_at = now() - interval '1 hour' WHERE id = {s}"),
    )
    .await
    .unwrap();
    assert!(jobs::recover_stuck(pool, 600).await.unwrap() >= 1);
    assert_eq!(jobs::get(pool, s).await.unwrap().unwrap().status, "queued");
    pelty_api::worker::run_once(&st, "t", 10).await.unwrap();
    assert_eq!(
        jobs::get(pool, s).await.unwrap().unwrap().status,
        "succeeded"
    );

    // Admin API: list, retry dead, recurring enqueue (deduplicated), permissions.
    let admin = sess(&app, &st, "admin").await;
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/jobs?status=dead",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["id"] == f));
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/jobs/{f}/retry"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["status"], "queued");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/jobs/{s}/retry"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    pelty_database::raw_execute(
        pool,
        &format!("UPDATE jobs SET status = 'succeeded' WHERE id = {f}"),
    )
    .await
    .unwrap();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/jobs/recurring",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    pelty_api::worker::run_once(&st, "t", 10).await.unwrap();
    if let Some(rid) = r.body["id"].as_i64() {
        let j = jobs::get(pool, rid).await.unwrap().unwrap();
        assert_eq!(j.status, "succeeded", "{:?}", j.last_error);
        assert!(j.result.unwrap()["invoices_created"].is_number());
    }
    let first = pelty_api::worker::schedule_recurring(&st).await.unwrap();
    let second = pelty_api::worker::schedule_recurring(&st).await.unwrap();
    assert!(
        second.is_none(),
        "daily recurring job deduplicated (first={first:?})"
    );
    let staff = sess(&app, &st, "staff").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/jobs", None, &staff.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}
