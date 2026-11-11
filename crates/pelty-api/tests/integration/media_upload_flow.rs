//! Image uploads: staff `/admin/media` and marketplace `/client/creator/media`
//! (PNG/JPEG/WebP/GIF re-encoded to PNG, resized, thumbnail, per-user quota),
//! plus the creator banner on the public profile.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn img(w: u32, h: u32, fmt: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x % 255) as u8, (y % 255) as u8, 90, 255])
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), fmt)
        .unwrap();
    out
}

async fn post(
    app: &axum::Router,
    s: &Sess,
    uri: &str,
    ct: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
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

async fn fetch(app: &axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    (
        st,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

fn dims(b: &[u8]) -> (u32, u32) {
    let i = image::load_from_memory(b).unwrap();
    (i.width(), i.height())
}

fn cleanup(ids: &[String]) {
    let dir = pelty_api::routes::media::media_dir();
    for id in ids {
        let _ = std::fs::remove_file(dir.join(format!("{id}.png")));
        let _ = std::fs::remove_file(dir.join(format!("{id}_t.png")));
    }
}

#[tokio::test]
async fn staff_and_creator_uploads() {
    let _serial = CREATOR_PAYOUTS.lock().await;
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let carol = sess(&app, &st, "customer").await;
    let mut ids = Vec::new();

    // Staff: big PNG is capped at 2048 px with a 480 px thumbnail.
    let (s, b) = post(
        &app,
        &admin,
        "/api/v1/admin/media",
        "image/png",
        img(3000, 1000, image::ImageFormat::Png),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    ids.push(b["id"].as_str().unwrap().to_string());
    assert_eq!(
        (b["width"].as_u64(), b["height"].as_u64()),
        (Some(2048), Some(683))
    );
    let (s, full) = fetch(&app, b["path"].as_str().unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(dims(&full).0, 2048);
    let (s, thumb) = fetch(&app, b["thumb"].as_str().unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(dims(&thumb).0, 480);

    // GIF accepted (first frame, re-encoded as PNG); type must match the bytes.
    let (s, b) = post(
        &app,
        &admin,
        "/api/v1/admin/media",
        "image/gif",
        img(64, 64, image::ImageFormat::Gif),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    ids.push(b["id"].as_str().unwrap().to_string());
    let (_, bytes) = fetch(&app, b["path"].as_str().unwrap()).await;
    assert!(bytes.starts_with(b"\x89PNG"));
    let (s, b) = post(
        &app,
        &admin,
        "/api/v1/admin/media",
        "image/png",
        img(64, 64, image::ImageFormat::Gif),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["error"]["code"], "error.media_type");
    let (s, _) = post(
        &app,
        &admin,
        "/api/v1/admin/media",
        "image/svg+xml",
        b"<svg/>".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // Customers cannot use the staff endpoint.
    let (s, _) = post(
        &app,
        &carol,
        "/api/v1/admin/media",
        "image/png",
        img(64, 64, image::ImageFormat::Png),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Creator endpoint: 404 while the marketplace module is off.
    let (s, _) = post(
        &app,
        &carol,
        "/api/v1/client/creator/media",
        "image/png",
        img(64, 64, image::ImageFormat::Png),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    enable(&app, &admin, &["digital_products", "creator_marketplace"]).await;
    let (s, b) = post(
        &app,
        &carol,
        "/api/v1/client/creator/media",
        "image/jpeg",
        img(800, 400, image::ImageFormat::Jpeg),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    ids.push(b["id"].as_str().unwrap().to_string());
    let avatar = b["path"].as_str().unwrap().to_string();
    let (s, b) = post(
        &app,
        &carol,
        "/api/v1/client/creator/media",
        "image/webp",
        img(1200, 300, image::ImageFormat::WebP),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    ids.push(b["id"].as_str().unwrap().to_string());
    let banner = b["path"].as_str().unwrap().to_string();

    // Uploaded paths work as creator avatar and banner.
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let slug = format!("media-{tag}");
    let r = call(&app, "POST", "/api/v1/client/creator", Some(json!({"slug": slug, "display_name": "Media Carol", "avatar_url": avatar, "banner_url": banner})), &carol.h()).await;
    assert!(r.status.is_success(), "{:?}", r.body);
    assert_eq!(
        r.body["creator"]["banner_url"]
            .as_str()
            .or(r.body["banner_url"].as_str()),
        Some(banner.as_str()),
        "{:?}",
        r.body
    );
    let r = call(&app, "POST", "/api/v1/client/creator", Some(json!({"slug": format!("x-{tag}"), "display_name": "X", "banner_url": "javascript:alert(1)"})), &sess(&app, &st, "customer").await.h()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Per-user hourly limit.
    let me = call(&app, "GET", "/api/v1/auth/me", None, &carol.h()).await;
    let uid: uuid::Uuid = me.body["id"].as_str().unwrap().parse().unwrap();
    for _ in 0..pelty_database::media::CREATOR_PER_HOUR {
        pelty_database::media::record(
            &st.pool,
            uuid::Uuid::new_v4(),
            Some(uid),
            "creator",
            10,
            (16, 16),
        )
        .await
        .unwrap();
    }
    let (s, b) = post(
        &app,
        &carol,
        "/api/v1/client/creator/media",
        "image/png",
        img(64, 64, image::ImageFormat::Png),
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{b:?}");

    cleanup(&ids);
    reset_modules(&st).await;
}
