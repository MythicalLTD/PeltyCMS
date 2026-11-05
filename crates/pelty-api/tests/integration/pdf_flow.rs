use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

async fn get_raw(
    app: &axum::Router,
    uri: &str,
    h: &[(&str, &str)],
) -> (StatusCode, String, Vec<u8>) {
    let mut b = Request::builder().uri(uri);
    for (k, v) in h {
        b = b.header(*k, *v);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    (
        res.status(),
        ct,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

/// Extract text with poppler's pdftotext (exercises the ToUnicode map the way
/// real viewers do). Required: tests fail loudly rather than silently skip.
fn pdf_text(pdf: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("pelty-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, pdf).unwrap();
    let out = std::process::Command::new("pdftotext")
        .arg(&path)
        .arg("-")
        .output()
        .expect("pdftotext (poppler-utils) must be installed");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[tokio::test]
async fn invoice_pdf_branding_and_ownership() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "DE", "first_name": "Alice", "city": "Berlin"})),
        &alice.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let aid = r.body["id"].as_str().unwrap().to_string();

    // Branding: staff only, validated.
    let br = "/api/v1/admin/settings/invoice-branding";
    assert_eq!(
        call(&app, "GET", br, None, &alice.h()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(call(&app, "PUT", br, Some(json!({"company_name": "X", "address": "", "tax_id": "", "footer": "", "accent": "red"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "PUT", br, Some(json!({"company_name": "Pelty Test SRL", "address": "Str. Ștefan cel Mare 1\nBucurești", "tax_id": "RO123", "footer": "Thank you!", "accent": "#1E40AF"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["accent"], "#1e40af");

    let r = call(&app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": aid, "currency": "EUR", "finalize": true,
        "items": [{"description": "Hosting plan", "quantity": "2", "unit_price": "12.50", "tax_rate": "19"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let number = r.body["invoice"]["number"].as_str().unwrap().to_string();

    let url = format!("/api/v1/client/invoices/{id}/pdf");
    let (s, ct, pdf) = get_raw(&app, &url, &alice.h()).await;
    assert_eq!((s, ct.as_str()), (StatusCode::OK, "application/pdf"));
    assert!(
        pdf.starts_with(b"%PDF-") && pdf.ends_with(b"%%EOF\n")
            || pdf.windows(5).any(|w| w == b"%%EOF")
    );
    let raw = String::from_utf8_lossy(&pdf);
    assert!(raw.contains("/FontFile2") && raw.contains("/ToUnicode"));
    let text = pdf_text(&pdf);
    for needle in [
        "Pelty Test SRL",
        "Ștefan cel Mare",
        "București",
        "Hosting plan",
        "Alice",
        &number,
        "Thank you!",
    ] {
        assert!(text.contains(needle), "missing {needle} in {text}");
    }
    assert_eq!(get_raw(&app, &url, &bob.h()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get_raw(&app, &url, &[]).await.0, StatusCode::UNAUTHORIZED);
    let (s, _, admin_pdf) = get_raw(
        &app,
        &format!("/api/v1/admin/invoices/{id}/pdf"),
        &admin.h(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(admin_pdf.starts_with(b"%PDF-"));
    assert_eq!(
        get_raw(
            &app,
            &format!("/api/v1/admin/invoices/{id}/pdf"),
            &alice.h()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    std::fs::write("/tmp/wtax-invoice.pdf", &pdf).ok();
}
