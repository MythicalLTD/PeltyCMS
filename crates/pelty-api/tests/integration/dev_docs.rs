//! `/docs` serves the generated docs (user set, developers/ set): staff only by default,
//! public with PELTY_DOCS_PUBLIC, never outside the docs folder.
use crate::common;
use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn docs_are_staff_only_unless_public() {
    let dir = std::env::temp_dir().join(format!("pelty-docs-serve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<h1>dev docs</h1>").unwrap();
    std::fs::create_dir_all(dir.join("developers")).unwrap();
    std::fs::write(dir.join("developers/index.html"), "<h1>developer docs</h1>").unwrap();
    std::fs::write(dir.join("developers/events.md"), "# Events").unwrap();
    std::env::set_var("PELTY_DOCS_DIR", &dir);
    std::env::remove_var("PELTY_DOCS_PUBLIC");
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;

    assert_eq!(raw_get(&app, "/docs/", &[]).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(raw_get(&app, "/docs/", &alice.h()).await.0, StatusCode::FORBIDDEN);
    let (code, h, body) = raw_get(&app, "/docs/", &admin.h()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("dev docs"));
    assert!(h.get("content-type").unwrap().to_str().unwrap().starts_with("text/html"));
    let (code, h, _) = raw_get(&app, "/docs/developers/events.md", &admin.h()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(h.get("content-type").unwrap().to_str().unwrap().starts_with("text/markdown"));
    let (code, _, body) = raw_get(&app, "/docs/developers/", &admin.h()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("developer docs"));
    assert_eq!(raw_get(&app, "/docs/developers/../../Cargo.toml", &admin.h()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(raw_get(&app, "/docs/../Cargo.toml", &admin.h()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(raw_get(&app, "/docs/nope.html", &admin.h()).await.0, StatusCode::NOT_FOUND);
    // A missing page index answers an empty list (the admin page's probe), still staff only.
    let (code, _, body) = raw_get(&app, "/docs/developers/pages.json", &admin.h()).await;
    assert_eq!((code, &body[..]), (StatusCode::OK, &b"[]"[..]));
    assert_eq!(raw_get(&app, "/docs/developers/pages.json", &[]).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(raw_get(&app, "/docs", &[]).await.0, StatusCode::PERMANENT_REDIRECT);

    std::env::set_var("PELTY_DOCS_PUBLIC", "true");
    assert_eq!(raw_get(&app, "/docs/", &[]).await.0, StatusCode::OK);
    std::env::remove_var("PELTY_DOCS_PUBLIC");
    std::env::set_var("PELTY_DOCS_DIR", dir.join("missing"));
    assert_eq!(raw_get(&app, "/docs/", &admin.h()).await.0, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(&dir);
}
async fn raw_get(app: &axum::Router, uri: &str, headers: &[(&str, &str)]) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    use tower::ServiceExt;
    let mut b = axum::http::Request::builder().uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let r = app.clone().oneshot(b.body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let (p, body) = r.into_parts();
    (p.status, p.headers, axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec())
}
