use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String, String) {
    let r = app.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let ct = r.headers().get("content-type").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    let st = r.status();
    let b = r.into_body().collect().await.unwrap().to_bytes();
    (st, ct, String::from_utf8(b.to_vec()).unwrap())
}

#[tokio::test]
async fn product_and_tld_feeds() {
    let (app, st) = app().await;
    let tag = Uuid::new_v4().simple().to_string()[..8].to_string();
    let (gid, pid, hid) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    sqlx::query("INSERT INTO product_groups (id, name, slug) VALUES ($1, 'G', $2)").bind(gid).bind(format!("feed-g-{tag}")).execute(&st.pool).await.unwrap();
    sqlx::query("INSERT INTO products (id, group_id, name, slug, description) VALUES ($1,$2,'Web <Pro>',$3,'Fast \"SSD\" hosting')")
        .bind(pid).bind(gid).bind(format!("feed-{tag}")).execute(&st.pool).await.unwrap();
    sqlx::query("INSERT INTO products (id, group_id, name, slug, status) VALUES ($1,$2,'Hidden',$3,'hidden')")
        .bind(hid).bind(gid).bind(format!("feed-h-{tag}")).execute(&st.pool).await.unwrap();
    sqlx::query("INSERT INTO product_prices (id, product_id, currency, cycle, amount) VALUES ($1,$2,'EUR','monthly',9.9), ($3,$2,'EUR','annual',99)")
        .bind(Uuid::new_v4()).bind(pid).bind(Uuid::new_v4()).execute(&st.pool).await.unwrap();

    let base = format!("/api/v1/feeds/products/feed-{tag}");
    let (s, ct, body) = get(&app, &base).await;
    assert_eq!(s, StatusCode::OK);
    assert!(ct.starts_with("application/javascript"));
    assert_eq!(body, "document.write(\"9.90 EUR\");\n");
    assert_eq!(get(&app, &format!("{base}?cycle=annual")).await.2, "document.write(\"99.00 EUR\");\n");
    assert_eq!(get(&app, &format!("{base}?field=name")).await.2, "document.write(\"Web \\u003cPro\\u003e\");\n");
    assert_eq!(get(&app, &format!("{base}?field=description")).await.2, "document.write(\"Fast \\\"SSD\\\" hosting\");\n");
    assert_eq!(get(&app, &format!("{base}?currency=USD")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, &format!("{base}?currency=eu1")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(get(&app, &format!("{base}?field=module_config")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(get(&app, &format!("/api/v1/feeds/products/feed-h-{tag}")).await.0, StatusCode::NOT_FOUND);

    let tld = format!("f{tag}");
    sqlx::query("INSERT INTO tld_pricing (tld, currency, register_price, renew_price, transfer_price) VALUES ($1,'EUR',12.5,14,10)").bind(&tld).execute(&st.pool).await.unwrap();
    assert_eq!(get(&app, &format!("/api/v1/feeds/tlds/.{tld}")).await.2, "document.write(\"12.50 EUR\");\n");
    assert_eq!(get(&app, "/api/v1/feeds/tlds/nope-zz").await.0, StatusCode::NOT_FOUND);
}
