use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use pelty_database::general_settings as gs;
use tower::ServiceExt;

async fn up(app: &axum::Router, s: &Sess, n: usize) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/tickets/attachments?filename=log.txt")
        .header("content-type", "text/plain")
        .header("cookie", s.cookie.as_str())
        .header("x-csrf-token", s.csrf.as_str())
        .body(Body::from(vec![b'a'; n]))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn attachment_size_limit_is_configurable() {
    let (app, state) = app().await;
    let cust = sess(&app, &state, "customer").await;
    let mut g = gs::load(&state.pool).await.unwrap();
    let before = g.clone();
    g.support.attachment_max_mb = 1;
    gs::save(&state.pool, &g).await.unwrap();
    assert_eq!(
        up(&app, &cust, 1024 * 1024 + 1).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(up(&app, &cust, 1000).await, StatusCode::OK);
    g.support.attachment_max_mb = 8;
    gs::save(&state.pool, &g).await.unwrap();
    // 6 MiB was rejected under the old fixed 5 MiB cap; now allowed.
    assert_eq!(up(&app, &cust, 6 * 1024 * 1024).await, StatusCode::OK);
    g.support.attachment_max_mb = 0;
    assert!(gs::save(&state.pool, &g).await.is_err());
    gs::save(&state.pool, &before).await.unwrap();
}
