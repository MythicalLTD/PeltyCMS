use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn admin_gateway_rules_crud_validation_and_permissions() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let url = "/api/v1/admin/gateways/manual/rules";
    assert_eq!(
        call(&app, "GET", url, None, &cust.h()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/gateways/nope/rules",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // Limits only for a currency no other test uses, so parallel tests aren't affected.
    let r = call(&app, "PUT", url, Some(json!({"fee_percent": "0", "currencies": [], "limits": {"isk": {"min": "100", "max": null, "fee_fixed": "5"}}})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["limits"]["ISK"]["min"], "100");
    let g = call(&app, "GET", url, None, &admin.h()).await;
    assert_eq!(g.body["limits"]["ISK"]["fee_fixed"], "5");
    let bad = call(&app, "PUT", url, Some(json!({"fee_percent": "0", "currencies": [], "limits": {"ISK": {"min": "9", "max": "1", "fee_fixed": "0"}}})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        call(
            &app,
            "PUT",
            url,
            Some(json!({"fee_percent": "-1"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let rules = pelty_database::gateway_rules::get(&st.pool, "manual")
        .await
        .unwrap();
    assert_eq!(
        rules.check("ISK", "99".parse().unwrap()),
        Err("error.gateway_below_min")
    );
    assert!(rules.check("EUR", "1".parse().unwrap()).is_ok());
}
