use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

async fn cid(app: &axum::Router, s: &Sess) -> String {
    call(app, "GET", "/api/v1/client/profile", None, &s.h())
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn mass_actions_per_row_results() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();
    let a = sess(&app, &st, "customer").await;
    let b = sess(&app, &st, "customer").await;
    let (ia, ib) = (cid(&app, &a).await, cid(&app, &b).await);
    let ghost = uuid::Uuid::new_v4().to_string();

    // Customers can't use it.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/customers",
        Some(json!({"ids": [ia], "action": "set_status", "value": "inactive"})),
        &a.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // Validation.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/customers",
        Some(json!({"ids": [], "action": "set_status", "value": "inactive"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.mass_ids");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/customers",
        Some(json!({"ids": [ia], "action": "explode"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_action");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/customers",
        Some(json!({"ids": [ia], "action": "set_status", "value": "zombie"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_status");

    // Status: two real + one ghost → 2 ok, 1 not_found.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/customers",
        Some(json!({"ids": [ia, ib, ghost], "action": "set_status", "value": "inactive"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["ok"], 2);
    assert_eq!(r.body["failed"], 1);
    let bad: Vec<_> = r.body["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["ok"] == false)
        .collect();
    assert_eq!(bad[0]["id"], ghost.as_str());
    assert_eq!(bad[0]["error"], "error.not_found");
    let c = call(
        &app,
        "GET",
        &format!("/api/v1/admin/customers/{ia}"),
        None,
        &ah,
    )
    .await;
    assert_eq!(c.body["customer"]["status"], "inactive", "{:?}", c.body);
    // Restore.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/customers",
        Some(json!({"ids": [ia, ib], "action": "set_status", "value": "active"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 2);

    // Tickets: open two, mass set priority + status, bad status fails per row.
    let mut tids = vec![];
    for s in [&a, &b] {
        let r = call(
            &app,
            "POST",
            "/api/v1/client/tickets",
            Some(json!({"department_id": SUPPORT, "subject": "Mass", "body": "x"})),
            &s.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        tids.push(r.body["ticket"]["id"].as_str().unwrap().to_string());
    }
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/tickets",
        Some(json!({"ids": tids, "action": "set_priority", "value": "high"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 2, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/tickets",
        Some(json!({"ids": tids, "action": "set_status", "value": "nope"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 0);
    assert_eq!(r.body["failed"], 2);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/tickets",
        Some(json!({"ids": tids, "action": "set_status", "value": "closed"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["ok"], 2, "{:?}", r.body);
    let t = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{}", tids[0]),
        None,
        &ah,
    )
    .await;
    assert_eq!(t.body["ticket"]["status"], "closed");
    assert_eq!(t.body["ticket"]["priority"], "high");

    // Services / domains: unknown ids are reported, not fatal.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/services",
        Some(json!({"ids": [ghost], "action": "suspend", "value": "abuse"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["failed"], 1);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/services",
        Some(json!({"ids": [ghost], "action": "delete"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_action");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/mass/domains",
        Some(json!({"ids": [ghost], "action": "auto_renew_off"})),
        &ah,
    )
    .await;
    assert_eq!(r.body["results"][0]["error"], "error.not_found");
}
