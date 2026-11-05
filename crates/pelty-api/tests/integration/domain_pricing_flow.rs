//! Registrar price sync + margins, premium names, per-TLD fields (own
//! binary: touches global premium settings and the `com`/`net` rows).
use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn dec(v: &Value) -> Decimal {
    Decimal::from_str(
        v.as_str()
            .unwrap_or_else(|| panic!("not a decimal string: {v:?}")),
    )
    .unwrap()
}
fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

#[tokio::test]
async fn price_sync_premium_and_fields() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;

    // --- margin rules + sync ---
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/domains/margins",
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad =
        json!([{"tld": "*", "percent": "-5", "fixed": "0", "min_margin": "0", "rounding": "cent"}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/domains/margins",
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let rules = json!([{"tld": "*", "percent": "20", "fixed": "0", "min_margin": "0", "rounding": "cent"},
                       {"tld": "net", "percent": "20", "fixed": "0", "min_margin": "0", "rounding": "whole"}]);
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/domains/margins",
        Some(rules),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body.as_array().unwrap().len(), 2);
    for tld in ["com", "net"] {
        let b = json!({"tld": tld, "currency": "EUR", "register_price": "1.00", "renew_price": "1.00", "transfer_price": "1.00", "min_years": 1, "max_years": 5, "registrar": "mock", "active": true});
        assert_eq!(
            call(&app, "PUT", "/api/v1/admin/tlds", Some(b), &admin.h())
                .await
                .status,
            StatusCode::OK
        );
    }
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/domains/sync-prices",
        Some(json!({"registrar": "mock", "dry_run": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["updated"], 2);
    let com = r.body["lines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["tld"] == "com")
        .unwrap()
        .clone();
    assert_eq!(dec(&com["new_register"]), d("10.20"));
    assert_eq!(dec(&com["new_renew"]), d("10.92"));
    let org = r.body["lines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["tld"] == "org")
        .unwrap()
        .clone();
    assert_eq!(org["action"], "not_listed");
    // Dry run wrote nothing.
    let t = pelty_database::domains::tld(&state.pool, "com", "EUR")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t.register_price, d("1.00"));
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/domains/sync-prices",
        Some(json!({"registrar": "mock"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let t = pelty_database::domains::tld(&state.pool, "com", "EUR")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (t.register_price, t.renew_price, t.transfer_price),
        (d("10.20"), d("10.92"), d("10.20"))
    );
    let t = pelty_database::domains::tld(&state.pool, "net", "EUR")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (t.register_price, t.renew_price, t.transfer_price),
        (d("12"), d("12"), d("11"))
    );
    let r = call(&app, "GET", "/api/v1/admin/domains/costs", None, &admin.h()).await;
    let c = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["tld"] == "com")
        .unwrap()
        .clone();
    assert_eq!(dec(&c["cost_renew"]), d("9.10"));
    // Second run: nothing changes.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/domains/sync-prices",
        Some(json!({"registrar": "mock"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["updated"], 0, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/domains/sync-prices",
        Some(json!({"registrar": "nope"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // --- premium names ---
    let tld = format!("p{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    for cur in ["EUR", "USD", "RON"] {
        let b = json!({"tld": tld, "currency": cur, "register_price": "10.00", "renew_price": "12.00", "transfer_price": "8.00", "min_years": 1, "max_years": 5, "registrar": "mock", "active": true});
        assert_eq!(
            call(&app, "PUT", "/api/v1/admin/tlds", Some(b), &admin.h())
                .await
                .status,
            StatusCode::OK
        );
    }
    let contact = json!({"name": "Ana Pop", "email": "ana@example.test", "country": "RO"});
    let pname = format!(
        "premium{}.{tld}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    // Disabled by default: lookup says unavailable with a reason; order refused.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/check?name={pname}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["available"], false, "{:?}", r.body);
    assert_eq!(r.body["premium_error"], "error.domain_premium_disabled");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": pname, "kind": "register", "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.domain_premium_disabled",
        "{:?}",
        r.body
    );
    let bad = json!({"enabled": true, "markup_percent": "-1", "max_price": "0"});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/domains/premium-settings",
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/domains/premium-settings",
        Some(json!({"enabled": true, "markup_percent": "10", "max_price": "0"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/check?name={pname}"),
        None,
        &cust.h(),
    )
    .await;
    let cur = r.body["pricing"]["currency"].as_str().unwrap().to_string();
    assert_eq!(
        cur, "EUR",
        "mock quotes EUR; test customer currency must be EUR"
    );
    assert_eq!(r.body["available"], true, "{:?}", r.body);
    assert_eq!(dec(&r.body["premium"]["register_price"]), d("275.00"));
    assert_eq!(dec(&r.body["premium"]["renew_price"]), d("33.00"));
    // Standard names stay standard.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/check?name=plain.{tld}"),
        None,
        &cust.h(),
    )
    .await;
    assert!(r.body["premium"].is_null(), "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": pname, "kind": "register", "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["domain"]["premium"], true);
    let did = uuid::Uuid::parse_str(r.body["domain"]["id"].as_str().unwrap()).unwrap();
    let iid = uuid::Uuid::parse_str(r.body["invoice_id"].as_str().unwrap()).unwrap();
    let items = pelty_database::invoices::items(&state.pool, iid)
        .await
        .unwrap();
    assert_eq!(items[0].unit_price, d("275.00"));
    // Renewals bill the stored premium renewal price, not the TLD price.
    sqlx::query(
        "UPDATE domains SET status = 'active', expires_on = CURRENT_DATE + 20 WHERE id = $1",
    )
    .bind(did)
    .execute(&state.pool)
    .await
    .unwrap();
    let dom = pelty_database::domains::get(&state.pool, did)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dom.premium_renew_price, Some(d("33.00")));
    let t = pelty_database::domains::tld(&state.pool, &tld, "EUR")
        .await
        .unwrap()
        .unwrap();
    let inv = pelty_database::domains::create_renewal(&state.pool, &dom, &t, 1)
        .await
        .unwrap()
        .unwrap();
    let items = pelty_database::invoices::items(&state.pool, inv.id)
        .await
        .unwrap();
    assert_eq!(items[0].unit_price, d("33.00"));
    // Price cap.
    call(
        &app,
        "PUT",
        "/api/v1/admin/domains/premium-settings",
        Some(json!({"enabled": true, "markup_percent": "10", "max_price": "100"})),
        &admin.h(),
    )
    .await;
    let p2 = format!(
        "premium{}.{tld}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": p2, "kind": "register", "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.domain_premium_too_expensive",
        "{:?}",
        r.body
    );
    call(
        &app,
        "PUT",
        "/api/v1/admin/domains/premium-settings",
        Some(json!({"enabled": false, "markup_percent": "0", "max_price": "0"})),
        &admin.h(),
    )
    .await;

    // --- per-TLD additional fields ---
    let bad = json!([{"tld": tld, "key": "Bad Key", "label": "x", "kind": "text"}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/domains/tld-fields/{tld}"),
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let fields = json!([{"tld": tld, "key": "cnp", "label": "CNP", "kind": "text", "required": true, "max_length": 13},
                        {"tld": tld, "key": "entity", "label": "Entity", "kind": "select", "options": ["person", "company"]}]);
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/domains/tld-fields/{tld}"),
        Some(fields),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/fields?tld={tld}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body.as_array().unwrap().len(), 2);
    let n3 = format!(
        "field{}.{tld}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": n3, "kind": "register", "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.domain_field_required");
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": n3, "kind": "register", "contact": contact, "extra_fields": {"cnp": "123", "entity": "alien"}})), &cust.h()).await;
    assert_eq!(r.body["error"]["code"], "error.domain_field_invalid");
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": n3, "kind": "register", "contact": contact, "extra_fields": {"cnp": "1234567890123", "entity": "company", "x": 1}})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["domain"]["extra_fields"],
        json!({"cnp": "1234567890123", "entity": "company"})
    );
    assert_eq!(r.body["domain"]["premium"], false);
}
