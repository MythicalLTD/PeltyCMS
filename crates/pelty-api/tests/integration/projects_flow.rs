use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn projects_tasks_time_and_billing() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let (ah, ch) = (admin.h(), alice.h());
    let cid = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE"})),
        &ch,
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let _ = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE"})),
        &bob.h(),
    )
    .await;

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/projects",
            Some(json!({"title": ""})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/projects",
            Some(json!({"title": "X"})),
            &ch
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/projects",
        Some(json!({"title": "Website", "customer_id": cid, "hourly_rate": "50.00"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pid = r.body["id"].as_str().unwrap().to_string();
    let base = format!("/api/v1/admin/projects/{pid}");

    let task = call(
        &app,
        "POST",
        &format!("{base}/tasks"),
        Some(json!({"title": "Design"})),
        &ah,
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("{base}/tasks/{task}"),
            Some(json!({"done": true})),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/time"),
            Some(json!({"minutes": 0})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/time"),
            Some(json!({"minutes": 10, "task_id": Uuid::new_v4()})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/time"),
            Some(json!({"minutes": 90, "task_id": task, "note": "Mockups"})),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/time"),
            Some(json!({"minutes": 7})),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/time"),
            Some(json!({"minutes": 30, "billable": false})),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );

    let d = call(&app, "GET", &base, None, &ah).await.body;
    assert_eq!(d["project"]["tasks_done"], 1);
    assert_eq!(d["project"]["minutes_total"], 127);
    assert_eq!(d["time"].as_array().unwrap().len(), 3);

    // Invisible to the client until shared; never to another client.
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/projects/{pid}"),
            None,
            &ch
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(call(&app, "PUT", &base, Some(json!({"title": "Website", "customer_id": cid, "hourly_rate": "50.00", "client_visible": true})), &ah).await.status, StatusCode::OK);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/projects/{pid}"),
            None,
            &ch
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/projects", None, &ch)
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/projects/{pid}"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Invoice billable time: 90 min = 75.00, 7 min = 5.83.
    let r = call(&app, "POST", &format!("{base}/invoice-time"), None, &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = r.body["invoice_id"].as_str().unwrap().to_string();
    let sub: rust_decimal::Decimal =
        sqlx::query_scalar("SELECT sum(amount) FROM invoice_items WHERE invoice_id = $1::uuid")
            .bind(&inv)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(sub, "80.83".parse().unwrap());
    let r = call(&app, "POST", &format!("{base}/invoice-time"), None, &ah).await;
    assert_eq!(
        (r.status, r.body["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("error.project_nothing_to_bill"))
    );
    let d = call(&app, "GET", &base, None, &ah).await.body;
    assert!(d["links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "invoice" && l["target_id"] == inv.as_str()));
    // Invoiced time is locked.
    let billed = d["time"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["invoice_id"] == inv.as_str())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        call(&app, "DELETE", &format!("{base}/time/{billed}"), None, &ah)
            .await
            .status,
        StatusCode::CONFLICT
    );

    // Links must belong to the project's client.
    let r = call(
        &app,
        "POST",
        &format!("{base}/links"),
        Some(json!({"kind": "invoice", "target_id": Uuid::new_v4()})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        call(&app, "DELETE", &base, None, &ah).await.status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", &base, None, &ah).await.status,
        StatusCode::NOT_FOUND
    );
}
