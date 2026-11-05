use crate::common;
use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn automation_dashboard_and_run_now() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/automation", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", "/api/v1/admin/automation", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tasks = r.body["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 9);
    assert!(tasks
        .iter()
        .any(|t| t["kind"] == "usage.bill" && t["interval_secs"] == 3600));

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/automation/test.ok/run",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/automation/usage.bill/run",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/automation/usage.bill/run",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let id = r.body["id"].as_i64();
    let r2 = call(
        &app,
        "POST",
        "/api/v1/admin/automation/usage.bill/run",
        None,
        &admin.h(),
    )
    .await;
    // Same minute: deduplicated (unless the minute rolled over between calls).
    if r2.body["duplicate"] == true {
        assert!(r2.body["id"].is_null());
    }
    if let Some(id) = id {
        sqlx::query("UPDATE jobs SET status = 'succeeded', finished_at = now(), result = '{\"invoices\":0}' WHERE id = $1").bind(id).execute(&state.pool).await.unwrap();
        let r = call(&app, "GET", "/api/v1/admin/automation", None, &admin.h()).await;
        let t = r.body["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["kind"] == "usage.bill")
            .unwrap()
            .clone();
        assert_eq!(t["health"], "ok", "{t:?}");
        assert!(t["stats"]["succeeded_24h"].as_i64().unwrap() >= 1);
    }
    let r = call(&app, "GET", "/api/v1/admin/automation", None, &admin.h()).await;
    assert!(r.body["totals"].is_object());
}

#[tokio::test]
async fn cron_schedules_preview_queue_and_history() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let task = "uploads.purge_orphans";
    let bad = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/schedule"),
        Some(serde_json::json!({ "schedule": "0 0 3 * * *" })),
        &admin.h(),
    )
    .await;
    assert_eq!(
        bad.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        bad.body
    );
    let p = call(
        &app,
        "POST",
        "/api/v1/admin/automation/cron-preview",
        Some(serde_json::json!({ "schedule": "30 2 * * 1" })),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let next = p.body["next_runs"].as_array().unwrap();
    assert_eq!(next.len(), 5);
    assert_eq!(
        next[1].as_i64().unwrap() - next[0].as_i64().unwrap(),
        7 * 86_400
    );
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/schedule"),
        Some(serde_json::json!({ "schedule": "* * * * *" })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let g = pelty_api::routes::automation::gate(&state).await;
    assert!(
        !g.allows(task, pelty_api::routes::automation::local_now()),
        "cron tasks skip the interval path"
    );
    // Saved two minutes ago: the last matching minute is due exactly once.
    sqlx::query(
        "UPDATE automation_settings SET updated_at = now() - interval '2 minutes' WHERE kind = $1",
    )
    .bind(task)
    .execute(&state.pool)
    .await
    .unwrap();
    let g = pelty_api::routes::automation::gate(&state).await;
    pelty_api::routes::automation::cron_tick(&state, &g).await;
    pelty_api::routes::automation::cron_tick(&state, &g).await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = $1 AND idempotency_key LIKE $2 AND created_at > now() - interval '1 minute'").bind(task).bind(format!("cron:{task}:%")).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 1);
    let l = call(
        &app,
        "GET",
        "/api/v1/admin/automation/tasks",
        None,
        &admin.h(),
    )
    .await;
    let t = l.body["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == task)
        .unwrap()
        .clone();
    assert_eq!(t["schedule"], "* * * * *");
    assert_eq!(t["default_schedule"], "0 * * * *");
    assert_eq!(t["next_runs"].as_array().unwrap().len(), 3);
    let h = call(
        &app,
        "GET",
        &format!("/api/v1/admin/automation/{task}/history?limit=5"),
        None,
        &admin.h(),
    )
    .await;
    assert!(
        !h.body["runs"].as_array().unwrap().is_empty(),
        "{:?}",
        h.body
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/automation/nope/history",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // Back to the built-in interval.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/schedule"),
        Some(serde_json::json!({ "schedule": null })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(pelty_api::routes::automation::gate(&state)
        .await
        .allows(task, pelty_api::routes::automation::local_now()));
}

/// Circuit breaker (disable after `PELTY_CRON_MAX_FAILURES` failed runs in a
/// row, audit, reset) and the `run_once` missed-run policy.
#[tokio::test]
async fn cron_breaker_and_catch_up() {
    use pelty_api::routes::automation as a;
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let task = "domains.daily";
    sqlx::query("DELETE FROM automation_settings WHERE kind = $1")
        .bind(task)
        .execute(&state.pool)
        .await
        .unwrap();
    let get = || async {
        let l = call(
            &app,
            "GET",
            "/api/v1/admin/automation/tasks",
            None,
            &admin.h(),
        )
        .await;
        l.body["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == task)
            .unwrap()
            .clone()
    };
    for _ in 0..4 {
        a::record_outcome(&state, task, Some("boom")).await;
    }
    let t = get().await;
    assert_eq!(
        (
            t["failures"].clone(),
            t["enabled"].clone(),
            t["tripped_at"].is_null()
        ),
        (serde_json::json!(4), serde_json::json!(true), true),
        "{t:?}"
    );
    // A success clears the streak.
    a::record_outcome(&state, task, None).await;
    assert_eq!(get().await["failures"], 0);
    for _ in 0..5 {
        a::record_outcome(&state, task, Some("still broken")).await;
    }
    let t = get().await;
    assert_eq!(
        (t["enabled"].clone(), t["tripped_error"].clone()),
        (serde_json::json!(false), serde_json::json!("still broken")),
        "{t:?}"
    );
    assert!(t["tripped_at"].is_i64());
    assert!(
        !a::gate(&state).await.allows(task, a::local_now()),
        "tripped tasks are not scheduled"
    );
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'automation.task_tripped' AND target = $1",
    )
    .bind(task)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(n >= 1);
    // Further failures do not trip (or mail) again.
    a::record_outcome(&state, task, Some("again")).await;
    let n2: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'automation.task_tripped' AND target = $1",
    )
    .bind(task)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(n, n2);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/automation/{task}/reset"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let t = get().await;
    assert_eq!(
        (
            t["enabled"].clone(),
            t["failures"].clone(),
            t["tripped_at"].is_null()
        ),
        (serde_json::json!(true), serde_json::json!(0), true)
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/automation/nope/reset",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Missed runs: a yearly schedule whose last time has no run.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/schedule"),
        Some(serde_json::json!({ "schedule": "0 0 1 1 *" })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let bad = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/missed"),
        Some(serde_json::json!({ "missed": "always" })),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    sqlx::query("DELETE FROM jobs WHERE kind = $1")
        .bind(task)
        .execute(&state.pool)
        .await
        .unwrap();
    assert!(
        !a::catch_up(&state).await.contains(&task.to_string()),
        "skip is the default"
    );
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/missed"),
        Some(serde_json::json!({ "missed": "run_once" })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(get().await["missed"], "run_once");
    assert!(a::catch_up(&state).await.contains(&task.to_string()));
    assert!(
        !a::catch_up(&state).await.contains(&task.to_string()),
        "only once"
    );
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = $1 AND payload ? 'catch_up'")
            .bind(task)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/schedule"),
        Some(serde_json::json!({ "schedule": null })),
        &admin.h(),
    )
    .await;
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/automation/{task}/missed"),
        Some(serde_json::json!({ "missed": "skip" })),
        &admin.h(),
    )
    .await;
}
