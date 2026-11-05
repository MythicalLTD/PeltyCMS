//! Custom per-TLD domain fields reach the registrar addon on registration.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;

#[tokio::test]
async fn custom_fields_sent_to_registrar() {
    let (_, mut state) = app().await;
    let t = pelty_provisioning::registrars::MockRegTransport::default();
    state.registrars = Arc::new(
        pelty_provisioning::registrar::RegistrarRegistry::with_transport(Arc::new(t.clone())),
    );
    common::registrars(&state.registrars);
    let app = pelty_api::router(state.clone());
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/addons/registrar-generic/settings",
        Some(json!({"api_base": "https://reg.example/api"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tld = format!("f{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let r = call(&app, "PUT", "/api/v1/admin/tlds", Some(json!({"tld": tld, "currency": "EUR", "register_price": "10.00", "renew_price": "10.00", "transfer_price": "10.00", "min_years": 1, "max_years": 2, "registrar": "generic", "active": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // Keys differing only in case are one field.
    let dup = json!([{"tld": tld, "key": "RegistrantNexus", "label": "Nexus", "kind": "text"}, {"tld": tld, "key": "registrantnexus", "label": "Nexus 2", "kind": "text"}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/domains/tld-fields/{tld}"),
            Some(dup),
            &admin.h()
        )
        .await
        .body["error"]["code"],
        "error.tld_field_invalid"
    );
    // Case-sensitive registrar parameter name is kept end to end.
    let f = json!([{"tld": tld, "key": "RegistrantNexus", "label": "Nexus", "kind": "text", "required": true}]);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/domains/tld-fields/{tld}"),
            Some(f),
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let name = format!("x{}.{tld}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    t.push(200, r#"{"available":true}"#);
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": name, "kind": "register", "contact": {"name": "Ana Pop", "email": "ana@example.test", "country": "RO"}, "extra_fields": {"RegistrantNexus": "C11"}})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let did = r.body["domain"]["id"].as_str().unwrap().to_string();
    let iid = r.body["invoice_id"].as_str().unwrap().to_string();
    let inv = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{iid}"),
        None,
        &admin.h(),
    )
    .await
    .body;
    let total = if inv["invoice"].is_null() {
        inv["total"].clone()
    } else {
        inv["invoice"]["total"].clone()
    };
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"), Some(json!({"amount": total, "idempotency_key": format!("k-{iid}"), "reference": format!("df-{iid}")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    t.push(201, r#"{"ok":true}"#);
    pelty_api::domains::process(&state).await.unwrap();
    let reqs = t.requests();
    let q = reqs
        .iter()
        .find(|q| q.url == "https://reg.example/api/domains")
        .expect("register call");
    let b: Value = serde_json::from_str(&q.body).unwrap();
    assert_eq!(b["domain"], name.as_str());
    assert_eq!(b["fields"], json!({"RegistrantNexus": "C11"}));
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "active", "{:?}", r.body);
}
