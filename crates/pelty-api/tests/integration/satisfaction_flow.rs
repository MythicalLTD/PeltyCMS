use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn satisfaction_report() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let r = call(&app, "POST", "/api/v1/admin/ticket-departments", Some(json!({"name": format!("CSAT {}", uuid::Uuid::new_v4().simple())})), &admin.h()).await;
    let dep = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "GET", "/api/v1/admin/ticket-satisfaction", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let empty = call(&app, "GET", &format!("/api/v1/admin/ticket-satisfaction?department_id={dep}"), None, &admin.h()).await;
    assert_eq!(empty.body["report"]["summary"]["ratings"], 0);
    assert!(empty.body["report"]["summary"]["avg_rating"].is_null());

    for (stars, comment) in [(5, "great"), (4, ""), (1, "slow"), (0, "")] {
        let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": dep, "subject": "s", "body": "b"})), &cust.h()).await;
        let tid = r.body["ticket"]["id"].as_str().unwrap().to_string();
        call(&app, "POST", &format!("/api/v1/admin/tickets/{tid}/reply"), Some(json!({"body": "hi"})), &admin.h()).await;
        if stars > 0 {
            let r = call(&app, "POST", &format!("/api/v1/client/tickets/{tid}/rate"), Some(json!({"rating": stars, "comment": comment})), &cust.h()).await;
            assert_eq!(r.status, StatusCode::OK);
        }
    }
    let r = call(&app, "GET", &format!("/api/v1/admin/ticket-satisfaction?department_id={dep}&days=7"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let s = &r.body["report"]["summary"];
    assert_eq!(s["ratings"], 3);
    assert_eq!(s["avg_rating"], "3.33");
    assert_eq!(s["csat"], "66.67");
    assert_eq!(s["rated_tickets_share"], "75.00");
    assert_eq!((s["r1"].as_i64(), s["r4"].as_i64(), s["r5"].as_i64(), s["r3"].as_i64()), (Some(1), Some(1), Some(1), Some(0)));
    assert_eq!(r.body["report"]["by_department"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["report"]["by_staff"][0]["ratings"], 3);
    let comments = r.body["report"]["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2, "empty comments are skipped");
    let r = call(&app, "GET", &format!("/api/v1/admin/ticket-satisfaction?department_id={dep}&low=true"), None, &admin.h()).await;
    let low = r.body["report"]["comments"].as_array().unwrap();
    assert_eq!(low.len(), 1);
    assert_eq!(low[0]["comment"], "slow");
}
