use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn plugin_ui_settings_routes_and_locales() {
    let (app, state) = app().await;
    let (adm, csrf, _) = login_new(&app, &state, "admin").await;
    let h = [("cookie", adm.as_str()), ("x-csrf-token", csrf.as_str())];
    let (cust, _, _) = login_new(&app, &state, "customer").await;

    // Module routes are 404 while the plugin is disabled (in-memory toggle).
    state.plugins.set_enabled("event-logger", false);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/plugins/event-logger/summary",
            None,
            &h
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let ui = call(&app, "GET", "/api/v1/plugins/ui", None, &h).await;
    assert!(ui.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["plugin"] != "event-logger"));

    state.plugins.set_enabled("event-logger", true);
    let r = call(
        &app,
        "GET",
        "/api/v1/plugins/event-logger/summary",
        None,
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["title"], "plugin.event-logger.widget");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/plugins/event-logger/summary",
            None,
            &[("cookie", &cust)]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // UI entries are filtered by permission.
    let ui = call(&app, "GET", "/api/v1/plugins/ui", None, &h).await;
    let mine: Vec<_> = ui.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["plugin"] == "event-logger")
        .collect();
    assert_eq!(mine.len(), 2);
    let ui = call(
        &app,
        "GET",
        "/api/v1/plugins/ui",
        None,
        &[("cookie", &cust)],
    )
    .await;
    assert!(ui.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["plugin"] != "event-logger"));

    // Settings: schema exposed, values validated.
    let g = call(
        &app,
        "GET",
        "/api/v1/admin/plugins/event-logger/settings",
        None,
        &h,
    )
    .await;
    assert_eq!(g.body["schema"]["properties"]["keep"]["type"], "integer");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/plugins/event-logger/settings",
            Some(json!({"keep": "lots"})),
            &h
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/plugins/event-logger/settings",
            Some(json!({"evil": 1})),
            &h
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/plugins/event-logger/settings",
            Some(json!({"keep": 50})),
            &h
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/plugins/event-logger/settings",
            None,
            &h
        )
        .await
        .body["values"]["keep"],
        50
    );

    // Plugin strings are merged into their namespace for every language.
    let en = call(&app, "GET", "/api/v1/i18n/en", None, &[]).await;
    let ro = call(&app, "GET", "/api/v1/i18n/ro", None, &[]).await;
    assert_eq!(en.body["plugin.event-logger.menu"], "Event log");
    assert_eq!(ro.body["plugin.event-logger.menu"], "Jurnal evenimente");
    assert_eq!(
        ro.body["plugin.example-webhook.settings.channel"],
        "Canal de notificări"
    );
}
