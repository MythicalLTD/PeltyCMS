use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use serde_json::json;
use tower::ServiceExt;

async fn cust_id(app: &axum::Router, s: &Sess) -> String {
    let p = call(app, "GET", "/api/v1/client/profile", None, &s.h()).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    p.body["id"].as_str().expect("customer id").to_string()
}

#[tokio::test]
async fn quote_send_accept_creates_invoice_once() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let cid = cust_id(&app, &alice).await;
    let _ = cust_id(&app, &bob).await;
    let body = json!({"customer_id": cid, "currency": "eur", "subject": "Dedicated setup", "valid_until": "2099-01-01",
        "items": [{"description": "Server", "quantity": "2", "unit_price": "49.995", "tax_rate": "19"}, {"description": "Setup", "quantity": "1", "unit_price": "10"}]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/quotes",
            Some(body.clone()),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let q = call(&app, "POST", "/api/v1/admin/quotes", Some(body), &admin.h()).await;
    assert_eq!(q.status, StatusCode::OK, "{:?}", q.body);
    let qid = q.body["id"].as_str().unwrap().to_string();
    assert!(q.body["number"].as_str().unwrap().starts_with("Q-"));
    // 2 × 49.995 = 99.99; tax 19% = 19.00 (rounded); + 10 = 128.99
    assert_eq!(q.body["subtotal"], "109.99");
    assert_eq!(
        q.body["total"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        "128.99".parse().unwrap()
    );

    // Drafts are invisible to the customer.
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/quotes/{qid}"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/quotes/{qid}/send"),
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
            "POST",
            &format!("/api/v1/admin/quotes/{qid}/send"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'email.send' AND payload->>'template' = 'quote_sent' AND payload->>'to' = $1",
    )
    .bind(&alice.email)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(jobs, 1, "one quote email queued");
    // Quote PDF: owner and staff get a real PDF; other customers 404.
    for (uri, h, want) in [
        (format!("/api/v1/client/quotes/{qid}/pdf"), alice.h(), StatusCode::OK),
        (format!("/api/v1/admin/quotes/{qid}/pdf"), admin.h(), StatusCode::OK),
        (format!("/api/v1/client/quotes/{qid}/pdf"), bob.h(), StatusCode::NOT_FOUND),
    ] {
        let mut b = Request::builder().uri(&uri);
        for (k, v) in &h {
            b = b.header(*k, *v);
        }
        let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), want, "{uri}");
        if want == StatusCode::OK {
            assert_eq!(res.headers()["content-type"], "application/pdf");
            let bytes = http_body_util::BodyExt::collect(res.into_body()).await.unwrap().to_bytes();
            assert!(bytes.starts_with(b"%PDF-"));
        }
    }
    let l = call(&app, "GET", "/api/v1/client/quotes", None, &alice.h()).await;
    assert_eq!(l.body["total"], 1);
    // Other customers can't see or accept it.
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/quotes/{qid}"),
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
            "POST",
            &format!("/api/v1/client/quotes/{qid}/accept"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let a = call(
        &app,
        "POST",
        &format!("/api/v1/client/quotes/{qid}/accept"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let iid = a.body["invoice_id"].as_str().unwrap().to_string();
    let inv = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{iid}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(inv.status, StatusCode::OK, "{:?}", inv.body);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/quotes/{qid}/accept"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/quotes/{qid}/decline"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    // Sent/accepted quotes can't be edited or deleted.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/quotes/{qid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Expired quote can't be accepted.
    let e = call(&app, "POST", "/api/v1/admin/quotes", Some(json!({"customer_id": cid, "currency": "EUR", "subject": "Old", "valid_until": "2000-01-01",
        "items": [{"description": "X", "quantity": "1", "unit_price": "1"}]})), &admin.h()).await;
    let eid = e.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/quotes/{eid}/send"),
        None,
        &admin.h(),
    )
    .await;
    let ex = call(
        &app,
        "POST",
        &format!("/api/v1/client/quotes/{eid}/accept"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(ex.body["error"]["code"], "error.quote.expired");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/quotes/{eid}/decline"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::OK
    );
}

async fn upload(
    app: &axum::Router,
    admin: &Sess,
    q: &str,
    data: &[u8],
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/admin/downloads?{q}"))
        .header("cookie", &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .header("content-type", "application/octet-stream")
        .body(Body::from(data.to_vec()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let b = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        st,
        serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn downloads_are_gated_by_active_service() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let acid = cust_id(&app, &alice).await;
    let _ = cust_id(&app, &bob).await;

    let (s, _) = upload(
        &app,
        &admin,
        "title=Bad&filename=x.exe",
        b"MZ\x90\x00\x03\x00",
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, b) = upload(
        &app,
        &admin,
        &format!("title=Agent&filename=../../agent.zip&products={pid}"),
        b"PK\x03\x04zipdata",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    let gated = b["id"].as_str().unwrap().to_string();
    let (s, b) = upload(&app, &admin, "title=Readme&filename=readme.txt", b"hello").await;
    assert_eq!(s, StatusCode::OK);
    let public = b["id"].as_str().unwrap().to_string();

    let lb = call(&app, "GET", "/api/v1/client/downloads", None, &bob.h()).await;
    let ids: Vec<&str> = lb.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert!(ids.contains(&public.as_str()) && !ids.contains(&gated.as_str()));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/downloads/{gated}"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Alice orders the product and gets an active service.
    let o = call(&app, "POST", "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/downloads/{gated}"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let order_id = o.body["order"]["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    sqlx::query("UPDATE services SET status = 'active' WHERE customer_id = $1::uuid")
        .bind(&acid)
        .execute(&st.pool)
        .await
        .unwrap();
    let la = call(&app, "GET", "/api/v1/client/downloads", None, &alice.h()).await;
    let d = la.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == gated.as_str())
        .expect("listed")
        .clone();
    assert_eq!(d["filename"], "agent.zip", "path components stripped");
    let req = Request::builder()
        .uri(format!("/api/v1/client/downloads/{gated}"))
        .header("cookie", &alice.cookie)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert!(res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment"));

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/downloads/{gated}"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/downloads/{gated}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
}
