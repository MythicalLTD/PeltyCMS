use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn addons_and_custom_fields_priced_validated_and_copied_to_service() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;

    // Admin-only management.
    let body = json!({"name": "Backups", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "3.00", "setup_fee": "1.00"}], "product_ids": [pid]});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/catalog/addons",
            Some(body.clone()),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let a = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/addons",
        Some(body),
        &admin.h(),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    let backups = a.body["id"].as_str().unwrap().to_string();
    let ip = call(&app, "POST", "/api/v1/admin/catalog/addons", Some(json!({"name": "Dedicated IP", "prices": [{"currency": "EUR", "cycle": "one_time", "amount": "2.00"}], "product_ids": [pid]})), &admin.h()).await;
    let ipid = ip.body["id"].as_str().unwrap().to_string();
    let other = call(&app, "POST", "/api/v1/admin/catalog/addons", Some(json!({"name": "Unlinked", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "1"}]})), &admin.h()).await;
    let unlinked = other.body["id"].as_str().unwrap().to_string();

    let fields_uri = format!("/api/v1/admin/catalog/products/{pid}/fields");
    let ah = admin.h();
    macro_rules! f {
        ($b:expr) => {
            call(&app, "POST", &fields_uri, Some($b), &ah)
        };
    }
    let host = f!(
        json!({"name": "Hostname", "kind": "text", "required": true, "pattern": "^[a-z0-9.-]+$"})
    )
    .await;
    assert_eq!(host.status, StatusCode::OK, "{:?}", host.body);
    let host = host.body["id"].as_str().unwrap().to_string();
    let os = f!(json!({"name": "OS", "kind": "select", "choices": ["Debian", "Ubuntu"]}))
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let secret = f!(json!({"name": "Rack", "kind": "text", "admin_only": true}))
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        f!(json!({"name": "Bad", "kind": "text", "pattern": "(["}))
            .await
            .body["error"]["code"],
        "error.invalid_field_pattern"
    );
    assert_eq!(
        f!(json!({"name": "Bad", "kind": "select"})).await.body["error"]["code"],
        "error.invalid_field_choices"
    );

    // Public extras hide admin-only fields and product links.
    let ex = call(
        &app,
        "GET",
        &format!("/api/v1/catalog/products/{pid}/extras"),
        None,
        &[],
    )
    .await;
    assert_eq!(ex.body["addons"].as_array().unwrap().len(), 2);
    assert!(ex.body["fields"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| x["id"] != secret.as_str()));
    assert!(ex.body["addons"][0]["product_ids"]
        .as_array()
        .unwrap()
        .is_empty());

    let ch0 = ch[0].clone();
    let bh = alice.h();
    macro_rules! place { ($a:expr, $f:expr) => { call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [
            {"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch0}, "addons": $a, "fields": $f}
        ]})), &bh) }; }
    let code = |r: Resp| r.body["error"]["code"].as_str().unwrap_or("").to_string();
    assert_eq!(
        code(place!(json!([]), json!({})).await),
        "error.field_required"
    );
    assert_eq!(
        code(place!(json!([]), json!({host.clone(): "Bad Host!"})).await),
        "error.invalid_field"
    );
    assert_eq!(
        code(
            place!(
                json!([]),
                json!({host.clone(): "vps1", os.clone(): "Windows"})
            )
            .await
        ),
        "error.invalid_field"
    );
    assert_eq!(
        code(
            place!(
                json!([]),
                json!({host.clone(): "vps1", secret.clone(): "R1"})
            )
            .await
        ),
        "error.invalid_field"
    );
    assert_eq!(
        code(place!(json!([unlinked]), json!({host.clone(): "vps1"})).await),
        "error.invalid_addon"
    );
    assert_eq!(
        code(place!(json!([backups, backups]), json!({host.clone(): "vps1"})).await),
        "error.invalid_addon"
    );

    // 10 + 5 setup + backups 3 + 1 setup + IP 2 one-time = 21.
    let r = place!(
        json!([backups, ipid]),
        json!({host.clone(): "vps1.example.com", os.clone(): "Debian"})
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "21.0000");
    let oid2 = r.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{oid2}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(ap.status, StatusCode::OK, "{:?}", ap.body);
    let sid = ap.body["services"][0]
        .as_str()
        .or(ap.body["service_ids"][0].as_str())
        .expect("sid")
        .to_string();
    let svc = call(
        &app,
        "GET",
        &format!("/api/v1/client/services/{sid}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(
        svc.body["recurring_amount"], "13.0000",
        "base + recurring addon: {:?}",
        svc.body
    );
    assert_eq!(svc.body["addons"].as_array().unwrap().len(), 2);
    assert_eq!(svc.body["custom_fields"][&host], "vps1.example.com");
}
