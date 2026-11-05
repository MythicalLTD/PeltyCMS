use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

/// Per-resource read/write: a custom role with `customers.read` can list
/// but not create customers; `.manage` implies `.read`; API keys can be
/// scoped to a read level.
#[tokio::test]
async fn read_levels_for_roles_and_keys() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let ah = admin.h();

    let l = call(&app, "GET", "/api/v1/admin/roles", None, &ah).await;
    let perms = l.body["permissions"].as_array().unwrap();
    for p in ["customers.read", "invoices.read", "tickets.read", "orders.read", "services.read", "catalog.read"] {
        assert!(perms.iter().any(|x| x == p), "{p} missing");
    }

    let name = format!("ro_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let r = call(&app, "POST", "/api/v1/admin/roles", Some(json!({"name": name, "permissions": ["admin.access", "customers.read"]})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let ro = sess(&app, &state, &name).await;
    assert_eq!(call(&app, "GET", "/api/v1/admin/customers", None, &ro.h()).await.status, StatusCode::OK);
    let c = json!({"email": email(), "first_name": "Ro", "last_name": "Le", "currency": "EUR"});
    assert_eq!(call(&app, "POST", "/api/v1/admin/customers", Some(c.clone()), &ro.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/invoices", None, &ro.h()).await.status, StatusCode::FORBIDDEN);

    // Admin session: `.manage` implies `.read`, visible in /auth/me.
    let me = call(&app, "GET", "/api/v1/auth/me", None, &ah).await;
    assert!(me.body.to_string().contains("customers.read"), "{:?}", me.body);

    // Read-only API key.
    let r = call(&app, "POST", "/api/v1/api-keys", Some(json!({"name": "reader", "scopes": ["admin.access", "customers.read"]})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let bearer = format!("Bearer {}", r.body["secret"].as_str().unwrap());
    let h = [("authorization", bearer.as_str())];
    assert_eq!(call(&app, "GET", "/api/v1/admin/customers", None, &h).await.status, StatusCode::OK);
    assert_eq!(call(&app, "POST", "/api/v1/admin/customers", Some(c.clone()), &h).await.status, StatusCode::FORBIDDEN);

    // A write key still reads.
    let r = call(&app, "POST", "/api/v1/api-keys", Some(json!({"name": "writer", "scopes": ["admin.access", "customers.manage"]})), &ah).await;
    let bearer = format!("Bearer {}", r.body["secret"].as_str().unwrap());
    let h = [("authorization", bearer.as_str())];
    assert_eq!(call(&app, "GET", "/api/v1/admin/customers", None, &h).await.status, StatusCode::OK);
    let r = call(&app, "POST", "/api/v1/admin/customers", Some(c), &h).await;
    assert!(r.status.is_success(), "{:?}", r.body);
}
