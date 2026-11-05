use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn staff_noticeboard_post_list_delete() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let cust = sess(&app, &st, "customer").await;

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/noticeboard", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/noticeboard",
            Some(json!({"body": "x"})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/noticeboard",
            Some(json!({"body": "   "})),
            &staff.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/noticeboard",
            Some(json!({"body": "x", "extra": 1})),
            &staff.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/noticeboard",
        Some(json!({"body": "Staff note"})),
        &staff.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let staff_id = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/noticeboard",
        Some(json!({"body": "Pinned by admin", "pinned": true})),
        &admin.h(),
    )
    .await;
    let admin_id = r.body["id"].as_str().unwrap().to_string();

    let l = call(&app, "GET", "/api/v1/admin/noticeboard", None, &staff.h()).await;
    assert_eq!(l.status, StatusCode::OK);
    let items = l.body["items"].as_array().unwrap();
    let pos = |id: &str| items.iter().position(|n| n["id"] == id).unwrap();
    assert!(pos(&admin_id) < pos(&staff_id), "pinned first");
    assert_eq!(items[pos(&staff_id)]["body"], "Staff note");

    // Staff cannot delete someone else's notice; admin (settings.manage) can delete any.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/noticeboard/{admin_id}"),
            None,
            &staff.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/noticeboard/{staff_id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/noticeboard/{admin_id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let l = call(&app, "GET", "/api/v1/admin/noticeboard", None, &admin.h()).await;
    assert!(l.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|n| n["id"] != staff_id && n["id"] != admin_id));
}
