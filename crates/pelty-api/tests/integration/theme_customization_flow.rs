//! Visual theme customization + active theme merge.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn customization_merges_into_active_theme() {
    let (app, _st) = app().await;
    let admin = sess(&app, &_st, "admin").await;
    let ah = admin.h();

    let put = call(
        &app,
        "PUT",
        "/api/v1/admin/themes/customization",
        Some(json!({
            "tokens": {"primary": "#ff00aa", "radius": "1.25rem"},
            "custom_css": ".card { border-width: 2px; }",
            "color_scheme": "dark"
        })),
        &ah,
    )
    .await;
    assert_eq!(put.status, StatusCode::OK, "{:?}", put.body);
    assert_eq!(put.body["tokens"]["primary"], "#ff00aa");

    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/themes/customization",
        Some(json!({"tokens": {}, "custom_css": "@import url(evil)", "color_scheme": null})),
        &ah,
    )
    .await;
    assert_eq!(
        bad.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        bad.body
    );
    assert_eq!(bad.body["error"]["code"], "error.theme_css_unsafe");

    let active = call(&app, "GET", "/api/v1/themes/active", None, &[]).await;
    assert_eq!(active.status, StatusCode::OK, "{:?}", active.body);
    assert_eq!(active.body["tokens"]["primary"], "#ff00aa");
    assert_eq!(active.body["tokens"]["radius"], "1.25rem");
    assert_eq!(active.body["custom_css"], ".card { border-width: 2px; }");
    assert_eq!(active.body["color_scheme"], "dark");

    // Reset
    let reset = call(
        &app,
        "PUT",
        "/api/v1/admin/themes/customization",
        Some(json!({"tokens": {}, "custom_css": "", "color_scheme": null})),
        &ah,
    )
    .await;
    assert_eq!(reset.status, StatusCode::OK, "{:?}", reset.body);
}
