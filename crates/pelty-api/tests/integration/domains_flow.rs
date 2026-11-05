use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn pay(app: &axum::Router, admin: &Sess, iid: &str, amount: &Value) {
    let r = call(app, "POST", &format!("/api/v1/admin/invoices/{iid}/payments"), Some(json!({"amount": amount, "idempotency_key": format!("k-{iid}"), "reference": format!("dom-{iid}")})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

#[tokio::test]
async fn domains_end_to_end() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let cust = sess(&app, &state, "customer").await;
    let other = sess(&app, &state, "customer").await;
    let tld = format!("t{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/tlds", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", "/api/v1/admin/registrars", None, &admin.h()).await;
    assert!(
        r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "mock" && i["enabled"] == true),
        "{:?}",
        r.body
    );
    // Validation: unknown registrar, negative price.
    let base = json!({"tld": tld, "currency": "EUR", "register_price": "10.99", "renew_price": "12.50", "transfer_price": "8.00", "min_years": 1, "max_years": 5, "registrar": "mock", "active": true});
    let mut bad = base.clone();
    bad["registrar"] = json!("nope");
    assert_eq!(
        call(&app, "PUT", "/api/v1/admin/tlds", Some(bad), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut bad = base.clone();
    bad["renew_price"] = json!("-1");
    assert_eq!(
        call(&app, "PUT", "/api/v1/admin/tlds", Some(bad), &admin.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for cur in ["EUR", "USD", "RON"] {
        let mut b = base.clone();
        b["currency"] = json!(cur);
        let r = call(&app, "PUT", "/api/v1/admin/tlds", Some(b), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        assert_eq!(r.body["register_price"], "10.9900");
    }

    let r = call(&app, "GET", "/api/v1/client/domains/tlds", None, &cust.h()).await;
    assert!(r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["tld"] == tld.as_str()));

    let name = format!(
        "shop-{}.{tld}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/check?name={}", name.to_uppercase()),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["available"], true, "{:?}", r.body);
    assert_eq!(r.body["name"], name.as_str());
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/check?name=taken.{tld}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["available"], false);
    let r = call(
        &app,
        "GET",
        "/api/v1/client/domains/check?name=x.unknowntld",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["supported"], false);
    let r = call(
        &app,
        "GET",
        "/api/v1/client/domains/check?name=bad_name",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_domain");

    let contact = json!({"name": "Ana Pop", "email": "ana@example.test", "country": "RO"});
    // Bad inputs.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": name, "kind": "register", "years": 9, "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_years");
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": name, "kind": "register", "nameservers": ["ns1.only.test"], "contact": contact})), &cust.h()).await;
    assert_eq!(r.body["error"]["code"], "error.nameserver_count");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": name, "kind": "register", "contact": {"name": "", "email": "x"}})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.invalid_contact");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": format!("taken.{tld}"), "kind": "register", "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    // Register 2 years: exact price 2 × 10.99 (before tax).
    let r = call(&app, "POST", "/api/v1/client/domains", Some(json!({"name": name, "kind": "register", "years": 2, "nameservers": ["ns1.host.test", "ns2.host.test"], "contact": contact})), &cust.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let did = r.body["domain"]["id"].as_str().unwrap().to_string();
    let iid = r.body["invoice_id"].as_str().unwrap().to_string();
    assert_eq!(r.body["domain"]["status"], "pending");
    let inv = call(
        &app,
        "GET",
        &format!("/api/v1/admin/invoices/{iid}"),
        None,
        &admin.h(),
    )
    .await;
    let sub = inv.body["invoice"]["subtotal"]
        .as_str()
        .or(inv.body["subtotal"].as_str())
        .unwrap()
        .to_string();
    assert_eq!(
        sub.parse::<rust_decimal::Decimal>().unwrap(),
        "21.98".parse().unwrap(),
        "{:?}",
        inv.body
    );
    let total = inv.body["invoice"]["total"].clone();
    let total = if total.is_null() {
        inv.body["total"].clone()
    } else {
        total
    };
    // Same name twice is a conflict; other customers can't see it.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": name, "kind": "register", "contact": contact})),
        &other.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/client/domains/{did}"),
            None,
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Unpaid: processing does nothing.
    pelty_api::domains::process(&state).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "pending");
    pay(&app, &admin, &iid, &total).await;
    let st = pelty_api::domains::process(&state).await.unwrap();
    assert!(st.registered >= 1);
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "active");
    let today = time::OffsetDateTime::now_utc().date();
    assert_eq!(
        r.body["domain"]["expires_on"],
        pelty_database::domains::add_years(today, 2).to_string()
    );

    // Nameservers / contact / auto-renew.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/client/domains/{did}/nameservers"),
        Some(json!({"nameservers": ["NS1.new.test", "ns2.new.test", "ns3.new.test"]})),
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["domain"]["nameservers"],
        json!(["ns1.new.test", "ns2.new.test", "ns3.new.test"])
    );
    let r = call(&app, "PUT", &format!("/api/v1/client/domains/{did}/contact"), Some(json!({"contact": {"name": "Ana P", "email": "ana2@example.test", "city": "Cluj"}, "privacy": true})), &cust.h()).await;
    assert_eq!(r.body["domain"]["contact"]["city"], "Cluj");
    assert_eq!(r.body["domain"]["privacy"], true);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/client/domains/{did}/nameservers"),
            Some(json!({"nameservers": []})),
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // EPP code: owner only, audited, stable for the mock registrar.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/domains/{did}/epp"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["code"].as_str().unwrap().starts_with("MOCK-"));
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/client/domains/{did}/epp"),
            None,
            &other.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Manual renewal: one invoice per term.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/domains/{did}/renew"),
        Some(json!({"years": 1})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rinv = r.body["invoice_id"].as_str().unwrap().to_string();
    let rtotal = r.body["total"].clone();
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/client/domains/{did}/renew"),
        Some(json!({"years": 1})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    pay(&app, &admin, &rinv, &rtotal).await;
    pelty_api::domains::process(&state).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(
        r.body["domain"]["expires_on"],
        pelty_database::domains::add_years(today, 3).to_string()
    );
    assert_eq!(r.body["renewals"][0]["applied"], true);

    // Expiry soon: daily pass raises an auto-renew invoice and one reminder.
    let soon = today + time::Duration::days(5);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/domains/{did}"),
        Some(json!({"expires_on": soon.to_string()})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let st = pelty_api::domains::daily(&state, today).await.unwrap();
    assert!(st.renewal_invoices >= 1 && st.reminders >= 1, "{st:?}");
    let again = pelty_api::domains::daily(&state, today).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{did}"),
        None,
        &cust.h(),
    )
    .await;
    let pending: Vec<_> = r.body["renewals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["applied"] == false)
        .collect();
    assert_eq!(pending.len(), 1, "auto-renew invoice raised once");
    let _ = again;
    let mails = mails_to(&state, &cust.email).await;
    assert_eq!(
        mails.iter().filter(|m| m.text.contains(&name)).count(),
        1,
        "one reminder"
    );
    // Lapsed domains enter the grace period (then redemption, then expired).
    call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/domains/{did}"),
        Some(json!({"expires_on": (today - time::Duration::days(1)).to_string()})),
        &admin.h(),
    )
    .await;
    pelty_api::domains::daily(&state, today).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains/{did}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "grace");
    // Sync pulls the registrar's view back.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/domains/{did}/sync"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "active", "{:?}", r.body);

    // Transfer: EPP code required, sealed, bad code fails the order.
    let tname = format!(
        "move-{}.{tld}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": tname, "kind": "transfer", "contact": contact})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.auth_code_required");
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(json!({"name": tname, "kind": "transfer", "contact": contact, "auth_code": "BAD"})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["domain"]["id"].as_str().unwrap().to_string();
    assert!(
        r.body["total"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap()
            >= "8.00".parse().unwrap()
    );
    pay(
        &app,
        &admin,
        r.body["invoice_id"].as_str().unwrap(),
        &r.body["total"],
    )
    .await;
    pelty_api::domains::process(&state).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains/{tid}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "failed");
    assert!(r.body["domain"]["last_error"]
        .as_str()
        .unwrap()
        .contains("authorization"));
    // Failed orders free the name.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/domains",
        Some(
            json!({"name": tname, "kind": "transfer", "contact": contact, "auth_code": "GOOD-123"}),
        ),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let tid = r.body["domain"]["id"].as_str().unwrap().to_string();
    pay(
        &app,
        &admin,
        r.body["invoice_id"].as_str().unwrap(),
        &r.body["total"],
    )
    .await;
    pelty_api::domains::process(&state).await.unwrap();
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/client/domains/{tid}"),
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.body["domain"]["status"], "active");
    assert!(r.body["domain"].get("auth_code_sealed").is_none());

    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/domains?q={tld}"),
        None,
        &admin.h(),
    )
    .await;
    assert!(r.body["total"].as_i64().unwrap() >= 3);
    let r = call(&app, "GET", "/api/v1/client/domains", None, &other.h()).await;
    assert_eq!(r.body["total"], 0);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/tlds/{tld}/ron"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
}
