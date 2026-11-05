use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn tracked_links_redirect_and_count() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let (ah, ch) = (admin.h(), cust.h());
    let slug = format!("promo-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let body = json!({"slug": slug.to_uppercase(), "name": "Promo", "target_url": "https://example.com/landing?a=1"});

    assert_eq!(call(&app, "POST", "/api/v1/admin/tracked-links", Some(body.clone()), &ch).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", "/api/v1/admin/tracked-links", Some(json!({"slug": slug, "name": "x", "target_url": "javascript:alert(1)"})), &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/tracked-links", Some(body.clone()), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "POST", "/api/v1/admin/tracked-links", Some(body), &ah).await.status, StatusCode::CONFLICT);

    // Anonymous clicks redirect and are counted.
    for _ in 0..2 {
        assert_eq!(call(&app, "GET", &format!("/api/v1/go/{slug}"), None, &[]).await.status, StatusCode::SEE_OTHER);
    }
    assert_eq!(call(&app, "GET", "/api/v1/go/does-not-exist-zz", None, &[]).await.status, StatusCode::NOT_FOUND);
    let loc: String = sqlx::query_scalar("SELECT target_url FROM tracked_links WHERE id = $1::uuid").bind(&id).fetch_one(&st.pool).await.unwrap();
    assert_eq!(loc, "https://example.com/landing?a=1");
    let r = call(&app, "GET", "/api/v1/admin/tracked-links", None, &ah).await;
    let row = r.body["items"].as_array().unwrap().iter().find(|l| l["id"] == id.as_str()).unwrap().clone();
    assert_eq!(row["clicks"], 2);
    assert_eq!(row["slug"], slug.as_str());
    assert!(row["last_click_at"].is_string());

    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/tracked-links/{id}"), None, &ah).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", &format!("/api/v1/go/{slug}"), None, &[]).await.status, StatusCode::NOT_FOUND);
}
