//! KYC: settings, uploads (type-sniffed), review, derived status and order gating.
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n0000fakepngbody";
const PDF: &[u8] = b"%PDF-1.4 fake";

async fn up(app: &axum::Router, s: &Sess, kind: &str, ct: &str, data: &[u8]) -> (StatusCode, Value) {
    let req = Request::builder().method("POST").uri(format!("/api/v1/client/kyc/documents?kind={kind}&filename=../id%20card.png"))
        .header("content-type", ct).header("cookie", s.cookie.as_str()).header("x-csrf-token", s.csrf.as_str()).body(Body::from(data.to_vec())).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let b = res.into_body().collect().await.unwrap().to_bytes();
    (st, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

async fn rv(app: &axum::Router, admin: &Sess, id: &Value, approve: bool, note: &str) -> Resp {
    call(app, "POST", &format!("/api/v1/admin/kyc/documents/{}/review", id["id"].as_str().unwrap()), Some(json!({"approve": approve, "note": note})), &admin.h()).await
}

#[tokio::test]
async fn kyc_upload_review_and_gate() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let other = sess(&app, &st, "customer").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let order = || json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]});

    call(&app, "PUT", "/api/v1/admin/kyc/settings", Some(json!({"enabled": false})), &admin.h()).await;
    // Disabled by default: uploads refused, orders fine.
    assert_eq!(up(&app, &cust, "identity", "image/png", PNG).await.0, StatusCode::CONFLICT);
    assert_eq!(call(&app, "POST", "/api/v1/client/orders", Some(order()), &cust.h()).await.status, StatusCode::OK);

    assert_eq!(call(&app, "PUT", "/api/v1/admin/kyc/settings", Some(json!({"enabled": true, "required_kinds": ["passport"]})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/kyc/settings", Some(json!({"enabled": true})), &cust.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "PUT", "/api/v1/admin/kyc/settings", Some(json!({"enabled": true, "required_kinds": ["identity", "address"], "gate_orders": true, "max_mb": 2})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Gate: unverified customer cannot order products or domains.
    let o = call(&app, "POST", "/api/v1/client/orders", Some(order()), &cust.h()).await;
    assert_eq!((o.status, o.body["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("error.kyc_required")), "{:?}", o.body);

    // Uploads: type sniffed, text refused, filename sanitised.
    assert_eq!(up(&app, &cust, "identity", "text/plain", b"hello").await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(up(&app, &cust, "identity", "application/pdf", PNG).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(up(&app, &cust, "selfie", "image/png", PNG).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (s1, id_doc) = up(&app, &cust, "identity", "image/png", PNG).await;
    assert_eq!(s1, StatusCode::OK, "{id_doc:?}");
    assert_eq!(id_doc["filename"], "id card.png");
    let (_, addr_doc) = up(&app, &cust, "address", "application/pdf", PDF).await;
    let me = call(&app, "GET", "/api/v1/client/kyc", None, &cust.h()).await;
    assert_eq!(me.body["status"]["kyc_status"], "pending");
    assert_eq!(me.body["documents"].as_array().unwrap().len(), 2);
    assert!(me.body["documents"][0].get("data").is_none());

    // Others can't see or delete it; admin queue lists it.
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/client/kyc/documents/{}", id_doc["id"].as_str().unwrap()), None, &other.h()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/v1/admin/kyc", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let q = call(&app, "GET", "/api/v1/admin/kyc?status=pending", None, &admin.h()).await;
    let row = q.body["items"].as_array().unwrap().iter().find(|r| r["email"] == cust.email.as_str()).cloned().unwrap();
    assert_eq!(row["pending"], 2);
    let cid = row["customer_id"].as_str().unwrap().to_string();

    // Download is a sandboxed attachment.
    let req = Request::builder().uri(format!("/api/v1/admin/kyc/documents/{}/download", id_doc["id"].as_str().unwrap())).header("cookie", admin.cookie.as_str()).body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert!(res.headers()["content-disposition"].to_str().unwrap().starts_with("attachment"));
    assert_eq!(res.into_body().collect().await.unwrap().to_bytes().as_ref(), PNG);

    // Reject needs a note; approve identity, reject address → rejected.
    assert_eq!(rv(&app, &admin, &addr_doc, false, "").await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rv(&app, &admin, &id_doc, true, "").await.body["kyc_status"], "pending");
    assert_eq!(rv(&app, &admin, &addr_doc, false, "Blurry").await.body["kyc_status"], "rejected");
    let me = call(&app, "GET", "/api/v1/client/kyc", None, &cust.h()).await;
    assert_eq!(me.body["status"]["kyc_note"], "Blurry");
    // Reviewed docs can't be withdrawn.
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/client/kyc/documents/{}", addr_doc["id"].as_str().unwrap()), None, &cust.h()).await.status, StatusCode::NOT_FOUND);

    // Re-upload address, approve → verified → can order.
    let (_, addr2) = up(&app, &cust, "address", "image/png", PNG).await;
    assert_eq!(rv(&app, &admin, &addr2, true, "").await.body["kyc_status"], "verified");
    assert_eq!(call(&app, "POST", "/api/v1/client/orders", Some(order()), &cust.h()).await.status, StatusCode::OK);

    // Admin override back to rejected re-gates.
    let ov = call(&app, "PUT", &format!("/api/v1/admin/customers/{cid}/kyc"), Some(json!({"status": "rejected", "note": "Expired"})), &admin.h()).await;
    assert_eq!(ov.status, StatusCode::OK, "{:?}", ov.body);
    assert_eq!(call(&app, "POST", "/api/v1/client/orders", Some(order()), &cust.h()).await.status, StatusCode::CONFLICT);
    let d = call(&app, "GET", &format!("/api/v1/admin/customers/{cid}/kyc"), None, &admin.h()).await;
    assert_eq!(d.body["documents"].as_array().unwrap().len(), 3);
    // Size cap from settings (2 MiB).
    let mut big = PNG.to_vec();
    big.resize(3 * 1024 * 1024, 0);
    assert_eq!(up(&app, &cust, "other", "image/png", &big).await.0, StatusCode::BAD_REQUEST);
    // Disable gating → orders allowed again regardless of status.
    call(&app, "PUT", "/api/v1/admin/kyc/settings", Some(json!({"enabled": true, "required_kinds": ["identity"], "gate_orders": false})), &admin.h()).await;
    assert_eq!(call(&app, "POST", "/api/v1/client/orders", Some(order()), &cust.h()).await.status, StatusCode::OK);
    // Leave the global setting off for other tests sharing the DB.
    call(&app, "PUT", "/api/v1/admin/kyc/settings", Some(json!({"enabled": false})), &admin.h()).await;
}
