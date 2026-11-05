use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 255) as u8, (y % 255) as u8, 90])
    });
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

async fn put(
    app: &axum::Router,
    s: &Sess,
    ct: &str,
    body: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/auth/avatar")
        .header("cookie", &s.cookie)
        .header("x-csrf-token", &s.csrf)
        .header("content-type", ct)
        .body(Body::from(body))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let b = res.into_body().collect().await.unwrap().to_bytes();
    (st, serde_json::from_slice(&b).unwrap_or_default())
}

async fn get(
    app: &axum::Router,
    s: &Sess,
    uri: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .uri(uri)
        .header("cookie", &s.cookie)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let (st, h) = (res.status(), res.headers().clone());
    (
        st,
        h,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

#[tokio::test]
async fn upload_reencode_serve_remove() {
    let (app, state) = app().await;
    let s = sess(&app, &state, "customer").await;
    let me = call(&app, "GET", "/api/v1/auth/me", None, &s.h()).await;
    assert!(me.body["avatar_version"].is_null());
    let id = me.body["id"].as_str().unwrap().to_string();

    // Polyglot: valid PNG with an HTML/script payload appended.
    let mut poly = png(300, 200);
    poly.extend_from_slice(b"<html><script>alert(1)</script></html>");
    let (st, b) = put(&app, &s, "image/png", poly).await;
    assert_eq!(st, StatusCode::OK, "{b:?}");
    let v = b["avatar_version"].as_i64().unwrap();
    let (st, h, bytes) = get(&app, &s, &format!("/api/v1/avatars/{id}?v={v}")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["cache-control"].to_str().unwrap().contains("max-age"));
    assert!(
        !bytes.windows(7).any(|w| w == b"<script"),
        "payload stripped by re-encode"
    );
    let img = image::load_from_memory(&bytes).unwrap();
    assert_eq!((img.width(), img.height()), (256, 256));
    let (_, _, small) = get(&app, &s, &format!("/api/v1/avatars/{id}?s=64")).await;
    assert_eq!(image::load_from_memory(&small).unwrap().width(), 64);
    let etag = h["etag"].to_str().unwrap().to_string();
    let req = Request::builder()
        .uri(format!("/api/v1/avatars/{id}"))
        .header("cookie", &s.cookie)
        .header("if-none-match", etag)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", None, &s.h())
            .await
            .body["avatar_version"],
        v
    );

    // Remove.
    assert_eq!(
        call(&app, "DELETE", "/api/v1/auth/avatar", None, &s.h())
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, &s, &format!("/api/v1/avatars/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(call(&app, "GET", "/api/v1/auth/me", None, &s.h())
        .await
        .body["avatar_version"]
        .is_null());
}

#[tokio::test]
async fn upload_rejects_bad_input() {
    let (app, state) = app().await;
    let s = sess(&app, &state, "customer").await;
    // Wrong type: SVG / HTML / GIF.
    let (st, b) = put(
        &app,
        &s,
        "image/svg+xml",
        b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>".to_vec(),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["error"]["code"], "error.avatar_type");
    let (st, _) = put(&app, &s, "image/png", b"GIF89a\x01\x00\x01\x00".to_vec()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    // Declared type does not match the bytes.
    let (st, _) = put(&app, &s, "image/jpeg", png(64, 64)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    // Valid magic, garbage body.
    let mut trunc = png(64, 64);
    trunc.truncate(40);
    let (st, b) = put(&app, &s, "image/png", trunc).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["error"]["code"], "error.avatar_invalid");
    // Oversized.
    let mut big = png(64, 64);
    big.resize(2 * 1024 * 1024 + 10, 0);
    let (st, _) = put(&app, &s, "image/png", big).await;
    assert!(
        st == StatusCode::BAD_REQUEST || st == StatusCode::PAYLOAD_TOO_LARGE,
        "{st}"
    );
    // Unauthenticated / no CSRF.
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/auth/avatar")
        .header("cookie", &s.cookie)
        .header("content-type", "image/png")
        .body(Body::from(png(32, 32)))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let req = Request::builder()
        .uri(format!("/api/v1/avatars/{}", uuid::Uuid::new_v4()))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[test]
fn decompression_bomb_dimensions_rejected() {
    // 5000x5000 exceeds the 4096 limit even though the file is tiny.
    let img = image::GrayImage::new(5000, 5000);
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    assert!(out.len() < 2 * 1024 * 1024);
    assert_eq!(
        pelty_api::routes::avatars::process(Some("image/png"), &out).unwrap_err(),
        "error.avatar_invalid"
    );
}
