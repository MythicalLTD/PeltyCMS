use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn site_config_and_pages() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;

    let p = call(&app, "GET", "/api/v1/public/site", None, &[]).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert!(p.body["config"]["site_name"].is_string());
    let home = call(&app, "GET", "/api/v1/public/site/pages/home", None, &[]).await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["type"] == "pricing"));
    for slug in ["about", "terms", "privacy"] {
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/v1/public/site/pages/{slug}"),
                None,
                &[]
            )
            .await
            .status,
            StatusCode::OK,
            "{slug}"
        );
    }

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/site", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let mut cfg = call(&app, "GET", "/api/v1/admin/site", None, &admin.h())
        .await
        .body["config"]
        .clone();
    cfg["primary_color"] = json!("javascript");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/site",
            Some(cfg.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    cfg["primary_color"] = json!("#59b5d3");
    cfg["nav"] = json!([{"label": "Bad", "href": "javascript:alert(1)"}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/site",
            Some(cfg.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    cfg["nav"] = json!([{"label": "Plans", "href": "/store"}, {"label": "Docs", "href": "https://docs.example"}]);
    cfg["social"] = json!([{"network": "myspace", "href": "https://x.example"}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/site",
            Some(cfg.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    cfg["social"] = json!([{"network": "github", "href": "javascript:x"}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/site",
            Some(cfg.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    cfg["social"] = json!([{"network": "github", "href": "https://github.com/MythicalLTD"}]);
    cfg["copyright"] = json!("© Acme");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/site",
        Some(cfg.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let p = call(&app, "GET", "/api/v1/public/site", None, &[]).await;
    assert_eq!(p.body["config"]["primary_color"], "#59b5d3");
    assert_eq!(p.body["config"]["social"][0]["network"], "github");
    assert_eq!(p.body["config"]["copyright"], "© Acme");
    // Regression: a double-encoded body (JSON string, not object) is rejected,
    // while changing only the tagline round-trips to the public config.
    let as_string = json!(cfg.to_string());
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/site",
            Some(as_string),
            &admin.h()
        )
        .await
        .status
        .as_u16()
            / 100,
        4
    );
    cfg["tagline"] = json!("Only the tagline changed");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/site",
            Some(cfg.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/public/site", None, &[])
            .await
            .body["config"]["tagline"],
        "Only the tagline changed"
    );

    let slug = format!("t-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let bad = json!({"slug": slug, "title": "T", "blocks": [{"type": "script", "props": {}}]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/site/pages",
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let body = json!({"slug": slug, "title": "About", "blocks": [{"type": "text", "props": {"body": "Hi"}}], "published": false, "show_in_nav": true});
    let c = call(
        &app,
        "POST",
        "/api/v1/admin/site/pages",
        Some(body.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    let id = c.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/site/pages",
            Some(body.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    // Drafts are invisible publicly.
    let pub_uri = format!("/api/v1/public/site/pages/{slug}");
    assert_eq!(
        call(&app, "GET", &pub_uri, None, &[]).await.status,
        StatusCode::NOT_FOUND
    );
    let mut b2 = body.clone();
    b2["published"] = json!(true);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/site/pages/{id}"),
            Some(b2),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let g = call(&app, "GET", &pub_uri, None, &[]).await;
    assert_eq!(
        (g.status, g.body["title"].as_str()),
        (StatusCode::OK, Some("About"))
    );
    let p = call(&app, "GET", "/api/v1/public/site", None, &[]).await;
    assert!(p.body["nav_pages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["slug"] == slug.as_str()));
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/site/pages/{id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", &pub_uri, None, &[]).await.status,
        StatusCode::NOT_FOUND
    );
}
