use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn kb_publish_search_votes_and_permissions() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/kb/categories", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/kb/categories",
        Some(json!({"slug": "Bad Slug", "name": "x"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_slug");
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/kb/categories",
        Some(json!({"slug": format!("c-{tag}"), "name": "Getting started"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cat = r.body["id"].as_str().unwrap().to_string();

    let slug = format!("reset-{tag}");
    let r = call(&app, "POST", "/api/v1/admin/kb/articles", Some(json!({"category_id": cat, "slug": slug, "title": format!("Reset password {tag}"), "body": "Click forgot zebrafish<script>", "locale": "xx"})), &admin.h()).await;
    assert_eq!(r.body["error"]["code"], "error.unknown_locale");
    let r = call(&app, "POST", "/api/v1/admin/kb/articles", Some(json!({"category_id": cat, "slug": slug, "title": format!("Reset password {tag}"), "body": "Click forgot zebrafish<script>"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let aid = r.body["article"]["id"].as_str().unwrap().to_string();
    // Duplicate slug.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/kb/articles",
        Some(json!({"category_id": cat, "slug": slug, "title": "t", "body": "b"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    // Drafts are invisible publicly.
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/public/kb/articles/{slug}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/kb?q={tag}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.body["articles"].as_array().unwrap().len(), 0);

    let r = call(&app, "PUT", &format!("/api/v1/admin/kb/articles/{aid}"), Some(json!({"category_id": cat, "slug": slug, "title": format!("Reset password {tag}"), "body": "Click forgot zebrafish<script>", "is_published": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Ticket-time suggestions match any word (OR), not all of them.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/kb/suggest?q=help%20me%20{tag}%20nothingmatcheshere"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.body["articles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["slug"] == slug.as_str()),
        "{:?}",
        r.body
    );
    let r = call(&app, "GET", "/api/v1/public/kb/suggest?q=a%20b", None, &[]).await;
    assert_eq!(r.body["articles"], json!([]));

    // Public search (prefix) without login; body returned verbatim (UI renders as text).
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/kb?q={}", &tag[..5]),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["articles"][0]["slug"], slug.as_str());
    assert!(r.body["categories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == cat.as_str()));
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/kb/articles/{slug}"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.body["article"]["body"], "Click forgot zebrafish<script>");
    assert_eq!(r.body["article"]["views"], 1);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/public/kb/articles/{slug}/vote"),
            Some(json!({"helpful": true})),
            &[]
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/kb/articles/{aid}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["article"]["helpful_yes"], 1);

    // Category with articles can't be deleted.
    let r = call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/kb/categories/{cat}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.category_not_empty");
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/kb/articles/{aid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/kb/categories/{cat}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn announcements_publish_schedule_and_locale() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let staff_less = sess(&app, &state, "customer").await;
    let tag = uuid::Uuid::new_v4().simple().to_string();

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/announcements",
            Some(json!({"title": "x", "body": "y"})),
            &staff_less.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(json!({"title": "  ", "body": "y"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_title");

    let mk = |title: String, extra: serde_json::Value| {
        let mut b = json!({"title": title, "body": "Maintenance tonight"});
        b.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        b
    };
    let live = call(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(mk(format!("live-{tag}"), json!({"is_published": true}))),
        &admin.h(),
    )
    .await;
    assert_eq!(live.status, StatusCode::OK, "{:?}", live.body);
    let live_id = live.body["announcement"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(mk(format!("draft-{tag}"), json!({}))),
        &admin.h(),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(mk(
            format!("future-{tag}"),
            json!({"is_published": true, "published_at": "2099-01-01T00:00:00Z"}),
        )),
        &admin.h(),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(mk(
            format!("ro-{tag}"),
            json!({"is_published": true, "locale": "ro"}),
        )),
        &admin.h(),
    )
    .await;

    let titles = |v: &serde_json::Value| -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["title"].as_str().unwrap().to_string())
            .filter(|t| t.ends_with(&tag))
            .collect()
    };
    let en = call(
        &app,
        "GET",
        "/api/v1/public/announcements?locale=en&per_page=100",
        None,
        &[],
    )
    .await;
    assert_eq!(titles(&en.body), vec![format!("live-{tag}")]);
    let ro = titles(
        &call(
            &app,
            "GET",
            "/api/v1/public/announcements?locale=ro&per_page=100",
            None,
            &[],
        )
        .await
        .body,
    );
    assert!(
        ro.contains(&format!("ro-{tag}")) && ro.contains(&format!("live-{tag}")) && ro.len() == 2
    );
    assert_eq!(
        titles(
            &call(
                &app,
                "GET",
                "/api/v1/admin/announcements?per_page=100",
                None,
                &admin.h()
            )
            .await
            .body
        )
        .len(),
        4
    );

    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/public/announcements/{live_id}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/announcements/{live_id}"),
        Some(mk(format!("live-{tag}"), json!({"is_published": false}))),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/public/announcements/{live_id}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/announcements/{live_id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
}
