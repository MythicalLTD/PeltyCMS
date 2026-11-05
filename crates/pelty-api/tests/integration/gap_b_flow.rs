//! Debit notes, UBL e-invoice export, headless commerce API and the
//! OpenAPI document.
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn get_raw(app: &axum::Router, uri: &str, h: &[(&str, &str)]) -> (StatusCode, String, String) {
    let mut b = Request::builder().uri(uri);
    for (k, v) in h {
        b = b.header(*k, *v);
    }
    let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    let ct = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    (res.status(), ct, String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).to_string())
}

async fn set_buyer(app: &axum::Router, c: &common::Sess) {
    let body = json!({"organization": "Acme GmbH", "country": "DE", "city": "Berlin", "address_line1": "Str 1", "first_name": "Al"});
    let r = call(app, "PUT", "/api/v1/client/profile", Some(body), &c.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

async fn order(app: &axum::Router, c: &Sess, pid: &str, oid: &str, ch: &str) -> Value {
    let r = call(app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch}}]})), &c.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

#[tokio::test]
async fn debit_notes_and_ubl_export() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    set_buyer(&app, &alice).await;
    let o = order(&app, &alice, &pid, &oid, &ch[0]).await;
    let iid = o["invoice"]["id"].as_str().unwrap().to_string();
    let inv_no = o["invoice"]["number"].as_str().unwrap().to_string();

    // Validation + permissions.
    let path = format!("/api/v1/admin/invoices/{iid}/debit-note");
    assert_eq!(call(&app, "POST", &path, Some(json!({"lines": []})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let neg = json!({"lines": [{"description": "x", "quantity": "1", "unit_price": "-1"}]});
    assert_eq!(call(&app, "POST", &path, Some(neg), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let body = json!({"lines": [{"description": "Extra bandwidth", "quantity": "2", "unit_price": "1.25"}], "reason": "Overage October"});
    assert_eq!(call(&app, "POST", &path, Some(body.clone()), &alice.h()).await.status, StatusCode::FORBIDDEN);

    let r = call(&app, "POST", &path, Some(body.clone()), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["kind"], "debit_note");
    assert_eq!(r.body["status"], "unpaid");
    assert_eq!(r.body["credit_for"], iid.as_str());
    assert_eq!(r.body["total"], "2.5000");
    let dn = r.body["id"].as_str().unwrap().to_string();
    assert!(r.body["number"].as_str().unwrap().starts_with("DN-"), "{:?}", r.body["number"]);

    // Linked both ways and visible to the owner only.
    let d = call(&app, "GET", &format!("/api/v1/admin/invoices/{iid}"), None, &admin.h()).await;
    assert_eq!(d.body["debit_notes"].as_array().unwrap().len(), 1);
    let d = call(&app, "GET", &format!("/api/v1/client/invoices/{dn}"), None, &alice.h()).await;
    assert_eq!(d.status, StatusCode::OK);
    assert_eq!(d.body["related"]["number"], inv_no.as_str());
    assert_eq!(d.body["note_reason"], "Overage October");
    assert_eq!(call(&app, "GET", &format!("/api/v1/client/invoices/{dn}"), None, &bob.h()).await.status, StatusCode::NOT_FOUND);

    // Payable like an invoice (manual payment settles it).
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{dn}/payments"), Some(json!({"amount": "2.50", "idempotency_key": format!("dn-{dn}")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = call(&app, "GET", &format!("/api/v1/admin/invoices/{dn}"), None, &admin.h()).await;
    assert_eq!(d.body["invoice"]["status"], "paid");
    // A debit note can't itself be debited.
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/invoices/{dn}/debit-note"), Some(body), &admin.h()).await.status, StatusCode::CONFLICT);

    // PDF says "Debit note".
    let (s, ct, _) = get_raw(&app, &format!("/api/v1/client/invoices/{dn}/pdf"), &alice.h()).await;
    assert_eq!((s, ct.as_str()), (StatusCode::OK, "application/pdf"));

    // UBL: settings validation, then export.
    let bad = call(&app, "PUT", "/api/v1/admin/settings/einvoice", Some(json!({"profile": "nope"})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/einvoice", Some(json!({"profile": "peppol"})), &alice.h()).await.status, StatusCode::FORBIDDEN);
    let cfg = json!({"profile": "cius_ro", "seller_name": "Mythical SRL", "seller_vat_id": "RO123", "seller_street": "Str X", "seller_city": "SECTOR1",
        "seller_subdivision": "RO-B", "seller_country": "ro", "iban": "RO49 AAAA 1B31 0075 9384 0000"});
    let r = call(&app, "PUT", "/api/v1/admin/settings/einvoice", Some(cfg), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["seller_country"], "RO");
    assert_eq!(r.body["iban"], "RO49AAAA1B31007593840000");

    let (s, ct, xml) = get_raw(&app, &format!("/api/v1/client/invoices/{iid}/ubl"), &alice.h()).await;
    assert_eq!(s, StatusCode::OK, "{xml}");
    assert!(ct.starts_with("application/xml"));
    assert!(xml.contains("urn:efactura.mfinante.ro:CIUS-RO:1.0.1"));
    assert!(xml.contains(&format!("<cbc:ID>{inv_no}</cbc:ID>")));
    assert!(xml.contains("<cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>"));
    assert!(xml.contains("<cbc:RegistrationName>Acme GmbH</cbc:RegistrationName>"));
    // 10.00 + 5.00 setup, untaxed in the default config.
    assert!(xml.contains("<cbc:PayableAmount currencyID=\"EUR\">15.00</cbc:PayableAmount>"), "{xml}");
    let (s, _, xml) = get_raw(&app, &format!("/api/v1/admin/invoices/{dn}/ubl"), &admin.h()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(xml.contains("<cbc:InvoiceTypeCode>383</cbc:InvoiceTypeCode>"));
    assert!(xml.contains(&format!("<cac:InvoiceDocumentReference><cbc:ID>{inv_no}</cbc:ID>")));
    assert!(xml.contains("<cbc:PrepaidAmount currencyID=\"EUR\">2.50</cbc:PrepaidAmount><cbc:PayableAmount currencyID=\"EUR\">0.00</cbc:PayableAmount>"));
    assert_eq!(get_raw(&app, &format!("/api/v1/client/invoices/{iid}/ubl"), &bob.h()).await.0, StatusCode::NOT_FOUND);

    // A buyer without a country can't be exported (EN 16931 BT-55).
    let o2 = order(&app, &bob, &pid, &oid, &ch[0]).await;
    let (s, _, _) = get_raw(&app, &format!("/api/v1/client/invoices/{}/ubl", o2["invoice"]["id"].as_str().unwrap()), &bob.h()).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn headless_purchase_flow_with_api_key() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let carol = sess(&app, &st, "customer").await;
    let dave = sess(&app, &st, "customer").await;

    let key = |s: Sess| {
        let app = app.clone();
        async move {
            let r = call(&app, "POST", "/api/v1/api-keys", Some(json!({"name": "shop", "scopes": ["client.access"]})), &s.h()).await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            format!("Bearer {}", r.body["secret"].as_str().unwrap())
        }
    };
    // Interactive sessions are refused on the headless surface.
    assert_eq!(call(&app, "GET", "/api/v1/headless/catalog", None, &carol.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/headless/catalog", None, &[]).await.status, StatusCode::UNAUTHORIZED);
    let kc = key(carol).await;
    let kd = key(dave).await;
    let hc = [("authorization", kc.as_str())];
    let hd = [("authorization", kd.as_str())];

    let c = call(&app, "GET", "/api/v1/headless/catalog", None, &hc).await;
    assert_eq!(c.status, StatusCode::OK, "{:?}", c.body);
    assert!(c.body["products"].as_array().unwrap().iter().any(|p| p.to_string().contains(&pid)));
    assert!(c.body["currencies"].is_array());

    // Cart: price comes from the server, never from the client.
    let item = json!({"product_id": pid, "cycle": "monthly", "config": {oid.clone(): ch[1]}});
    let r = call(&app, "POST", "/api/v1/headless/carts", Some(json!({"currency": "EUR", "items": [item.clone()]})), &hc).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let cart = r.body["cart"]["id"].as_str().unwrap().to_string();
    assert!(r.body["preview"].to_string().contains("17.5"), "{:?}", r.body["preview"]);
    let r = call(&app, "PUT", &format!("/api/v1/headless/carts/{cart}"), Some(json!({"currency": "EUR", "items": [item.clone(), item]})), &hc).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // Another customer's key can't see or check out this cart.
    assert_eq!(call(&app, "GET", &format!("/api/v1/headless/carts/{cart}"), None, &hd).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "POST", &format!("/api/v1/headless/carts/{cart}/checkout"), None, &hd).await.status, StatusCode::NOT_FOUND);

    let r = call(&app, "POST", &format!("/api/v1/headless/carts/{cart}/checkout"), None, &hc).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "35.0000");
    let iid = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let oid2 = r.body["order"]["id"].as_str().unwrap().to_string();
    // Replay is refused.
    assert_eq!(call(&app, "POST", &format!("/api/v1/headless/carts/{cart}/checkout"), None, &hc).await.status, StatusCode::CONFLICT);

    let r = call(&app, "POST", &format!("/api/v1/headless/invoices/{iid}/pay"), Some(json!({"gateway": "manual"})), &hc).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", &format!("/api/v1/headless/invoices/{iid}/pay"), Some(json!({"gateway": "manual"})), &hd).await.status, StatusCode::NOT_FOUND);

    let r = call(&app, "GET", &format!("/api/v1/headless/orders/{oid2}"), None, &hc).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["order"]["status"], "pending");
    assert_eq!(r.body["invoice"]["status"], "unpaid");
    assert_eq!(call(&app, "GET", &format!("/api/v1/headless/orders/{oid2}"), None, &hd).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_document_served_at_both_paths() {
    let (app, _st) = app().await;
    for p in ["/api/openapi.json", "/api/v1/openapi.json"] {
        let r = call(&app, "GET", p, None, &[]).await;
        assert_eq!(r.status, StatusCode::OK, "{p}");
        let paths = &r.body["paths"];
        for k in ["/api/v1/headless/catalog", "/api/v1/headless/carts", "/api/v1/headless/carts/{id}/checkout", "/api/v1/headless/invoices/{id}/pay",
            "/api/v1/headless/orders/{id}", "/api/v1/admin/invoices/{id}/debit-note", "/api/v1/client/invoices/{id}/ubl"] {
            assert!(paths.get(k).is_some(), "{k} missing in {p}");
        }
        assert_eq!(r.body["components"]["securitySchemes"]["api_key"]["scheme"], "bearer");
    }
}
