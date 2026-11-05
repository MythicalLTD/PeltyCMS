use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const ADMIN: &str = "admin";
const CUSTOMER: &str = "customer";

#[tokio::test]
async fn customer_profile_is_own_and_hides_notes() {
    let (app, st) = app().await;
    let (ck, csrf, email) = login_new(&app, &st, CUSTOMER).await;
    let h = [("cookie", ck.as_str()), ("x-csrf-token", csrf.as_str())];
    let r = call(&app, "GET", "/api/v1/client/profile", None, &h).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["email"], email);
    assert!(r.body.get("notes").is_none(), "staff notes must not leak");
    let cid = r.body["id"].as_str().unwrap().to_string();

    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"organization": "Acme", "country": "ro", "city": "Cluj"})),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["country"], "RO");
    assert_eq!(r.body["id"], cid.as_str());
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "Romania"})),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"currency": "XXX"})),
        &h,
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown currency rejected by FK"
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"status": "active"})),
        &h,
    )
    .await;
    assert!(r.status.is_client_error(), "customer cannot set status");

    // Customer is not staff.
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/customers", None, &h)
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Contacts are scoped: another customer cannot delete mine.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/contacts",
        Some(json!({"name": "Bob", "email": "bob@x.test", "is_billing": true})),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let contact = r.body["id"].as_str().unwrap().to_string();
    let (ck2, csrf2, _) = login_new(&app, &st, CUSTOMER).await;
    let h2 = [("cookie", ck2.as_str()), ("x-csrf-token", csrf2.as_str())];
    let r = call(
        &app,
        "DELETE",
        &format!("/api/v1/client/contacts/{contact}"),
        None,
        &h2,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(
        call(&app, "GET", "/api/v1/client/contacts", None, &h2)
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // Admin sees notes, can set status, list/search works.
    let (ack, acsrf, _) = login_new(&app, &st, ADMIN).await;
    let ah = [("cookie", ack.as_str()), ("x-csrf-token", acsrf.as_str())];
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/customers/{cid}"),
        Some(json!({"notes": "vip", "status": "inactive"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["notes"], "vip");
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/customers?q={email}"),
        None,
        &ah,
    )
    .await;
    assert_eq!(r.body["total"], 1, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/customers/{cid}"),
        None,
        &ah,
    )
    .await;
    assert_eq!(r.body["contacts"].as_array().unwrap().len(), 1);
    assert!(call(&app, "GET", "/api/v1/client/profile", None, &h)
        .await
        .body
        .get("notes")
        .is_none());

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(json!({"email": "offline@x.test", "organization": "Offline SRL"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["owner_user_id"].is_null());
}
