//! Creator codes: off → 404 and codes ignored at checkout; affiliates pick a
//! code, admins set its discount; the cart preview and the first invoice are
//! discounted; paying credits the creator; own, unknown and disabled codes
//! are refused.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

fn dec(v: &Value) -> String {
    let t = v.as_str().unwrap_or("?");
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t.to_string()
    }
}

#[tokio::test]
async fn creator_code_discounts_and_credits_the_creator() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let creator = sess(&app, &st, "customer").await;
    let rival = sess(&app, &st, "customer").await;
    let buyer = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    for p in ["/api/v1/admin/creator-codes", "/api/v1/client/creator-code"] {
        assert_eq!(
            call(&app, "GET", p, None, &admin.h()).await.status,
            StatusCode::NOT_FOUND,
            "{p}"
        );
    }

    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": format!("Cc {tag}"), "slug": format!("cc-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let gid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": gid, "name": format!("Vip {tag}"), "slug": format!("vip-{tag}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "10.00"}]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let order = |h: Vec<(&'static str, String)>, code: &str| {
        let app = app.clone();
        let body = json!({"currency": "EUR", "creator_code": code, "items": [{"product_id": pid, "cycle": "one_time"}]});
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
            call(&app, "POST", "/api/v1/client/orders", Some(body), &h).await
        }
    };
    let owned = |s: &Sess| -> Vec<(&'static str, String)> {
        vec![
            ("cookie", s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ]
    };
    let code = format!("Cool_{tag}");
    let lower = code.to_ascii_lowercase();

    // Off: an entered code is ignored, not an error.
    let o = order(owned(&buyer), &code).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(dec(&o.body["invoice"]["total"]), "10");

    enable(&app, &admin, &["creator_codes"]).await;
    let r = call(
        &app,
        "GET",
        "/api/v1/client/creator-code",
        None,
        &creator.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    for s in [&creator, &rival] {
        assert_eq!(
            call(&app, "POST", "/api/v1/client/affiliate", None, &s.h())
                .await
                .status,
            StatusCode::OK
        );
    }
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator-code",
        Some(json!({"code": "no spaces"})),
        &creator.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator-code",
        Some(json!({"code": code})),
        &creator.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["code"], lower.as_str());
    let cid = r.body["id"].as_str().unwrap().to_string();
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/creator-code",
        Some(json!({"code": lower})),
        &rival.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.creator_code_taken");

    let r = call(
        &app,
        "GET",
        &format!("/api/v1/public/creator-codes/{}", code.to_uppercase()),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["discount_percent"], "0");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/public/creator-codes/nobody-xyz",
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Admin: 10 % for buyers; customers can't manage codes; max 50 %.
    let url = format!("/api/v1/admin/creator-codes/{cid}");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/creator-codes",
            None,
            &creator.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "PUT",
        &url,
        Some(json!({"discount_percent": "60", "enabled": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "PUT",
        &url,
        Some(json!({"discount_percent": "10", "enabled": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let l = call(&app, "GET", "/api/v1/admin/creator-codes", None, &admin.h()).await;
    assert!(l.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["code"] == lower.as_str()));

    // Cart preview shows the discount; a bad code is reported, not fatal.
    let cart = |c: &str| {
        let app = app.clone();
        let body = json!({"currency": "EUR", "country": "QZ", "creator_code": c, "items": [{"product_id": pid, "cycle": "one_time"}]});
        async move {
            let r = call(
                &app,
                "PUT",
                &format!("/api/v1/store/carts/{}", uuid::Uuid::new_v4()),
                Some(body),
                &[],
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body["preview"].clone()
        }
    };
    let p = cart(&code).await;
    assert_eq!(
        (dec(&p["creator_discount"]), dec(&p["total"])),
        ("1".into(), "9".into()),
        "{p:?}"
    );
    assert_eq!(p["creator_code"], lower.as_str());
    let p = cart("nobody-xyz").await;
    assert_eq!(p["creator_error"], "error.creator_code_invalid");
    assert_eq!(dec(&p["total"]), "10");

    // Own code and unknown codes are refused at checkout.
    let o = order(owned(&creator), &code).await;
    assert_eq!(o.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(o.body["error"]["code"], "error.creator_code_own");
    assert_eq!(
        order(owned(&buyer), "nobody-xyz").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Buyer pays 9.00; the creator earns their 10 % commission on it.
    let o = order(owned(&buyer), &code).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(dec(&o.body["invoice"]["total"]), "9");
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let p = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("cc-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let d = call(&app, "GET", "/api/v1/client/affiliate", None, &creator.h())
        .await
        .body;
    let c = d["commissions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["invoice_id"] == iid.as_str())
        .cloned();
    assert_eq!(
        c.as_ref().map(|c| dec(&c["amount"])),
        Some("0.9".into()),
        "{d:?}"
    );
    let me = call(
        &app,
        "GET",
        "/api/v1/client/creator-code",
        None,
        &creator.h(),
    )
    .await;
    assert_eq!(me.body["orders"], 1);

    // Disabled code: refused. Module off: ignored again.
    let r = call(
        &app,
        "PUT",
        &url,
        Some(json!({"discount_percent": "10", "enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        order(owned(&buyer), &code).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    reset_modules(&st).await;
    let o = order(owned(&buyer), &code).await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    assert_eq!(dec(&o.body["invoice"]["total"]), "10");
}
