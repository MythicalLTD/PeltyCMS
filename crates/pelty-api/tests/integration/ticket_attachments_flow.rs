use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

async fn raw(
    app: &axum::Router,
    method: &str,
    uri: &str,
    ct: &str,
    body: Vec<u8>,
    s: &Sess,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", ct)
        .header("cookie", s.cookie.as_str())
        .header("x-csrf-token", s.csrf.as_str())
        .body(Body::from(body))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let h = res.headers().clone();
    (
        st,
        h,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

async fn upload(
    app: &axum::Router,
    s: &Sess,
    name: &str,
    ct: &str,
    data: &[u8],
) -> (StatusCode, Value) {
    let (st, _, b) = raw(
        app,
        "POST",
        &format!("/api/v1/tickets/attachments?filename={name}"),
        ct,
        data.to_vec(),
        s,
    )
    .await;
    (st, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

#[tokio::test]
async fn attachments_validation_linking_and_isolation() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;

    // Type sniffing: HTML disguised as text/plain-ish, exe, mismatched image.
    assert_eq!(
        upload(&app, &cust, "x.html", "text/html", b"<script>1</script>")
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        upload(&app, &cust, "x.png", "image/png", b"MZ\x90\x00binary\x00")
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        upload(&app, &cust, "x.jpg", "image/jpeg", b"\x89PNG\r\n\x1a\n....")
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let big = vec![b'a'; 5 * 1024 * 1024 + 1];
    let (st, _) = upload(&app, &cust, "big.txt", "text/plain", &big).await;
    assert!(
        st == StatusCode::BAD_REQUEST || st == StatusCode::PAYLOAD_TOO_LARGE,
        "{st}"
    );

    let (st, a) = upload(
        &app,
        &cust,
        "..%2F..%2Fserver.log",
        "text/plain",
        b"error at line 3",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{a:?}");
    assert_eq!(a["filename"], "server.log");
    let aid = a["id"].as_str().unwrap().to_string();

    // Someone else cannot use my pending upload.
    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "Steal", "body": "x", "attachment_ids": [aid]})), &other.h()).await;
    assert_eq!(r.body["error"]["code"], "error.invalid_attachment");

    let r = call(&app, "POST", "/api/v1/client/tickets", Some(json!({"department_id": SUPPORT, "subject": "Logs", "body": "see file", "attachment_ids": [aid]})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["ticket"]["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["messages"][0]["attachments"][0]["id"], aid.as_str());
    // Re-linking an already linked file fails.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/tickets/{tid}/reply"),
        Some(json!({"body": "again", "attachment_ids": [aid]})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_attachment");

    // Download: owner ok with safe headers, other customer 404.
    let (st, h, body) = raw(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{tid}/attachments/{aid}"),
        "text/plain",
        vec![],
        &cust,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body, b"error at line 3");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment;"));
    let (st, _, _) = raw(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{tid}/attachments/{aid}"),
        "text/plain",
        vec![],
        &other,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Internal-note attachment is hidden from the customer.
    let (_, n) = upload(
        &app,
        &admin,
        "note.png",
        "image/png",
        b"\x89PNG\r\n\x1a\nxxxxxxxx",
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/tickets/{tid}/reply"),
        Some(json!({"body": "internal", "internal": true, "attachment_ids": [nid]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let (st, _, _) = raw(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{tid}/attachments/{nid}"),
        "text/plain",
        vec![],
        &cust,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = raw(
        &app,
        "GET",
        &format!("/api/v1/admin/tickets/{tid}/attachments/{nid}"),
        "text/plain",
        vec![],
        &admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/tickets/{tid}"),
        None,
        &cust.h(),
    )
    .await;
    assert!(!r.body.to_string().contains(&nid));
}
