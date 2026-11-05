//! Changes the global trusted-proxy list; own binary, reset at the end.
use crate::common;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

async fn seen_ip(
    app: &axum::Router,
    h: &[(&str, &str)],
    peer: [u8; 4],
    xff: Option<&str>,
) -> Value {
    let mut b = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/security/ip-rules");
    for (k, v) in h {
        b = b.header(*k, *v);
    }
    if let Some(x) = xff {
        b = b.header("x-forwarded-for", x);
    }
    let mut req = b.body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((peer, 40000))));
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    v["your_ip"].clone()
}

#[tokio::test]
async fn trusted_proxy_forwarded_for() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let h = admin.h();
    let url = "/api/v1/admin/security/trusted-proxies";
    assert_eq!(
        call(&app, "GET", url, None, &cust.h()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "PUT", url, Some(json!({"proxies": ["nope"]})), &h)
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            url,
            Some(json!({"proxies": ["0.0.0.0/0"]})),
            &h
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Not configured: the header is ignored.
    let r = call(&app, "PUT", url, Some(json!({"proxies": []})), &h).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        seen_ip(&app, &h, [10, 0, 0, 5], Some("198.51.100.7")).await,
        "10.0.0.5"
    );

    let r = call(
        &app,
        "PUT",
        url,
        Some(json!({"proxies": ["10.0.0.0/8", "10.0.0.0/8", "192.0.2.1"]})),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["proxies"], json!(["10.0.0.0/8", "192.0.2.1/32"]));
    assert_eq!(
        call(&app, "GET", url, None, &h).await.body["proxies"],
        json!(["10.0.0.0/8", "192.0.2.1/32"])
    );
    assert_eq!(
        seen_ip(
            &app,
            &h,
            [10, 0, 0, 5],
            Some("6.6.6.6, 198.51.100.7, 192.0.2.1")
        )
        .await,
        "198.51.100.7"
    );
    // An untrusted peer cannot spoof its address.
    assert_eq!(
        seen_ip(&app, &h, [203, 0, 113, 9], Some("198.51.100.7")).await,
        "203.0.113.9"
    );
    // X-Real-IP (nginx `proxy_set_header X-Real-IP`) from a trusted peer only.
    let hr = [h[0], h[1], ("x-real-ip", "198.51.100.9")];
    assert_eq!(
        seen_ip(&app, &hr, [10, 0, 0, 5], None).await,
        "198.51.100.9"
    );
    assert_eq!(
        seen_ip(&app, &hr, [203, 0, 113, 9], None).await,
        "203.0.113.9"
    );

    assert_eq!(
        call(&app, "PUT", url, Some(json!({"proxies": []})), &h)
            .await
            .status,
        StatusCode::OK
    );
}
