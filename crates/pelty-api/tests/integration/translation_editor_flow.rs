use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn translation_editor() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let key = "updates.never";
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/translations?lang=ro",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/translations?lang=xx",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/translations?lang=ro&q={key}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let row = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == key)
        .unwrap()
        .clone();
    assert_eq!(row["base"], "Never");
    assert_eq!(row["file"], "Niciodată");
    assert!(row["override"].is_null());

    // Validation: unknown key, empty, too long, new placeholder.
    for (body, code) in [
        (
            json!({"lang": "ro", "key": "no.such.key", "value": "x"}),
            "error.unknown_translation_key",
        ),
        (
            json!({"lang": "ro", "key": key, "value": "  "}),
            "error.translation_value_invalid",
        ),
        (
            json!({"lang": "ro", "key": key, "value": "x".repeat(4001)}),
            "error.translation_value_invalid",
        ),
        (
            json!({"lang": "ro", "key": key, "value": "Hi {{password}}"}),
            "error.translation_placeholder",
        ),
        (
            json!({"lang": "ro", "key": key, "value": "Hi {password}"}),
            "error.translation_placeholder",
        ),
        // Same name, other syntax: the original's syntax is kept.
        (
            json!({"lang": "ro", "key": "updates.available", "value": "Versiunea {{version}}"}),
            "error.translation_placeholder",
        ),
    ] {
        let r = call(
            &app,
            "PUT",
            "/api/v1/admin/translations",
            Some(body),
            &admin.h(),
        )
        .await;
        assert_eq!(r.body["error"]["code"], code, "{:?}", r.body);
    }
    // Existing placeholders are allowed.
    let r = call(&app, "PUT", "/api/v1/admin/translations", Some(json!({"lang": "ro", "key": "updates.available", "value": "Versiunea {version} e gata"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/translations",
        Some(json!({"lang": "ro", "key": key, "value": "Încă nu"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["effective"], "Încă nu");
    // Server-side catalog and the UI catalog both see it; English untouched.
    assert_eq!(state.i18n.t("ro", key), "Încă nu");
    assert_eq!(state.i18n.t("en", key), "Never");
    let r = call(&app, "GET", "/api/v1/i18n/ro", None, &[]).await;
    assert_eq!(r.body[key], "Încă nu");
    assert_eq!(r.body["updates.available"], "Versiunea {version} e gata");
    let r = call(&app, "GET", "/api/v1/i18n/en", None, &[]).await;
    assert_eq!(r.body[key], "Never");
    let r = call(
        &app,
        "GET",
        "/api/v1/admin/translations?lang=ro&filter=overridden&per_page=200",
        None,
        &admin.h(),
    )
    .await;
    let keys: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["key"].as_str().unwrap())
        .collect();
    assert!(
        keys.contains(&key) && keys.contains(&"updates.available"),
        "{keys:?}"
    );
    // A fresh process picks it up on reload.
    let (_app2, state2) = common::app().await;
    assert_eq!(state2.i18n.t("ro", key), "Niciodată");
    pelty_api::translations::reload(&state2).await.unwrap();
    assert_eq!(state2.i18n.t("ro", key), "Încă nu");

    // Revert.
    for k in [key, "updates.available"] {
        let r = call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/translations?lang=ro&key={k}"),
            None,
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    assert_eq!(state.i18n.t("ro", key), "Niciodată");
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/translations?lang=ro&key={key}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = call(&app, "GET", "/api/v1/i18n/ro", None, &[]).await;
    assert_eq!(r.body[key], "Niciodată");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'translation.override_%' AND target = $1").bind(format!("ro:{key}")).fetch_one(&state.pool).await.unwrap();
    assert!(n >= 2);
}
