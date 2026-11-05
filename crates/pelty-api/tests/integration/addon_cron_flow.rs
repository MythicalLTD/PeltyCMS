//! Addon cron jobs (ABI 0.5) with the real `fixture-cron` component: the
//! `cron` capability needs approval, jobs appear as `<addon>:<job>` tasks with
//! the manifest schedule, run through `on-cron`, and follow edits/disable.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

const ID: &str = "integration-cron";

async fn run(app: &axum::Router, st: &pelty_api::AppState, admin: &Sess, task: &str) -> Value {
    let r = call(app, "POST", &format!("/api/v1/admin/automation/{task}/run"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_i64().expect("enqueued");
    for _ in 0..20 {
        pelty_api::worker::run_once(st, "cron-test", 20).await.unwrap();
        let (status, result, err): (String, Option<Value>, Option<String>) = sqlx::query_as("SELECT status::text, result, last_error FROM jobs WHERE id = $1").bind(id).fetch_one(&st.pool).await.unwrap();
        if (status != "queued" && status != "running") || err.is_some() {
            return json!({ "status": status, "result": result, "error": err });
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("job {id} did not finish");
}

#[tokio::test]
async fn addon_cron_jobs() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(ID).execute(&st.pool).await.unwrap();
    sqlx::query("DELETE FROM jobs WHERE payload->>'addon' = $1").bind(ID).execute(&st.pool).await.unwrap();
    sqlx::query("DELETE FROM automation_settings WHERE kind LIKE 'integration-cron:%'").execute(&st.pool).await.unwrap();
    pelty_api::addons::accept_package(&st, &fixture_zip("fixture-cron", "pelty-addon-fixture-cron"), true).await.expect("fixture package");
    let base = format!("/api/v1/admin/addons/{ID}");
    let r = call(&app, "POST", &format!("{base}/install"), None, &admin.h()).await;
    assert_eq!((r.status, r.body["error"]["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("error.addon_capability_approval")), "cron needs approval: {:?}", r.body);
    let r = call(&app, "POST", &format!("{base}/install"), Some(json!({ "approve": ["cron"] })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", &format!("{base}/enable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);

    let tasks = || async {
        let l = call(&app, "GET", "/api/v1/admin/automation/tasks", None, &admin.h()).await;
        assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
        l.body["tasks"].as_array().unwrap().clone()
    };
    let t = tasks().await;
    let nightly = t.iter().find(|t| t["name"] == "integration-cron:nightly").cloned().expect("listed");
    assert_eq!(nightly["default_schedule"], "0 3 * * *");
    assert_eq!(nightly["schedule"], "0 3 * * *");
    assert_eq!(nightly["source"], ID);
    assert!(t.iter().any(|t| t["name"] == "integration-cron:broken"));

    let ok = run(&app, &st, &admin, "integration-cron:nightly").await;
    assert_eq!(ok["status"], "succeeded", "{ok:?}");
    assert_eq!(ok["result"]["cleaned"], 3, "{ok:?}");
    let bad = run(&app, &st, &admin, "integration-cron:broken").await;
    assert_ne!(bad["status"], "succeeded", "{bad:?}");
    assert!(bad["error"].as_str().is_some_and(|e| e.contains("broken on purpose")), "{bad:?}");

    // A run with no retries left feeds the task's breaker and the addon's call stats.
    pelty_database::jobs::enqueue(&st.pool, "addon.job", json!({ "addon": ID, "op": "broken", "cron_job": "broken", "cron": 0 }), None, None, 1).await.unwrap();
    pelty_api::worker::run_once(&st, "cron-test", 20).await.unwrap();
    let t = tasks().await.into_iter().find(|t| t["name"] == "integration-cron:broken").unwrap();
    assert!(t["failures"].as_i64().unwrap() >= 1, "{t:?}");
    assert!(st.addon_health.get(ID).by_kind.contains_key("cron"));
    let r = call(&app, "POST", "/api/v1/admin/automation/integration-cron:broken/reset", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Editable like any task; the edit wins over the manifest default.
    let r = call(&app, "PUT", "/api/v1/admin/automation/integration-cron:nightly/schedule", Some(json!({ "schedule": "15 4 * * 1" })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let t = tasks().await;
    let nightly = t.iter().find(|t| t["name"] == "integration-cron:nightly").cloned().unwrap();
    assert_eq!((nightly["schedule"].clone(), nightly["default_schedule"].clone()), (json!("15 4 * * 1"), json!("0 3 * * *")));

    // Runtime registration (power.cron-register).
    let reg = |body: Value| {
        let app = app.clone();
        let h = admin.h();
        async move {
            let r = call(&app, "POST", &format!("/api/v1/addons/{ID}/cron"), Some(body), &h).await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body
        }
    };
    assert_eq!(reg(json!({ "action": "register", "name": "dyn", "schedule": "*/5 * * * *" })).await, json!({ "ok": true }));
    for bad in [json!({ "action": "register", "name": "dyn", "schedule": "nope" }), json!({ "action": "register", "name": "nightly", "schedule": "* * * * *" })] {
        assert!(reg(bad.clone()).await["err"].is_string(), "{bad:?}");
    }
    let dynt = tasks().await.into_iter().find(|t| t["name"] == "integration-cron:dyn").expect("runtime job listed");
    assert_eq!(dynt["schedule"], "*/5 * * * *");
    let ran = run(&app, &st, &admin, "integration-cron:dyn").await;
    assert_eq!((ran["status"].clone(), ran["result"]["runtime"].clone()), (json!("succeeded"), json!(true)), "{ran:?}");
    assert_eq!(reg(json!({ "action": "unregister", "name": "dyn" })).await, json!({ "ok": true }));
    assert!(!tasks().await.iter().any(|t| t["name"] == "integration-cron:dyn"));
    reg(json!({ "action": "register", "name": "dyn", "schedule": "0 * * * *" })).await;

    // Disabled addon: its tasks are gone.
    assert_eq!(call(&app, "POST", &format!("{base}/disable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    assert!(!tasks().await.iter().any(|t| t["name"].as_str().is_some_and(|n| n.starts_with("integration-cron:"))));

    // Uninstall drops runtime jobs and edited schedules.
    assert_eq!(call(&app, "DELETE", &base, None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let n: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM addon_cron_jobs WHERE addon_id = $1) + (SELECT count(*) FROM automation_settings WHERE kind LIKE 'integration-cron:%')").bind(ID).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n, 0);
}
