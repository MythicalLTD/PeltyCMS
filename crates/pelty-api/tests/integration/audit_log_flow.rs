//! Admin audit log: every filter (action prefix, actor e-mail / id, target,
//! IP prefix, free text, date range, system-only), validation, the action
//! picker, CSV export (audited, formula-guarded) and permission checks.
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn ids(v: &Value) -> Vec<i64> {
    v["items"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn audit_log_filters_and_export() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&staff.email).fetch_one(&st.pool).await.unwrap();
    let tag = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
    let p = format!("zz{tag}");
    let rec = |action: String, actor: Option<uuid::Uuid>, target: String, ip: &'static str, meta: Value| {
        let pool = st.pool.clone();
        async move { pelty_database::audit::record(&pool, actor, &action, Some(&target), Some(ip), meta).await.unwrap() }
    };
    rec(format!("{p}.alpha"), Some(uid), format!("t-{tag}-1"), "10.9.1.1", json!({"note": format!("needle-{tag}")})).await;
    rec(format!("{p}.beta"), None, format!("t-{tag}-2"), "10.9.2.2", json!({})).await;
    rec(format!("{p}_x.gamma"), Some(uid), format!("t-{tag}-3"), "192.0.2.7", json!({"formula": "=HYPERLINK(1)"})).await;
    let old_id: i64 = sqlx::query_scalar("SELECT id FROM audit_logs WHERE action = $1").bind(format!("{p}.beta")).fetch_one(&st.pool).await.unwrap();

    let get = |q: String| {
        let (app, h) = (app.clone(), admin.h());
        async move { call(&app, "GET", &format!("/api/v1/admin/audit-logs?{q}"), None, &h).await }
    };
    // Action prefix: `_` is literal, so `{p}.` doesn't match `{p}_x.`.
    let r = get(format!("action={p}.")).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!((r.body["total"].as_i64(), ids(&r.body).len()), (Some(2), 2));
    assert_eq!(get(format!("action={p}")).await.body["total"], 3);
    // Actor e-mail substring and actor id.
    assert_eq!(get(format!("action={p}&actor={}", staff.email)).await.body["total"], 2);
    assert_eq!(get(format!("action={p}&actor_id={uid}")).await.body["total"], 2);
    // System only (no actor).
    let sys = get(format!("action={p}&system=true")).await.body;
    assert_eq!(ids(&sys), vec![old_id]);
    assert!(sys["items"][0]["actor_id"].is_null());
    // Target exact, IP prefix, free text over metadata.
    assert_eq!(get(format!("target=t-{tag}-3")).await.body["items"][0]["action"], format!("{p}_x.gamma"));
    assert_eq!(get(format!("action={p}&ip=10.9.")).await.body["total"], 2);
    assert_eq!(get(format!("action={p}&ip=10.9.2.2")).await.body["total"], 1);
    let ft = get(format!("q=needle-{tag}")).await.body;
    assert_eq!((ft["total"].as_i64(), ft["items"][0]["action"].as_str()), (Some(1), Some(format!("{p}.alpha").as_str())));
    // `%` in the free text is literal, not a wildcard.
    assert_eq!(get(format!("action={p}&q=%25")).await.body["total"], 0);

    // Date range: whole days, inclusive `to`; RFC 3339 also accepted.
    // (audit_logs is append-only, so ranges are checked around "now".)
    let today = time::OffsetDateTime::now_utc().date();
    let yesterday = today.previous_day().unwrap();
    assert_eq!(get(format!("action={p}&from={today}&to={today}")).await.body["total"], 3);
    assert_eq!(get(format!("action={p}&to={yesterday}")).await.body["total"], 0);
    assert_eq!(get(format!("action={p}&from={}", today.next_day().unwrap())).await.body["total"], 0);
    let r = get(format!("action={p}&from={yesterday}T00:00:00Z")).await;
    assert_eq!(r.body["total"], 3, "{:?}", r.body);
    // Invalid / inverted ranges.
    assert_eq!(get("from=yesterday".into()).await.body["error"]["code"], "error.invalid_range");
    assert_eq!(get(format!("from={today}&to={yesterday}")).await.body["error"]["code"], "error.invalid_range");

    // Action picker.
    let a = call(&app, "GET", "/api/v1/admin/audit-logs/actions", None, &admin.h()).await.body;
    assert!(a["items"].as_array().unwrap().iter().any(|i| i["action"] == format!("{p}.alpha") && i["count"] == 1), "{a:?}");

    // CSV export: same filters, BOM, formula guard; the export is audited.
    let req = Request::builder().uri(format!("/api/v1/admin/audit-logs/export.csv?action={p}")).header("cookie", &admin.cookie).body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers()["content-type"].to_str().unwrap().starts_with("text/csv"));
    assert_eq!(res.headers()["x-total-count"], "3");
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    let lines: Vec<&str> = body.trim_end().split("\r\n").collect();
    assert!(lines[0].starts_with("\u{feff}id,created_at,action,actor_id,actor_email,target,ip,metadata"), "{}", lines[0]);
    assert_eq!(lines.len(), 4, "{body}");
    assert!(body.contains(&staff.email));
    assert!(body.contains("HYPERLINK") && !body.contains(",=HYPERLINK"), "never a raw leading =");
    let ex: Value = sqlx::query_scalar("SELECT metadata FROM audit_logs WHERE action = 'audit.exported' AND metadata->'query'->>'action' = $1").bind(&p).fetch_one(&st.pool).await.unwrap();
    assert_eq!(ex["rows"], 3);

    // Permissions: customers and roles without audit.read are refused.
    assert_eq!(call(&app, "GET", "/api/v1/admin/audit-logs", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/audit-logs/actions", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let role = format!("noaudit_{tag}");
    let r = call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": role, "permissions": ["admin.access", "tickets.read"]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let limited = sess(&app, &st, &role).await;
    assert_eq!(call(&app, "GET", "/api/v1/admin/audit-logs", None, &limited.h()).await.status, StatusCode::FORBIDDEN);
    let req = Request::builder().uri("/api/v1/admin/audit-logs/export.csv").header("cookie", &limited.cookie).body(Body::empty()).unwrap();
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sensitive_admin_actions_are_audited() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&admin.email).fetch_one(&st.pool).await.unwrap();
    let count = |action: &'static str| {
        let pool = st.pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_logs WHERE actor_id = $1 AND action = $2").bind(uid).bind(action).fetch_one(&pool).await.unwrap() }
    };
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let r = call(&app, "POST", "/api/v1/admin/support/spam-rules", Some(json!({"kind": "domain", "value": format!("{tag}.example")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(count("support.spam_rule_added").await, 1);
    let id = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/support/spam-rules/{id}"), None, &admin.h()).await.status, StatusCode::OK);
    assert_eq!(count("support.spam_rule_deleted").await, 1);
}
