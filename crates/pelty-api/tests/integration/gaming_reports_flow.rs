//! Gaming reports in the report library: hidden and 404 while their module
//! is off; once on, a paid order for a product sold on a game server shows
//! up under that server, and every gaming report runs as JSON and CSV.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const GAMING: [&str; 4] = [
    "game_server_sales",
    "game_deliveries",
    "creator_codes",
    "marketplace_sales",
];

async fn library_keys(app: &axum::Router, admin: &Sess) -> Vec<String> {
    let r = call(
        app,
        "GET",
        "/api/v1/admin/reports/library",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["key"].as_str().unwrap().to_string())
        .collect()
}

pub(crate) fn run_path(key: &str, extra: &str) -> String {
    let now = time::OffsetDateTime::now_utc().date();
    let m = format!("{}-{:02}", now.year(), u8::from(now.month()));
    format!("/api/v1/admin/reports/library/{key}?from={m}&to={m}{extra}")
}

#[tokio::test]
async fn gaming_reports_follow_their_modules() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let buyer = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];

    let keys = library_keys(&app, &admin).await;
    for k in GAMING {
        assert!(!keys.iter().any(|x| x == k), "{k} listed while off");
        let r = call(&app, "GET", &run_path(k, ""), None, &admin.h()).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{k}");
    }
    // Core reports are unaffected.
    assert!(keys.iter().any(|x| x == "income_by_product"));

    enable(
        &app,
        &admin,
        &[
            "game_profiles",
            "game_delivery",
            "creator_codes",
            "digital_products",
            "creator_marketplace",
        ],
    )
    .await;
    let keys = library_keys(&app, &admin).await;
    for k in GAMING {
        assert!(keys.iter().any(|x| x == k), "{k} missing while on");
    }

    // A paid order for a product sold on a game server.
    let srv = call(
        &app,
        "POST",
        "/api/v1/admin/game/servers",
        Some(json!({"game": "minecraft_java", "name": format!("Survival {tag}"), "slug": format!("surv-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(srv.status, StatusCode::OK, "{:?}", srv.body);
    let srv = srv.body["id"].as_str().unwrap().to_string();
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/game/servers/{srv}/products"),
        Some(json!({"product_ids": [pid]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &buyer.h()).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let inv = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{inv}/payments"),
        Some(json!({"amount": total, "idempotency_key": uuid::Uuid::new_v4().to_string()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let r = call(
        &app,
        "GET",
        &run_path("game_server_sales", ""),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let row: &Value = r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[0] == format!("Survival {tag}").as_str())
        .unwrap_or_else(|| panic!("server row missing: {:?}", r.body));
    assert_eq!(row[1], "EUR");
    assert_eq!(row[3], "1");
    // seed_product: 10 monthly + 5 setup.
    assert_eq!(row[2].as_str().unwrap().parse::<f64>().unwrap(), 15.0);

    for k in GAMING {
        let r = call(&app, "GET", &run_path(k, ""), None, &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{k}: {:?}", r.body);
        assert_eq!(r.body["key"], k);
        let cols = r.body["columns"].as_array().unwrap().len();
        for row in r.body["rows"].as_array().unwrap() {
            assert_eq!(row.as_array().unwrap().len(), cols, "{k}");
        }
        let req = Request::builder()
            .uri(run_path(k, "&format=csv"))
            .header("cookie", &admin.cookie)
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{k}");
        assert!(res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/csv"));
        let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap();
        let first = r.body["columns"][0].as_str().unwrap();
        assert!(
            body.starts_with(&format!("\u{feff}{first},")),
            "{k}: {body}"
        );
    }

    reset_modules(&st).await;
    let r = call(
        &app,
        "GET",
        &run_path("game_server_sales", "&format=csv"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}
