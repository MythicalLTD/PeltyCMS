use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn client_groups_crud_membership_and_rbac() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff = sess(&app, &state, "staff").await;
    let cust = sess(&app, &state, "customer").await;
    // Customer record is created lazily on first profile read.
    assert_eq!(call(&app, "GET", "/api/v1/client/profile", None, &cust.h()).await.status, StatusCode::OK);
    let name = format!("VIP {}", &uuid::Uuid::new_v4().simple().to_string()[..8]);

    assert_eq!(call(&app, "GET", "/api/v1/admin/client-groups", None, &staff.h()).await.status, StatusCode::FORBIDDEN);
    for (body, code) in [
        (json!({"name": name, "color": "neon"}), "error.invalid_color"),
        (json!({"name": name, "discount_percent": "100.01"}), "error.invalid_discount"),
        (json!({"name": name, "discount_percent": "1.234"}), "error.invalid_discount"),
        (json!({"name": " "}), "error.invalid_name"),
    ] {
        let r = call(&app, "POST", "/api/v1/admin/client-groups", Some(body), &admin.h()).await;
        assert_eq!(r.body["error"]["code"], code);
    }
    let r = call(&app, "POST", "/api/v1/admin/client-groups", Some(json!({"name": name, "color": "violet", "discount_percent": "12.50"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["group"]["discount_percent"], "12.50");
    let gid = r.body["group"]["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "POST", "/api/v1/admin/client-groups", Some(json!({"name": name})), &admin.h()).await.status, StatusCode::CONFLICT);

    let r = call(&app, "POST", &format!("/api/v1/admin/client-groups/{gid}/members"), Some(json!({"email": "nobody@example.invalid"})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.unknown_customer");
    let r = call(&app, "POST", &format!("/api/v1/admin/client-groups/{gid}/members"), Some(json!({"email": cust.email.to_uppercase()})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let m = call(&app, "GET", &format!("/api/v1/admin/client-groups/{gid}/members"), None, &admin.h()).await;
    let cid = m.body["items"][0]["id"].as_str().unwrap().to_string();
    let l = call(&app, "GET", "/api/v1/admin/client-groups", None, &admin.h()).await;
    assert_eq!(l.body["items"].as_array().unwrap().iter().find(|g| g["id"] == gid.as_str()).unwrap()["member_count"], 1);

    let r = call(&app, "POST", &format!("/api/v1/admin/customers/{cid}/group"), Some(json!({"group_id": uuid::Uuid::new_v4()})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.invalid_group");
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/client-groups/{gid}/members/{cid}"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/client-groups/{gid}/members/{cid}"), None, &admin.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/customers/{cid}/group"), Some(json!({"group_id": gid})), &admin.h()).await.status, StatusCode::NO_CONTENT);
    // Deleting the group detaches members.
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/client-groups/{gid}"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'customer.group_changed' AND target = $1")
        .bind(&cid).fetch_one(&state.pool).await.unwrap();
    assert_eq!(n, 3);
}
