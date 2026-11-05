use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn default_nameservers_and_dns_template_apply_to_orders() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let tld = format!("d{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let r = call(&app, "PUT", "/api/v1/admin/tlds", Some(json!({"tld": tld, "currency": "EUR", "register_price": "10.00", "renew_price": "10.00", "transfer_price": "10.00",
        "min_years": 1, "max_years": 5, "registrar": "mock", "active": true, "dns_management_price": "0"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/domain-defaults", Some(json!({"nameservers": ["ns1.only.test"]})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/settings/domain-defaults", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "PUT", "/api/v1/admin/settings/domain-defaults", Some(json!({"nameservers": ["NS1.Def.test", "ns2.def.test"],
        "dns_template": [{"rtype": "cname", "host": "www", "value": "{domain}"}, {"rtype": "A", "host": "@", "value": "192.0.2.10"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["nameservers"][0], "ns1.def.test");

    let contact = json!({"name": "Ana Pop", "email": "ana@example.test", "country": "RO"});
    let name = format!("shop{}.{tld}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": name, "kind": "register", "contact": contact, "dns_management": true})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let did = r.body["domain"]["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["domain"]["nameservers"], json!(["ns1.def.test", "ns2.def.test"]));
    let d = call(&app, "GET", &format!("/api/v1/client/domains/{did}/dns"), None, &cust.h()).await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    let s = d.body.to_string();
    assert!(s.contains(&format!("\"value\":\"{name}\"")) && s.contains("192.0.2.10"), "{s}");

    // Explicit nameservers win.
    let name2 = format!("own{}.{tld}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": name2, "kind": "register", "contact": contact, "nameservers": ["a.mine.test", "b.mine.test"]})), &cust.h()).await;
    assert_eq!(r.body["domain"]["nameservers"], json!(["a.mine.test", "b.mine.test"]));
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/domain-defaults", Some(json!({})), &admin.h()).await.status, StatusCode::OK);
}
