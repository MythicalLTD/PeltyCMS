//! Creator side of reviews and Q&A: the owning creator replies and answers,
//! sees no customer email or id, cannot answer unapproved or rejected
//! questions, and other creators cannot touch the product.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn put_module(app: &axum::Router, admin: &Sess, id: &str, settings: Value) {
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/admin/modules/{id}"),
        Some(json!({"enabled": true, "settings": settings})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

async fn creator(app: &axum::Router, s: &Sess, slug: &str) {
    let r = call(
        app,
        "POST",
        "/api/v1/client/creator",
        Some(json!({"slug": slug, "display_name": slug, "payout_method": "manual"})),
        &s.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "approved");
}

async fn answer(app: &axum::Router, id: &str, who: &Sess) -> StatusCode {
    call(
        app,
        "POST",
        &format!("/api/v1/client/reviews/creator/questions/{id}/answer"),
        Some(json!({"text": "1.20 and up"})),
        &who.h(),
    )
    .await
    .status
}

#[tokio::test]
async fn creator_replies_and_answers() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let dave = sess(&app, &st, "customer").await;
    let erin = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    enable(&app, &admin, &["digital_products"]).await;
    put_module(&app, &admin, "creator_marketplace", json!({"mode": "multi_vendor", "auto_approve_creators": true, "auto_approve_listings": true})).await;
    put_module(
        &app,
        &admin,
        "product_reviews",
        json!({"require_approval": true}),
    )
    .await;
    creator(&app, &dave, &format!("dave-{tag}")).await;
    creator(&app, &erin, &format!("erin-{tag}")).await;

    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Maps", "slug": format!("maps-{tag}")})),
        &admin.h(),
    )
    .await;
    let gid = g.body["id"].as_str().unwrap().to_string();
    let l = call(
        &app,
        "POST",
        "/api/v1/client/creator/listings",
        Some(json!({"group_id": gid, "name": "Island Map", "slug": format!("island-{tag}"), "description": "A map", "currency": "EUR", "cycle": "one_time", "price": "5.00"})),
        &dave.h(),
    )
    .await;
    assert_eq!(l.status, StatusCode::OK, "{:?}", l.body);
    assert_eq!(l.body["product_status"], "active");
    let pid = l.body["product_id"].as_str().unwrap().to_string();

    // Bob reviews; staff approve.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/reviews/products/{pid}"),
        Some(json!({"rating": 4, "body": "Nice map"})),
        &bob.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rid = r.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/reviews/{rid}/status"),
        Some(json!({"status": "approved"})),
        &admin.h(),
    )
    .await;

    // Two questions: one stays pending, one is approved, one rejected later.
    let ask = |text: &'static str| {
        let app = app.clone();
        let h = bob.h();
        let url = format!("/api/v1/client/reviews/products/{pid}/questions");
        async move {
            let q = call(&app, "POST", &url, Some(json!({"text": text})), &h).await;
            assert_eq!(q.status, StatusCode::OK, "{:?}", q.body);
            q.body["id"].as_str().unwrap().to_string()
        }
    };
    let pending = ask("Is it multiplayer?").await;
    let approved = ask("Which version?").await;
    let rejected = ask("Spam question here").await;
    for (id, s) in [(&approved, "approved"), (&rejected, "approved")] {
        let r = call(
            &app,
            "POST",
            &format!("/api/v1/admin/reviews/questions/{id}/status"),
            Some(json!({"status": s})),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
    }

    // Inbox: no customer data, no pending questions while approval is on.
    let inbox = call(
        &app,
        "GET",
        "/api/v1/client/reviews/creator",
        None,
        &dave.h(),
    )
    .await;
    assert_eq!(inbox.status, StatusCode::OK);
    assert_eq!(inbox.body["creator"], true);
    let text = inbox.body.to_string();
    assert!(
        !text.contains("customer_email") && !text.contains("customer_id"),
        "{text}"
    );
    assert_eq!(inbox.body["reviews"][0]["id"], rid.as_str());
    assert!(inbox.body["reviews"][0]["author"].is_string());
    let qids: Vec<&str> = inbox.body["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["id"].as_str().unwrap())
        .collect();
    assert!(qids.contains(&approved.as_str()) && !qids.contains(&pending.as_str()));

    // Owner replies and answers.
    let rep = call(
        &app,
        "POST",
        &format!("/api/v1/client/reviews/creator/reviews/{rid}/reply"),
        Some(json!({"text": "Thanks Bob!"})),
        &dave.h(),
    )
    .await;
    assert_eq!(rep.status, StatusCode::OK, "{:?}", rep.body);
    assert_eq!(rep.body["reply"], "Thanks Bob!");
    assert_eq!(answer(&app, &approved, &dave).await, StatusCode::OK);
    assert_eq!(
        answer(&app, &pending, &dave).await,
        StatusCode::NOT_FOUND,
        "pending needs staff approval first"
    );
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/reviews/questions/{rejected}/status"),
        Some(json!({"status": "rejected"})),
        &admin.h(),
    )
    .await;
    assert_eq!(answer(&app, &rejected, &dave).await, StatusCode::NOT_FOUND);
    // Another creator cannot touch Dave's product.
    assert_eq!(answer(&app, &approved, &erin).await, StatusCode::NOT_FOUND);
    let other = call(
        &app,
        "POST",
        &format!("/api/v1/client/reviews/creator/reviews/{rid}/reply"),
        Some(json!({"text": "Hijack"})),
        &erin.h(),
    )
    .await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);

    // Public sees the creator's reply and answer.
    let pb = call(
        &app,
        "GET",
        &format!("/api/v1/public/reviews/products/{pid}"),
        None,
        &[],
    )
    .await;
    assert_eq!(pb.body["items"][0]["reply"], "Thanks Bob!");
    let pq = call(
        &app,
        "GET",
        &format!("/api/v1/public/reviews/products/{pid}/questions"),
        None,
        &[],
    )
    .await;
    assert!(pq.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|q| q["answer"] == "1.20 and up"));
    reset_modules(&st).await;
}
