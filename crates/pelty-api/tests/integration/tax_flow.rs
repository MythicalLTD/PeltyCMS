use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};

async fn customer(app: &axum::Router, st: &pelty_api::AppState, profile: Value) -> (Sess, String) {
    let c = sess(app, st, "customer").await;
    let r = call(app, "PUT", "/api/v1/client/profile", Some(profile), &c.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    (c, id)
}

fn n(v: &Value) -> String {
    v.as_str()
        .unwrap_or_default()
        .parse::<rust_decimal::Decimal>()
        .map(|d| d.normalize().to_string())
        .unwrap_or_default()
}

async fn invoice(app: &axum::Router, admin: &Sess, cid: &str, items: Value) -> Value {
    let r = call(
        app,
        "POST",
        "/api/v1/admin/invoices",
        Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true, "items": items})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["invoice"].clone()
}

#[tokio::test]
async fn tax_rules_vat_reverse_charge_exempt_inclusive_and_compound() {
    let (app, st) = app().await;
    pelty_database::raw_execute(&st.pool, "DELETE FROM tax_rules")
        .await
        .unwrap();
    let admin = sess(&app, &st, "admin").await;

    // Permissions + validation.
    let (alice, ro) = customer(&app, &st, json!({"country": "RO"})).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/tax-rules", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/tax",
            Some(json!({"enabled": true})),
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let bad = call(
        &app,
        "POST",
        "/api/v1/admin/tax-rules",
        Some(json!({"name": "X", "country": "RO", "rate": "101"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (bad.status, bad.body["error"]["code"].clone()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!("error.invalid_tax_rate")
        ),
        "{:?}",
        bad.body
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/tax",
            Some(json!({"home_country": "Romania"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE", "vat_number": "DE12"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = call(&app, "PUT", "/api/v1/admin/settings/tax", Some(json!({"enabled": true, "inclusive": false, "home_country": "ro", "reverse_charge": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["home_country"], "RO");
    for rule in [
        json!({"name": "TVA", "country": "RO", "rate": "19"}),
        json!({"name": "MwSt", "country": "DE", "rate": "19"}),
        json!({"name": "GST", "country": "CA", "region": "QC", "rate": "5", "is_vat": false}),
        json!({"name": "QST", "country": "CA", "region": "QC", "rate": "9.975", "compound": true, "is_vat": false, "priority": 1}),
        json!({"name": "GST", "country": "CA", "rate": "5", "is_vat": false}),
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/tax-rules",
            Some(rule),
            &admin.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/tax-rules", None, &admin.h())
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .len(),
        5
    );

    // Domestic: 19 % on top; manual-rate lines keep their own rate.
    let inv = invoice(
        &app,
        &admin,
        &ro,
        json!([
            {"description": "VPS", "quantity": "1", "unit_price": "100.00"},
            {"description": "Manual", "quantity": "1", "unit_price": "10.00", "tax_rate": "9"}
        ]),
    )
    .await;
    assert_eq!(
        (n(&inv["subtotal"]), n(&inv["tax_total"]), n(&inv["total"])),
        ("110".into(), "19.9".into(), "129.9".into())
    );
    assert_eq!(inv["reverse_charge"], false);
    assert_eq!(inv["tax_breakdown"].as_array().unwrap().len(), 2);
    assert_eq!(inv["customer_snapshot"]["country"], "RO");
    let iid = inv["id"].as_str().unwrap();
    // Client sees the breakdown; snapshot + tax fields are frozen.
    let mine = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{iid}"),
        None,
        &alice.h(),
    )
    .await;
    assert_eq!(
        mine.body["invoice"]["tax_breakdown"][0]["name"], "TVA",
        "{:?}",
        mine.body
    );
    for sql in [
        format!("UPDATE invoices SET tax_breakdown = '[]' WHERE id = '{iid}'"),
        format!("UPDATE invoices SET customer_snapshot = NULL WHERE id = '{iid}'"),
        format!("UPDATE invoices SET reverse_charge = TRUE WHERE id = '{iid}'"),
    ] {
        assert!(
            pelty_database::raw_execute(&st.pool, &sql).await.is_err(),
            "must fail: {sql}"
        );
    }

    // EU B2B with VAT number in another member state: reverse charge, no VAT.
    let (_b, de) = customer(
        &app,
        &st,
        json!({"country": "DE", "vat_number": "de 123.456-789"}),
    )
    .await;
    let inv = invoice(
        &app,
        &admin,
        &de,
        json!([{"description": "VPS", "quantity": "1", "unit_price": "100.00"}]),
    )
    .await;
    assert_eq!(
        (n(&inv["tax_total"]), inv["reverse_charge"].as_bool()),
        ("0".into(), Some(true))
    );
    assert_eq!(inv["customer_snapshot"]["vat_number"], "DE123456789");
    // German consumer (no VAT number) pays German VAT.
    let (_c, de2) = customer(&app, &st, json!({"country": "DE"})).await;
    assert_eq!(
        n(&invoice(
            &app,
            &admin,
            &de2,
            json!([{"description": "VPS", "quantity": "2", "unit_price": "5.00"}])
        )
        .await["tax_total"]),
        "1.9"
    );

    // Staff-set exemption.
    let (_d, ex) = customer(&app, &st, json!({"country": "RO"})).await;
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/customers/{ex}"),
        Some(json!({"tax_exempt": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["tax_exempt"].as_bool()),
        (StatusCode::OK, Some(true)),
        "{:?}",
        r.body
    );
    assert_eq!(
        n(&invoice(
            &app,
            &admin,
            &ex,
            json!([{"description": "VPS", "quantity": "1", "unit_price": "100.00"}])
        )
        .await["tax_total"]),
        "0"
    );

    // Region tier wins over country tier; compound QST on net + GST.
    let (_e, qc) = customer(&app, &st, json!({"country": "CA", "region": "qc"})).await;
    let inv = invoice(
        &app,
        &admin,
        &qc,
        json!([{"description": "VPS", "quantity": "1", "unit_price": "100.00"}]),
    )
    .await;
    assert_eq!(n(&inv["tax_total"]), "15.47");
    let (_f, on) = customer(&app, &st, json!({"country": "CA", "region": "ON"})).await;
    assert_eq!(
        n(&invoice(
            &app,
            &admin,
            &on,
            json!([{"description": "VPS", "quantity": "1", "unit_price": "100.00"}])
        )
        .await["tax_total"]),
        "5"
    );
    // No rule for the country: no tax.
    let (_g, us) = customer(&app, &st, json!({"country": "US"})).await;
    assert_eq!(
        n(&invoice(
            &app,
            &admin,
            &us,
            json!([{"description": "VPS", "quantity": "1", "unit_price": "100.00"}])
        )
        .await["tax_total"]),
        "0"
    );

    // Order flow is taxed too (per line: 12.50 -> 2.38, 5.00 setup -> 0.95).
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[1]}}]})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            n(&r.body["invoice"]["tax_total"]),
            n(&r.body["invoice"]["total"])
        ),
        ("3.33".into(), "20.83".into())
    );

    // Tax-inclusive prices: gross stays, net is derived; lines add up exactly.
    let r = call(&app, "PUT", "/api/v1/admin/settings/tax", Some(json!({"enabled": true, "inclusive": true, "home_country": "RO", "reverse_charge": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK);
    let inv = invoice(&app, &admin, &ro, json!([{"description": "VPS", "quantity": "1", "unit_price": "119.00"}, {"description": "Small", "quantity": "3", "unit_price": "3.33"}])).await;
    assert_eq!(n(&inv["total"]), "128.99");
    assert_eq!(inv["tax_inclusive"], true);
    let sub: rust_decimal::Decimal = inv["subtotal"].as_str().unwrap().parse().unwrap();
    let tax: rust_decimal::Decimal = inv["tax_total"].as_str().unwrap().parse().unwrap();
    assert_eq!(sub + tax, "128.99".parse().unwrap());
    assert_eq!(tax, "20.60".parse().unwrap()); // 19.00 + (9.99 gross -> 8.39 net + 1.60)

    // Disabled engine: auto lines get no tax.
    call(
        &app,
        "PUT",
        "/api/v1/admin/settings/tax",
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        n(&invoice(
            &app,
            &admin,
            &ro,
            json!([{"description": "VPS", "quantity": "1", "unit_price": "100.00"}])
        )
        .await["tax_total"]),
        "0"
    );

    // Rule CRUD.
    let rules = call(&app, "GET", "/api/v1/admin/tax-rules", None, &admin.h())
        .await
        .body["items"]
        .clone();
    let id = rules[0]["id"].as_str().unwrap();
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/tax-rules/{id}"),
        Some(json!({"name": "VAT", "country": "AT", "rate": "20"})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (r.status, r.body["rate"].as_str()),
        (StatusCode::OK, Some("20.0000")),
        "{:?}",
        r.body
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/tax-rules/{id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/tax-rules/{id}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
