//! Automatic client-group rules: validation, dry run, apply, priority, overwrite.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn client_group_rules() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let tag = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
    let dom = format!("{tag}.example");
    let grp = |n: &str| json!({"name": format!("{n} {tag}")});
    let g1 = call(
        &app,
        "POST",
        "/api/v1/admin/client-groups",
        Some(grp("Edu")),
        &admin.h(),
    )
    .await
    .body["group"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let g2 = call(
        &app,
        "POST",
        "/api/v1/admin/client-groups",
        Some(grp("Other")),
        &admin.h(),
    )
    .await
    .body["group"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mk = |local: &str| json!({"email": format!("{local}@{dom}"), "first_name": "R", "last_name": "G", "currency": "EUR"});
    let a = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(mk("a")),
        &admin.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(mk("b")),
        &admin.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/client-group-rules",
            None,
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    for (body, code) in [
        (
            json!({"group_id": g1, "kind": "bogus", "value": "x"}),
            "error.invalid_rule_kind",
        ),
        (
            json!({"group_id": g1, "kind": "country", "value": "ROU"}),
            "error.invalid_rule_value",
        ),
        (
            json!({"group_id": g1, "kind": "active_services_min", "value": "-2"}),
            "error.invalid_rule_value",
        ),
        (
            json!({"group_id": uuid::Uuid::new_v4(), "kind": "country", "value": "RO"}),
            "error.invalid_group",
        ),
        (
            json!({"group_id": g1, "kind": "country", "value": "RO", "priority": -1}),
            "error.invalid_priority",
        ),
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/client-group-rules",
            Some(body),
            &admin.h(),
        )
        .await;
        assert_eq!(r.body["error"]["code"], code);
    }
    // Lower priority number wins: g1 (domain, 10) beats g2 (domain, 20).
    let r = call(&app, "POST", "/api/v1/admin/client-group-rules", Some(json!({"group_id": g2, "kind": "email_domain", "value": format!("@{}", dom.to_uppercase()), "priority": 20})), &admin.h()).await;
    assert_eq!(r.body["rule"]["value"], dom);
    let r2 = r.body["rule"]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/client-group-rules",
        Some(json!({"group_id": g1, "kind": "email_domain", "value": dom, "priority": 10})),
        &admin.h(),
    )
    .await;
    let r1 = r.body["rule"]["id"].as_str().unwrap().to_string();

    let gid = |c: String| {
        let pool = state.pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<uuid::Uuid>>(
                "SELECT client_group_id FROM customers WHERE id = $1::uuid",
            )
            .bind(c)
            .fetch_one(&pool)
            .await
            .unwrap()
            .map(|u| u.to_string())
        }
    };
    let ours = |v: &serde_json::Value| {
        v["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["customer_id"] == a || c["customer_id"] == b)
            .cloned()
            .collect::<Vec<_>>()
    };
    let dry = call(
        &app,
        "POST",
        "/api/v1/admin/client-group-rules/apply",
        Some(json!({})),
        &admin.h(),
    )
    .await;
    assert_eq!(dry.body["dry_run"], true);
    let ch = ours(&dry.body);
    assert_eq!(ch.len(), 2);
    assert!(ch.iter().all(|c| c["to"] == g1 && c["rule_id"] == r1));
    assert_eq!(gid(a.clone()).await, None, "dry run changes nothing");

    call(
        &app,
        "POST",
        "/api/v1/admin/client-group-rules/apply",
        Some(json!({"dry_run": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(gid(a.clone()).await.as_deref(), Some(g1.as_str()));
    assert_eq!(gid(b.clone()).await.as_deref(), Some(g1.as_str()));
    // Idempotent.
    assert!(ours(
        &call(
            &app,
            "POST",
            "/api/v1/admin/client-group-rules/apply",
            Some(json!({})),
            &admin.h()
        )
        .await
        .body
    )
    .is_empty());

    // Disable r1; without overwrite already-grouped customers stay, with overwrite they move to g2.
    call(&app, "PUT", &format!("/api/v1/admin/client-group-rules/{r1}"), Some(json!({"group_id": g1, "kind": "email_domain", "value": dom, "priority": 10, "enabled": false})), &admin.h()).await;
    assert!(ours(
        &call(
            &app,
            "POST",
            "/api/v1/admin/client-group-rules/apply",
            Some(json!({"dry_run": false})),
            &admin.h()
        )
        .await
        .body
    )
    .is_empty());
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/client-group-rules/apply",
        Some(json!({"dry_run": false, "overwrite": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(ours(&r.body).len(), 2);
    assert_eq!(gid(a.clone()).await.as_deref(), Some(g2.as_str()));

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/client-group-rules/{r2}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/client-group-rules/{r2}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/client-group-rules/{r1}"),
        None,
        &admin.h(),
    )
    .await;
}
