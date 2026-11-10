//! Digital products: versions, expiring links, license issue/validate with
//! domain binding and activation limits, lifecycle (suspend/terminate) and
//! the escrow asset grant. Everything is 404 while the module is off.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn raw(
    app: &axum::Router,
    s: &Sess,
    method: &str,
    uri: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/octet-stream");
    for (k, v) in s.h() {
        b = b.header(k, v);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let st = res.status();
    (
        st,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

async fn check(app: &axum::Router, body: Value) -> Value {
    let r = call_from(
        app,
        "203.0.113.7",
        "POST",
        "/api/v1/public/licenses/validate",
        Some(body),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

async fn my_license(app: &axum::Router, s: &Sess) -> Value {
    for _ in 0..100 {
        let r = call(app, "GET", "/api/v1/client/digital/licenses", None, &s.h()).await;
        if let Some(l) = r.body["items"].as_array().and_then(|a| a.first()) {
            return l.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("license was not issued");
}

async fn status_of(app: &axum::Router, s: &Sess, want: &str) -> bool {
    for _ in 0..100 {
        let l = my_license(app, s).await;
        if l["license"]["status"] == want {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn versions_links_and_licenses() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/public/licenses/validate",
            Some(json!({"key": "x"})),
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/digital/products",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["digital_products"]).await;

    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let bad = json!({"license_enabled": true, "max_activations": 1, "bind_domain": true, "bind_ip": false, "bind_hwid": false, "link_ttl_secs": 5});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/digital/products/{pid}"),
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(&app, "PUT", &format!("/api/v1/admin/digital/products/{pid}"), Some(json!({
        "license_enabled": true, "max_activations": 1, "bind_domain": true, "bind_ip": false, "bind_hwid": false,
        "license_days": null, "link_ttl_secs": 120, "escrow_asset": "cfx-asset-42"
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let file = b"PK\x03\x04 resource bytes".to_vec();
    let (s1, b1) = raw(&app, &admin, "POST", &format!("/api/v1/admin/digital/products/{pid}/versions?version=1.0.0&filename=res.zip&changelog=First%20release"), file.clone()).await;
    assert_eq!(s1, StatusCode::OK, "{}", String::from_utf8_lossy(&b1));
    let (s2, _) = raw(
        &app,
        &admin,
        "POST",
        &format!("/api/v1/admin/digital/products/{pid}/versions?version=1.0.0&filename=res.zip"),
        file.clone(),
    )
    .await;
    assert_eq!(s2, StatusCode::CONFLICT);
    let (s3, _) = raw(
        &app,
        &admin,
        "POST",
        &format!("/api/v1/admin/digital/products/{pid}/versions?version=..%2Fx&filename=res.zip"),
        file.clone(),
    )
    .await;
    assert_eq!(s3, StatusCode::UNPROCESSABLE_ENTITY);
    let cl = call(
        &app,
        "GET",
        &format!("/api/v1/public/digital/products/{pid}/changelog"),
        None,
        &[],
    )
    .await;
    assert_eq!(cl.body["items"][0]["changelog"], "First release");

    // Purchase → license issued.
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[0]}}]})), &alice.h()).await;
    let order_id = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let sid = ap.body["services"][0].as_str().unwrap().to_string();
    let lic = my_license(&app, &alice).await;
    let key = lic["key"].as_str().unwrap().to_string();
    assert_eq!(key.len(), 29);
    assert_eq!(lic["license"]["max_activations"], 1);

    // Validation: binding, activation limit, re-check, deactivate.
    assert_eq!(
        check(&app, json!({"key": key})).await["reason"],
        "domain_required"
    );
    let v = check(
        &app,
        json!({"key": key, "domain": "https://www.shop-a.example/x", "product_id": pid}),
    )
    .await;
    assert_eq!(v["valid"], true, "{v:?}");
    assert_eq!(v["activations"], 1);
    assert_eq!(
        check(
            &app,
            json!({"key": key.to_lowercase(), "domain": "shop-a.example"})
        )
        .await["valid"],
        true
    );
    assert_eq!(
        check(&app, json!({"key": key, "domain": "shop-b.example"})).await["reason"],
        "activation_limit"
    );
    assert_eq!(
        check(
            &app,
            json!({"key": key, "domain": "shop-b.example", "activate": false})
        )
        .await["reason"],
        "not_activated"
    );
    assert_eq!(
        check(
            &app,
            json!({"key": key, "domain": "shop-a.example", "product_id": uuid::Uuid::new_v4()})
        )
        .await["reason"],
        "wrong_product"
    );
    assert_eq!(
        check(&app, json!({"key": "AAAAA-BBBBB-CCCCC-DDDDD-EEEEE"})).await["reason"],
        "not_found"
    );
    let d = call_from(
        &app,
        "203.0.113.7",
        "POST",
        "/api/v1/public/licenses/deactivate",
        Some(json!({"key": key, "domain": "shop-a.example"})),
        &[],
    )
    .await;
    assert_eq!(d.body["removed"], true);
    assert_eq!(
        check(&app, json!({"key": key, "domain": "shop-b.example"})).await["valid"],
        true
    );
    // Customer self-service reset frees the slot.
    let lid = lic["license"]["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/digital/licenses/{lid}/reset"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/digital/licenses/{lid}/reset"),
            None,
            &alice.h()
        )
        .await
        .body["removed"],
        1
    );

    // Downloads: owned product lists the version; links work for the owner only.
    let mine = call(
        &app,
        "GET",
        "/api/v1/client/digital/products",
        None,
        &alice.h(),
    )
    .await;
    let vid = mine.body["items"][0]["versions"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/digital/products",
            None,
            &bob.h()
        )
        .await
        .body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/digital/versions/{vid}/link"),
            None,
            &bob.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let link = call(
        &app,
        "POST",
        &format!("/api/v1/client/digital/versions/{vid}/link"),
        None,
        &alice.h(),
    )
    .await;
    let url = link.body["url"].as_str().unwrap().to_string();
    let (ds, data) = raw(&app, &bob, "GET", &url, vec![]).await;
    assert_eq!(ds, StatusCode::OK);
    assert_eq!(data, file);
    for _ in 0..4 {
        raw(&app, &bob, "GET", &url, vec![]).await;
    }
    assert_eq!(
        raw(&app, &bob, "GET", &url, vec![]).await.0,
        StatusCode::NOT_FOUND,
        "link use limit"
    );
    assert_eq!(
        raw(
            &app,
            &bob,
            "GET",
            "/api/v1/public/digital/download/nope",
            vec![]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Escrow grant recorded for the purchase.
    let g = call(
        &app,
        "GET",
        "/api/v1/admin/digital/grants",
        None,
        &admin.h(),
    )
    .await;
    let grant = g.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["service_id"] == sid.as_str())
        .cloned()
        .expect("grant");
    assert_eq!(grant["asset"], "cfx-asset-42");
    assert_eq!(grant["status"], "pending");

    // Lifecycle: suspend → suspended, unsuspend → active, terminate → revoked.
    st.emit(DomainEvent::new(
        "service.suspended",
        json!({"service_id": sid}),
    ));
    assert!(status_of(&app, &alice, "suspended").await);
    assert_eq!(
        check(&app, json!({"key": key, "domain": "shop-b.example"})).await["reason"],
        "suspended"
    );
    st.emit(DomainEvent::new(
        "service.unsuspended",
        json!({"service_id": sid}),
    ));
    assert!(status_of(&app, &alice, "active").await);
    st.emit(DomainEvent::new(
        "service.terminated",
        json!({"service_id": sid}),
    ));
    assert!(status_of(&app, &alice, "revoked").await);
    assert_eq!(
        check(&app, json!({"key": key, "domain": "shop-b.example"})).await["reason"],
        "revoked"
    );
    let g = call(
        &app,
        "GET",
        "/api/v1/admin/digital/grants",
        None,
        &admin.h(),
    )
    .await;
    assert!(g.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["service_id"] == sid.as_str() && x["status"] == "revoked"));

    // Staff issue a license by hand; the module off hides everything again.
    let cust = call(
        &app,
        "GET",
        "/api/v1/client/digital/licenses",
        None,
        &alice.h(),
    )
    .await
    .body["items"][0]["license"]["customer_id"]
        .clone();
    let m = call(
        &app,
        "POST",
        "/api/v1/admin/digital/licenses",
        Some(json!({"product_id": pid, "customer_id": cust, "days": 30})),
        &admin.h(),
    )
    .await;
    assert_eq!(m.status, StatusCode::OK, "{:?}", m.body);
    let v = check(
        &app,
        json!({"key": m.body["key"], "domain": "shop-c.example"}),
    )
    .await;
    assert_eq!(v["valid"], true);
    assert!(v["expires_at"].is_string());
    call(
        &app,
        "PUT",
        "/api/v1/admin/modules/digital_products",
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/client/digital/licenses",
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
