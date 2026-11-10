//! Product reviews and Q&A: verified-purchase gate, moderation, staff reply,
//! edits re-entering moderation, rating summary, questions and answers, and
//! 404 while the module is off.
use crate::common;
use crate::modules_flow::reset_modules;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn settings(app: &axum::Router, admin: &Sess, settings: Value) {
    let r = call(
        app,
        "PUT",
        "/api/v1/admin/modules/product_reviews",
        Some(json!({"enabled": true, "settings": settings})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

async fn public(app: &axum::Router, pid: &str) -> Value {
    let r = call(
        app,
        "GET",
        &format!("/api/v1/public/reviews/products/{pid}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

#[tokio::test]
async fn reviews_moderation_and_questions() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let carol = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];

    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Ranks", "slug": format!("ranks-{tag}")})),
        &admin.h(),
    )
    .await;
    let gid = g.body["id"].as_str().unwrap().to_string();
    let p = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": gid, "name": "VIP", "slug": format!("vip-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let pid = p.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "10.00"}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let url = format!("/api/v1/client/reviews/products/{pid}");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/public/reviews/products/{pid}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "module off"
    );
    settings(
        &app,
        &admin,
        json!({"require_approval": true, "verified_only": true}),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/public/reviews/products/{}", uuid::Uuid::new_v4()),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Verified-only: no purchase, no review.
    let e = call(&app, "GET", &url, None, &bob.h()).await;
    assert_eq!(e.body["purchased"], false);
    assert_eq!(e.body["can_review"], false);
    let five = json!({"rating": 5, "title": "Great rank", "body": "Worth it"});
    let r = call(&app, "POST", &url, Some(five.clone()), &bob.h()).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.review_purchase_required");

    // Bob buys it.
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let pay = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("rv-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(pay.status, StatusCode::OK, "{:?}", pay.body);
    assert_eq!(
        call(&app, "GET", &url, None, &bob.h()).await.body["purchased"],
        true
    );
    let bad = call(&app, "POST", &url, Some(json!({"rating": 6})), &bob.h()).await;
    assert_eq!(
        bad.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        bad.body
    );
    assert_eq!(bad.body["error"]["code"], "error.review_invalid");
    let r = call(&app, "POST", &url, Some(five.clone()), &bob.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "pending");
    assert_eq!(r.body["verified"], true);
    let rid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(public(&app, &pid).await["summary"]["count"], 0);

    // Moderation.
    let q = call(
        &app,
        "GET",
        "/api/v1/admin/reviews?status=pending",
        None,
        &admin.h(),
    )
    .await;
    assert!(q.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == rid.as_str() && r["product_name"] == "VIP"));
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/reviews", None, &bob.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let ok = call(
        &app,
        "POST",
        &format!("/api/v1/admin/reviews/{rid}/status"),
        Some(json!({"status": "approved"})),
        &admin.h(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    let rep = call(
        &app,
        "POST",
        &format!("/api/v1/admin/reviews/{rid}/reply"),
        Some(json!({"text": "Thanks!"})),
        &admin.h(),
    )
    .await;
    assert_eq!(rep.status, StatusCode::OK, "{:?}", rep.body);
    let pb = public(&app, &pid).await;
    assert_eq!(pb["summary"]["count"], 1);
    assert_eq!(pb["summary"]["average"], "5.00");
    assert_eq!(pb["summary"]["histogram"], json!([0, 0, 0, 0, 1]));
    assert_eq!(pb["items"][0]["verified"], true);
    assert_eq!(pb["items"][0]["reply"], "Thanks!");
    assert!(pb["items"][0].get("customer_id").is_none(), "no ids leak");

    // Editing re-enters moderation and clears the reply.
    let r = call(
        &app,
        "POST",
        &url,
        Some(json!({"rating": 3, "body": "Changed my mind"})),
        &bob.h(),
    )
    .await;
    assert_eq!(r.body["id"], rid.as_str());
    assert_eq!(r.body["status"], "pending");
    assert_eq!(r.body["reply"], "");
    assert_eq!(public(&app, &pid).await["summary"]["count"], 0);

    // Open reviews, no approval: Carol reviews without buying.
    settings(
        &app,
        &admin,
        json!({"require_approval": false, "verified_only": false}),
    )
    .await;
    let r = call(&app, "POST", &url, Some(json!({"rating": 4})), &carol.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "approved");
    assert_eq!(r.body["verified"], false);
    let crid = r.body["id"].as_str().unwrap().to_string();
    let pb = public(&app, &pid).await;
    assert_eq!(pb["summary"]["count"], 1);
    assert_eq!(pb["summary"]["average"], "4.00");

    // Questions.
    let qurl = format!("{url}/questions");
    let short = call(&app, "POST", &qurl, Some(json!({"text": "?"})), &carol.h()).await;
    assert_eq!(short.status, StatusCode::UNPROCESSABLE_ENTITY);
    let qa = call(
        &app,
        "POST",
        &qurl,
        Some(json!({"text": "Does it include fly?"})),
        &carol.h(),
    )
    .await;
    assert_eq!(qa.status, StatusCode::OK, "{:?}", qa.body);
    assert_eq!(qa.body["status"], "approved");
    let qid = qa.body["id"].as_str().unwrap().to_string();
    let ans = call(
        &app,
        "POST",
        &format!("/api/v1/admin/reviews/questions/{qid}/answer"),
        Some(json!({"text": "Yes, /fly is included."})),
        &admin.h(),
    )
    .await;
    assert_eq!(ans.status, StatusCode::OK, "{:?}", ans.body);
    let pq = call(
        &app,
        "GET",
        &format!("/api/v1/public/reviews/products/{pid}/questions"),
        None,
        &[],
    )
    .await;
    assert_eq!(pq.body["items"][0]["answer"], "Yes, /fly is included.");
    let aq = call(
        &app,
        "GET",
        "/api/v1/admin/reviews/questions",
        None,
        &admin.h(),
    )
    .await;
    assert!(aq.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|q| q["id"] == qid.as_str()));
    settings(&app, &admin, json!({"questions": false})).await;
    let closed = call(
        &app,
        "POST",
        &qurl,
        Some(json!({"text": "Another question?"})),
        &carol.h(),
    )
    .await;
    assert_eq!(closed.status, StatusCode::CONFLICT);
    assert_eq!(closed.body["error"]["code"], "error.questions_closed");

    // Creators only: Bob is not one.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/reviews/creator",
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // Delete: only your own.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/client/reviews/{crid}"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/client/reviews/{crid}"),
            None,
            &carol.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(public(&app, &pid).await["summary"]["count"], 0);
    reset_modules(&st).await;
}
