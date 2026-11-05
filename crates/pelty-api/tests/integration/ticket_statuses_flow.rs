use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn custom_ticket_statuses() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;

    let r = call(&app, "GET", "/api/v1/ticket-statuses", None, &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["slug"] == "closed" && x["builtin"] == true));

    let slug = format!("vendor_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let body = json!({"slug": slug, "name": "Waiting on vendor", "color": "destructive", "sort_order": 50});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/ticket-statuses",
            Some(body.clone()),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-statuses",
        Some(body.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/ticket-statuses",
            Some(body),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/ticket-statuses",
        Some(json!({"slug": "Bad-Slug", "name": "x"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_ticket_status");

    // Use it on a ticket.
    let r = call(
        &app,
        "GET",
        "/api/v1/client/ticket-departments",
        None,
        &cust.h(),
    )
    .await;
    let dep = r.body["items"][0]["id"].as_str().map(str::to_string);
    let dep = match dep {
        Some(d) => d,
        None => call(
            &app,
            "POST",
            "/api/v1/admin/ticket-departments",
            Some(json!({"name": format!("D {slug}")})),
            &admin.h(),
        )
        .await
        .body["id"]
            .as_str()
            .unwrap()
            .to_string(),
    };
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(json!({"department_id": dep, "subject": "s", "body": "b"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["ticket"]["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{tid}"),
        Some(json!({"status": "nope_status"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{tid}"),
        Some(json!({"status": slug})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets?status={slug}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["items"].as_array().unwrap().len(), 1);

    // Rename; in-use and built-in statuses can't be deleted.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/ticket-statuses/{slug}"),
        Some(json!({"name": "Vendor", "color": "outline"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/ticket-statuses/{slug}"),
            None,
            &admin.h()
        )
        .await
        .body["error"]["code"],
        "error.ticket_status_in_use"
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/ticket-statuses/open",
            None,
            &admin.h()
        )
        .await
        .body["error"]["code"],
        "error.ticket_status_builtin"
    );
    call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/tickets/{tid}"),
        Some(json!({"status": "closed"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/ticket-statuses/{slug}"),
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
            &format!("/api/v1/admin/ticket-statuses/{slug}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
