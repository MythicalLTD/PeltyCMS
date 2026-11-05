use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn uid(app: &axum::Router, s: &Sess) -> String {
    call(app, "GET", "/api/v1/auth/me", None, &s.h()).await.body["id"].as_str().map(str::to_string).unwrap_or_default()
}

#[tokio::test]
async fn roles_staff_audit_and_2fa_policy() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let ah = admin.h();
    let off = || call(&app, "PUT", "/api/v1/admin/security/staff-2fa", Some(json!({"required": false})), &ah);
    off().await;

    // Staff lacks users.manage.
    assert_eq!(call(&app, "GET", "/api/v1/admin/roles", None, &staff.h()).await.status, StatusCode::FORBIDDEN);
    let l = call(&app, "GET", "/api/v1/admin/roles", None, &ah).await;
    assert_eq!(l.status, StatusCode::OK);
    assert!(l.body["permissions"].as_array().unwrap().iter().any(|p| p == "invoices.manage"));
    let admin_role = l.body["items"].as_array().unwrap().iter().find(|r| r["name"] == "admin").unwrap()["id"].as_str().unwrap().to_string();

    // Create a custom role; validation and duplicates.
    let name = format!("billing_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    assert_eq!(call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": "Bad Name"})), &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": name, "permissions": ["nope.x"]})), &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": name, "permissions": ["admin.access", "settings.manage"]})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": name})), &ah).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/roles/{admin_role}"), Some(json!({"name": "admin", "permissions": []})), &ah).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/roles/{admin_role}"), None, &ah).await.status, StatusCode::CONFLICT);

    // Assign the role to the staff user: they gain invoices access live.
    let sid = uid(&app, &staff).await;
    assert!(!sid.is_empty());
    assert_eq!(call(&app, "GET", "/api/v1/admin/currencies", None, &staff.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/users/{sid}/roles"), Some(json!({"roles": [name.clone()]})), &ah).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/admin/currencies", None, &staff.h()).await.status, StatusCode::OK);
    let sl = call(&app, "GET", "/api/v1/admin/staff", None, &ah).await;
    let me = sl.body["items"].as_array().unwrap().iter().find(|m| m["id"] == sid.as_str()).unwrap().clone();
    assert_eq!(me["roles"], json!([name.clone()]));
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/roles/{rid}"), None, &ah).await.status, StatusCode::CONFLICT);

    // Self-demotion guard.
    let aid = uid(&app, &admin).await;
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/users/{aid}/roles"), Some(json!({"roles": ["staff"]})), &ah).await.status, StatusCode::CONFLICT);

    // Audit log filters.
    let a = call(&app, "GET", &format!("/api/v1/admin/audit-logs?action=user.roles&target={sid}"), None, &ah).await;
    assert_eq!(a.status, StatusCode::OK);
    assert_eq!(a.body["total"], 1);
    assert_eq!(a.body["items"][0]["actor_email"], admin.email.as_str());

    // 2FA policy: admin must have 2FA to enable; then staff without 2FA is locked out of admin APIs.
    assert_eq!(call(&app, "PUT", "/api/v1/admin/security/staff-2fa", Some(json!({"required": true})), &ah).await.status, StatusCode::CONFLICT);
    sqlx::query("INSERT INTO user_mfa (user_id, secret_enc, enabled_at) VALUES ($1::uuid, '\\x00', now())").bind(&aid).execute(&st.pool).await.unwrap();
    assert_eq!(call(&app, "PUT", "/api/v1/admin/security/staff-2fa", Some(json!({"required": true})), &ah).await.status, StatusCode::OK);
    let blocked = call(&app, "GET", "/api/v1/admin/currencies", None, &staff.h()).await;
    let g = call(&app, "GET", "/api/v1/admin/security/staff-2fa", None, &ah).await;
    off().await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    assert_eq!(blocked.body["error"]["code"], "error.mfa_required");
    assert_eq!(g.body["required"], true);
    assert_eq!(call(&app, "GET", "/api/v1/admin/currencies", None, &staff.h()).await.status, StatusCode::OK);

    // Cleanup: role can be deleted once unused.
    call(&app, "PUT", &format!("/api/v1/admin/users/{sid}/roles"), Some(json!({"roles": ["staff"]})), &ah).await;
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/roles/{rid}"), None, &ah).await.status, StatusCode::OK);
}

#[tokio::test]
async fn admin_marks_email_verified() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let me = call(&app, "GET", "/api/v1/auth/me", None, &cust.h()).await;
    assert_eq!(me.body["email_verified"], json!(false), "{:?}", me.body);
    let id = me.body["id"].as_str().unwrap().to_string();
    let url = format!("/api/v1/admin/users/{id}/verify-email");
    assert_eq!(call(&app, "POST", &url, None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "POST", &url, None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let me = call(&app, "GET", "/api/v1/auth/me", None, &cust.h()).await;
    assert_eq!(me.body["email_verified"], json!(true));
    let missing = format!("/api/v1/admin/users/{}/verify-email", uuid::Uuid::new_v4());
    assert_eq!(call(&app, "POST", &missing, None, &admin.h()).await.status, StatusCode::NOT_FOUND);
}
