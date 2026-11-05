use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
use tower::ServiceExt;

async fn get_raw(app: &axum::Router, uri: &str, h: &[(&str, &str)]) -> (StatusCode, String, String) {
    let mut b = Request::builder().uri(uri);
    for (k, v) in h {
        b = b.header(*k, *v);
    }
    let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    let ct = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    (res.status(), ct, String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap())
}

fn d(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap_or("0")).unwrap()
}

/// (net, tax, gross, documents) of the IS/EUR row at `rate`, non reverse-charge.
fn row(r: &Value, period: &str, rate: &str) -> (Decimal, Decimal, Decimal, i64) {
    r["rows"].as_array().unwrap().iter()
        .find(|x| x["period"] == period && x["country"] == "IS" && x["currency"] == "EUR" && d(&x["rate"]) == Decimal::from_str(rate).unwrap() && x["reverse_charge"] == false)
        .map(|x| (d(&x["net"]), d(&x["tax"]), d(&x["gross"]), x["documents"].as_i64().unwrap()))
        .unwrap_or_default()
}

async fn invoice(app: &axum::Router, admin: &Sess, cid: &str, items: Value) -> (String, String, String) {
    let r = call(app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true, "items": items})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let i = &r.body["invoice"];
    (i["id"].as_str().unwrap().into(), i["number"].as_str().unwrap().into(), i["total"].as_str().unwrap().into())
}

#[tokio::test]
async fn tax_report_and_csv_exports_are_exact() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let cid = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "IS", "first_name": "=cmd", "last_name": "Ștefan, Jr."})), &alice.h()).await.body["id"].as_str().unwrap().to_string();
    let month = time::OffsetDateTime::now_utc().date().to_string()[..7].to_string();
    let q = format!("?from={month}&to={month}");

    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/reports/tax{q}"), None, &alice.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(get_raw(&app, "/api/v1/admin/exports/invoices.csv", &alice.h()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/admin/reports/tax?from=2026-13", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/reports/tax?from=2020-01&to=2026-01", None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    let before = call(&app, "GET", &format!("/api/v1/admin/reports/tax{q}"), None, &admin.h()).await.body;
    let (b24, b0) = (row(&before, &month, "24"), row(&before, &month, "0"));

    let (_, a_no, a_total) = invoice(&app, &admin, &cid, json!([
        {"description": "VPS", "quantity": "1", "unit_price": "100.00", "tax_rate": "24"},
        {"description": "Setup", "quantity": "1", "unit_price": "10.01", "tax_rate": "0"}])).await;
    assert_eq!(d(&json!(a_total)), Decimal::from_str("134.01").unwrap());
    let (b_id, b_no, b_total) = invoice(&app, &admin, &cid, json!([{"description": "Domain", "quantity": "1", "unit_price": "33.33", "tax_rate": "24"}])).await;
    assert_eq!(d(&json!(b_total)), Decimal::from_str("41.33").unwrap());
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{b_id}/payments"), Some(json!({"amount": b_total, "idempotency_key": uuid::Uuid::new_v4().to_string(), "reference": format!("bank-{}", uuid::Uuid::new_v4().simple()), "gateway": "manual"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pay = call(&app, "GET", &format!("/api/v1/admin/invoices/{b_id}"), None, &admin.h()).await.body["payments"][0]["id"].as_str().unwrap().to_string();
    let r = call(&app, "POST", &format!("/api/v1/admin/payments/{pay}/refunds"), Some(json!({"amount": "12.40", "idempotency_key": "exp"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // Report delta: 24 %: +100 +33.33 −10 (credit note) net; +24 +8.00 −2.40 tax.
    let after = call(&app, "GET", &format!("/api/v1/admin/reports/tax{q}"), None, &admin.h()).await;
    assert_eq!(after.status, StatusCode::OK);
    let (a24, a0) = (row(&after.body, &month, "24"), row(&after.body, &month, "0"));
    let s = |x: &str| Decimal::from_str(x).unwrap();
    assert_eq!((a24.0 - b24.0, a24.1 - b24.1, a24.2 - b24.2, a24.3 - b24.3), (s("123.33"), s("29.60"), s("152.93"), 3));
    assert_eq!((a0.0 - b0.0, a0.1 - b0.1, a0.3 - b0.3), (s("10.01"), s("0"), 1));
    assert!(after.body["totals"].as_array().unwrap().iter().any(|t| t["currency"] == "EUR"));

    // CSV: UTF-8 BOM, exact decimals, quoting + formula guard.
    let (st_, ct, csv) = get_raw(&app, &format!("/api/v1/admin/reports/tax.csv{q}"), &admin.h()).await;
    assert_eq!((st_, ct.as_str()), (StatusCode::OK, "text/csv; charset=utf-8"));
    assert!(csv.starts_with("\u{feff}period,country,currency,tax_rate,reverse_charge,documents,net,tax,gross\r\n"));
    assert!(csv.lines().any(|l| l.starts_with(&format!("{month},IS,EUR,24.00,no,"))), "{csv}");

    let (_, _, csv) = get_raw(&app, &format!("/api/v1/admin/exports/invoices.csv{q}"), &admin.h()).await;
    let a_line = csv.lines().find(|l| l.starts_with(&format!("{a_no},"))).unwrap();
    assert!(a_line.contains(",\"'=cmd Ștefan, Jr.\",") && a_line.contains(",IS,") && a_line.contains(",EUR,110.01,24.00,134.01,0.00,134.01,"), "{a_line}");
    let b_line = csv.lines().find(|l| l.starts_with(&format!("{b_no},"))).unwrap();
    assert!(b_line.contains(",invoice,refunded,") || b_line.contains(",invoice,paid,"), "{b_line}");
    assert!(b_line.contains(",33.33,8.00,41.33,41.33,0.00,"), "{b_line}");
    assert!(csv.lines().any(|l| l.contains(",credit_note,") && l.contains(",10.00,2.40,12.40,")), "{csv}");

    let (_, _, csv) = get_raw(&app, &format!("/api/v1/admin/exports/payments.csv{q}"), &admin.h()).await;
    let p = csv.lines().find(|l| l.ends_with(&pay)).unwrap();
    assert!(p.contains(&format!(",{b_no},")) && p.contains(",manual,bank-") && p.contains(",succeeded,EUR,41.33,12.40,28.93,"), "{p}");
}
