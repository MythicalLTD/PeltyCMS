use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(
    app: &axum::Router,
    uri: &str,
    hdr: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut b = Request::builder().uri(uri);
    for (k, v) in hdr {
        b = b.header(*k, *v);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (st, h) = (res.status(), res.headers().clone());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (st, h, String::from_utf8_lossy(&body).into_owned())
}

fn temp_dist() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pelty-dist-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(d.join("assets")).unwrap();
    std::fs::write(d.join("index.html"), "<html>DEFAULT-SPA</html>").unwrap();
    std::fs::write(
        d.join("assets/app-abc123.js"),
        "console.log('x');".repeat(200),
    )
    .unwrap();
    d
}

#[tokio::test]
async fn spa_fallback_etag_cache_and_compression() {
    let (_, state) = common::app().await;
    let mut cfg = (*state.cfg).clone();
    cfg.frontend_dir = temp_dist();
    let state = pelty_api::AppState {
        cfg: std::sync::Arc::new(cfg),
        ..state
    };
    let app = pelty_api::router(state);

    let (st, h, body) = get(&app, "/admin/users", &[]).await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("DEFAULT-SPA"));
    assert_eq!(h["cache-control"], "no-cache");

    let (st, h, _) = get(&app, "/assets/app-abc123.js", &[]).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(h["cache-control"], "public, max-age=31536000, immutable");
    let etag = h["etag"].to_str().unwrap().to_string();
    let (st, _, _) = get(&app, "/assets/app-abc123.js", &[("if-none-match", &etag)]).await;
    assert_eq!(st, StatusCode::NOT_MODIFIED);

    let (_, h, _) = get(
        &app,
        "/assets/app-abc123.js",
        &[("accept-encoding", "gzip")],
    )
    .await;
    assert_eq!(h["content-encoding"], "gzip");

    assert_eq!(
        get(&app, "/assets/missing.js", &[]).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(!get(&app, "/../Cargo.toml", &[])
        .await
        .2
        .contains("[workspace]"));
    let (st, _, body) = get(&app, "/api/v1/nope", &[]).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert!(body.contains("error.not_found"));
}

#[tokio::test]
async fn active_theme_with_frontend_bundle_replaces_default() {
    let (_, state) = common::app().await;
    let mut cfg = (*state.cfg).clone();
    cfg.frontend_dir = temp_dist();
    let state = pelty_api::AppState {
        cfg: std::sync::Arc::new(cfg),
        ..state
    };
    // In-memory only, so parallel tests sharing the DB are unaffected.
    state.set_active_theme("minimal");
    let app = pelty_api::router(state.clone());
    let (st, _, body) = get(&app, "/dashboard", &[]).await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("pelty-minimal-theme"), "{body}");
    assert_eq!(get(&app, "/pelty-sdk.js", &[]).await.0, StatusCode::OK);
    state.set_active_theme("default");
    assert!(get(&app, "/dashboard", &[]).await.2.contains("DEFAULT-SPA"));
}
