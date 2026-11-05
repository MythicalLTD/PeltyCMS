use crate::common;
use axum::http::StatusCode;
use common::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::sync::Arc;

fn dec(v: &Value) -> Decimal {
    v.as_str().unwrap_or("0").parse().unwrap()
}

async fn pay(app: &axum::Router, admin: &Sess, iid: &str, amount: &Value) {
    let r = call(app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"), Some(json!({"amount": amount, "idempotency_key": format!("k-{iid}"), "reference": format!("dx-{iid}")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

async fn invoice(app: &axum::Router, admin: &Sess, iid: &str) -> Value {
    let r = call(
        app,
        "GET",
        &format!("/api/v1/admin/invoices/{iid}"),
        None,
        &admin.h(),
    )
    .await;
    if r.body["invoice"].is_null() {
        r.body
    } else {
        r.body["invoice"].clone()
    }
}

#[tokio::test]
async fn registrar_addons_pricing_dns_lifecycle_bulk_public() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let tld = format!("x{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);

    // Registrars are addons: the catalog lists them with a settings schema.
    let r = call(&app, "GET", "/api/v1/admin/addons", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let s = r.body.to_string();
    for id in [
        "registrar-namecheap",
        "registrar-resellerclub",
        "registrar-enom",
        "registrar-opensrs",
        "registrar-godaddy",
        "registrar-centralnic",
        "registrar-generic",
        "registrar-mock",
    ] {
        assert!(s.contains(id), "missing {id}");
    }
    let r = call(&app, "GET", "/api/v1/admin/registrars", None, &admin.h()).await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == "godaddy"));

    // TLD with add-on prices and late fees.
    let base = json!({"tld": tld, "currency": "EUR", "register_price": "10.00", "renew_price": "12.00", "transfer_price": "8.00", "min_years": 1, "max_years": 5,
        "registrar": "mock", "active": true, "grace_days": 10, "grace_fee": "5.00", "redemption_days": 20, "redemption_fee": "80.00",
        "id_protection_price": "2.50", "dns_management_price": "1.00", "epp_required": true, "tld_group": "hot", "sort_order": 1});
    let mut bad = base.clone();
    bad["grace_fee"] = json!("-1");
    assert_eq!(
        call(&app, "PUT", "/api/v1/admin/tlds", Some(bad), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut bad = base.clone();
    bad["tld_group"] = json!("weird");
    assert_eq!(
        call(&app, "PUT", "/api/v1/admin/tlds", Some(bad), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/tlds",
        Some(base.clone()),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(dec(&r.body["redemption_fee"]), Decimal::new(80, 0));

    // Copy to USD at 1.1, then +10% on USD only.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/tlds/copy-currency",
        Some(json!({"from": "EUR", "to": "USD", "rate": "1.1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/tlds/bulk-adjust",
        Some(json!({"tlds": [tld], "currency": "USD", "percent": "10"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["updated"], 1, "{:?}", r.body);
    let r = call(&app, "GET", "/api/v1/admin/tlds", None, &admin.h()).await;
    let usd = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["tld"] == tld.as_str() && t["currency"] == "USD")
        .unwrap()
        .clone();
    assert_eq!(dec(&usd["register_price"]), Decimal::new(1210, 2)); // 10 × 1.1 × 1.1
    assert_eq!(dec(&usd["id_protection_price"]), Decimal::new(275, 2));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/tlds/bulk-adjust",
            Some(json!({"percent": "-100"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Public storefront search (no session) with suggestions.
    let label = format!("pub{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/domains/check?name={label}.{tld}&currency=EUR&suggest=true"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["supported"], true);
    assert_eq!(r.body["results"][0]["available"], true);
    assert_eq!(r.body["results"][0]["pricing"]["tld_group"], "hot");
    let r = call(
        &app,
        "GET",
        "/api/v1/public/domains/tlds?currency=usd",
        None,
        &[],
    )
    .await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["currency"] == "USD"));

    // Order with ID protection + DNS management: 2y × (10 + 2.50 + 1.00) = 27.00.
    let name = format!("{label}.{tld}");
    let contact = json!({"name": "Ana Pop", "email": "ana@example.test", "country": "RO"});
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": name, "kind": "register", "years": 2, "contact": contact, "privacy": true, "dns_management": true})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let did = r.body["domain"]["id"].as_str().unwrap().to_string();
    let iid = r.body["invoice_id"].as_str().unwrap().to_string();
    let inv = invoice(&app, &admin, &iid).await;
    assert_eq!(dec(&inv["subtotal"]), Decimal::new(2700, 2), "{inv:?}");
    pay(&app, &admin, &iid, &inv["total"]).await;
    pelty_api::domains::process(&state).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "active");
    assert_eq!(r.body["domain"]["dns_management"], true);

    // DNS records: validation, replace, read back.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/domains/{did}/dns"),
        Some(json!({"records": [{"rtype": "A", "host": "www", "value": "999.1.1.1"}]})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_dns_record");
    let recs = json!({"records": [{"rtype": "A", "host": "@", "value": "203.0.113.5"}, {"rtype": "mx", "host": "", "value": "mx.mail.test", "priority": 5}, {"rtype": "TXT", "host": "@", "value": "v=spf1 -all"}]});
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/domains/{did}/dns"),
        Some(recs),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["items"].as_array().unwrap().len(), 3);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains/{did}/dns"),
        None,
        &admin.h(),
    )
    .await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["rtype"] == "MX" && x["priority"] == 5));
    // Turning DNS management off locks the zone editor.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/domains/{did}/addons"),
        Some(json!({"dns_management": false, "registrar_lock": false})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["registrar_lock"], false);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/domains/{did}/dns"),
            None,
            &cust.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Lifecycle: expired 5 days ago → grace; renewal adds the grace fee.
    let today = time::OffsetDateTime::now_utc().date();
    sqlx::query(
        "UPDATE domains SET expires_on = $2, auto_renew = false, synced_at = now() WHERE id = $1",
    )
    .bind(uuid::Uuid::parse_str(&did).unwrap())
    .bind(today - time::Duration::days(5))
    .execute(&state.pool)
    .await
    .unwrap();
    pelty_api::domains::daily(&state, today).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "grace");
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/domains/{did}/renew"),
        Some(json!({"years": 1})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = invoice(&app, &admin, r.body["invoice_id"].as_str().unwrap()).await;
    // 12 renew + 2.50 ID protection + 5 grace fee (DNS management now off).
    assert_eq!(dec(&inv["subtotal"]), Decimal::new(1950, 2), "{inv:?}");

    // 15 days past (grace 10) → redemption; 31 → expired.
    sqlx::query("UPDATE domains SET expires_on = $2, synced_at = now() WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&did).unwrap())
        .bind(today - time::Duration::days(15))
        .execute(&state.pool)
        .await
        .unwrap();
    pelty_api::domains::daily(&state, today).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains/{did}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "redemption");
    sqlx::query("UPDATE domains SET expires_on = $2, synced_at = now() WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&did).unwrap())
        .bind(today - time::Duration::days(31))
        .execute(&state.pool)
        .await
        .unwrap();
    pelty_api::domains::daily(&state, today).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains/{did}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "expired");

    // Admin bulk: status back to active + lock; client bulk nameservers.
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/domains/bulk",
        Some(json!({"ids": [did], "action": "status", "status": "active"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ok"], 1, "{:?}", r.body);
    sqlx::query("UPDATE domains SET expires_on = $2, synced_at = now() WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&did).unwrap())
        .bind(today + time::Duration::days(300))
        .execute(&state.pool)
        .await
        .unwrap();
    let r = call(&app, "POST", "/api/v1/client/domains/bulk", Some(json!({"ids": [did], "action": "nameservers", "nameservers": ["ns1.bulk.test", "ns2.bulk.test"]})), &cust.h()).await;
    assert_eq!(r.body["ok"], 1, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains/bulk",
        Some(json!({"ids": [did], "action": "status", "status": "cancelled"})),
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["failed"], 1,
        "clients can't set status: {:?}",
        r.body
    );
    let other = sess(&app, &state, "customer").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains/bulk",
        Some(json!({"ids": [did], "action": "auto_renew", "value": true})),
        &other.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/domains/{did}"),
        Some(json!({"admin_notes": "VIP", "registrar_lock": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["admin_notes"], "VIP");

    // Registrar addon in use can't be uninstalled; an unconfigured real
    // registrar refuses to work (no network call is made).
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/addons/registrar-mock",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let mut gd = base.clone();
    gd["tld"] = json!(format!("g{}", &tld[1..]));
    gd["registrar"] = json!("godaddy");
    assert_eq!(
        call(&app, "PUT", "/api/v1/admin/tlds", Some(gd), &admin.h())
            .await
            .status,
        StatusCode::OK
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/check?name=abc.g{}", &tld[1..]),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["error"]["code"], "error.registrar_unavailable",
        "{:?}",
        r.body
    );
}

#[tokio::test]
async fn whois_uses_rdap_transport() {
    let (_, mut state) = app().await;
    let t = pelty_provisioning::registrars::MockRegTransport::default();
    t.push(404, "");
    t.push(200, r#"{"ldhName":"example.com","status":["active"],"events":[{"eventAction":"expiration","eventDate":"2030-01-02T00:00:00Z"}]}"#);
    state.registrars = Arc::new(
        pelty_provisioning::registrar::RegistrarRegistry::with_transport(Arc::new(t.clone())),
    );
    common::registrars(&state.registrars);
    let app = pelty_api::router(state.clone());
    let r = call(
        &app,
        "GET",
        "/api/v1/public/domains/whois?name=free-x.com",
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["whois"]["registered"], false);
    let r = call(
        &app,
        "GET",
        "/api/v1/public/domains/whois?name=Example.com",
        None,
        &[],
    )
    .await;
    assert_eq!(r.body["whois"]["expires"], "2030-01-02");
    assert!(t.requests()[1].url.ends_with("/domain/example.com"));
    let r = call(
        &app,
        "GET",
        "/api/v1/public/domains/whois?name=nodot",
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}
