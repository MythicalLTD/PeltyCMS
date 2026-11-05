use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn cid(app: &axum::Router, s: &Sess) -> String {
    call(app, "GET", "/api/v1/client/profile", None, &s.h())
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Mail merge to an explicit client selection, scheduled for later.
#[tokio::test]
async fn scheduled_mail_merge_to_selected_clients() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();
    let a = sess(&app, &st, "customer").await;
    let b = sess(&app, &st, "customer").await;
    let other = sess(&app, &st, "customer").await;
    let (ia, ib) = (cid(&app, &a).await, cid(&app, &b).await);
    let seg = json!({"customer_ids": [ia, ib]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/campaigns/preview",
            Some(seg.clone()),
            &ah
        )
        .await
        .body["recipients"],
        2
    );
    let r = call(&app, "POST", "/api/v1/admin/campaigns", Some(json!({"name": "Merge", "subject": "Hi {{first_name}}", "body": "Dear {{name}} <{{email}}> of {{company_name}}", "segment": seg})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    let sch = format!("/api/v1/admin/campaigns/{id}/schedule");

    // Past times are rejected; future ones put the campaign on hold.
    assert_eq!(
        call(
            &app,
            "POST",
            &sch,
            Some(json!({"send_at": "2001-01-01T00:00:00Z"})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let soon = (time::OffsetDateTime::now_utc() + time::Duration::seconds(2))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let r = call(&app, "POST", &sch, Some(json!({"send_at": soon})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = call(
        &app,
        "GET",
        &format!("/api/v1/admin/campaigns/{id}"),
        None,
        &ah,
    )
    .await;
    assert_eq!(d.body["campaign"]["status"], "scheduled");
    assert_eq!(
        d.body["campaign"]["total"], 0,
        "recipients are snapshotted only when due"
    );
    // Editing a scheduled campaign is blocked until unscheduled.
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/campaigns/{id}"),
            Some(json!({"name": "x", "subject": "y", "body": "z"})),
            &ah
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    let mut status = String::new();
    for _ in 0..400 {
        pelty_api::worker::run_once(&st, "test-sched", 20)
            .await
            .unwrap();
        let d = call(
            &app,
            "GET",
            &format!("/api/v1/admin/campaigns/{id}"),
            None,
            &ah,
        )
        .await;
        status = d.body["campaign"]["status"].as_str().unwrap().to_string();
        if status == "sent" {
            assert_eq!(d.body["campaign"]["sent"], 2);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(status, "sent");
    let sent = mailbox().sent.lock().unwrap().clone();
    let ma = sent.iter().find(|m| m.to == a.email).expect("a got mail");
    assert!(ma.text.starts_with("Dear "), "{}", ma.text);
    assert!(ma.text.contains(&format!("<{}>", a.email)));
    assert!(sent.iter().any(|m| m.to == b.email));
    assert!(!sent.iter().any(|m| m.to == other.email), "not selected");

    // Unschedule returns to draft; a stale job then does nothing.
    let r = call(&app, "POST", "/api/v1/admin/campaigns", Some(json!({"name": "Later", "subject": "s", "body": "b", "segment": {"customer_ids": [ia]}})), &ah).await;
    let id2 = r.body["id"].as_str().unwrap().to_string();
    let later = (time::OffsetDateTime::now_utc() + time::Duration::days(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/campaigns/{id2}/schedule"),
            Some(json!({"send_at": later})),
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/campaigns/{id2}/schedule"),
            None,
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/campaigns/{id2}/schedule"),
            None,
            &ah
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let r = pelty_api::routes::campaigns::run_job(&st, &json!({"id": id2, "scheduled": true}))
        .await
        .unwrap();
    assert_eq!(r["skipped"], true);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/admin/campaigns/{id2}"),
            None,
            &ah
        )
        .await
        .body["campaign"]["status"],
        "draft"
    );
}
