use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn general_settings_maintenance_and_custom_code() {
    let (app, st) = app().await;
    // Shared test DB: start from defaults so a previous failed run cannot leak state.
    pelty_database::general_settings::save(&st.pool, &Default::default())
        .await
        .unwrap();
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let cust = sess(&app, &st, "customer").await;

    let g = call(
        &app,
        "GET",
        "/api/v1/admin/general-settings",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let mut s = g.body["settings"].clone();
    assert_eq!(s["localisation"]["date_format"], "DD/MM/YYYY");
    s["system_urls"]["system_url"] = json!("http://not-https");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/general-settings",
            Some(s.clone()),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    s["system_urls"]["system_url"] = json!("https://billing.example");
    s["localisation"]["date_format"] = json!("YYYY-MM-DD");
    s["maintenance"] = json!({"enabled": true, "message": "Back at 6", "redirect_url": ""});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/general-settings",
            Some(s.clone()),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/general-settings",
        Some(s.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let p = call(&app, "GET", "/api/v1/public/general", None, &[]).await;
    assert_eq!(p.body["maintenance"]["enabled"], true);
    assert_eq!(p.body["localisation"]["date_format"], "YYYY-MM-DD");

    // Maintenance blocks checkout with 503 and the custom message; reads still work.
    let c = call(
        &app,
        "POST",
        "/api/v1/client/checkout",
        Some(json!({})),
        &cust.h(),
    )
    .await;
    assert_eq!(c.status, StatusCode::SERVICE_UNAVAILABLE, "{:?}", c.body);
    assert_eq!(c.body["error"]["message"], "Back at 6");
    assert_eq!(
        call(&app, "GET", "/api/v1/public/site", None, &[])
            .await
            .status,
        StatusCode::OK
    );
    s["maintenance"]["enabled"] = json!(false);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/general-settings",
            Some(s),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_ne!(
        call(
            &app,
            "POST",
            "/api/v1/client/checkout",
            Some(json!({})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::SERVICE_UNAVAILABLE
    );

    // Custom CSS/JS: admin role only (not staff), validated, exposed publicly.
    let code = json!({"css": ".x{color:red}", "head_js": "", "footer_js": "console.log(1)"});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/custom-code",
            Some(code.clone()),
            &staff.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/custom-code",
            Some(json!({"css": "</style><script>"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/custom-code",
            Some(code),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let p = call(&app, "GET", "/api/v1/public/general", None, &[]).await;
    assert_eq!(p.body["custom_code"]["css"], ".x{color:red}");
    let _ = call(
        &app,
        "PUT",
        "/api/v1/admin/custom-code",
        Some(json!({})),
        &admin.h(),
    )
    .await;
}
