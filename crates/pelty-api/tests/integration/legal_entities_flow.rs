use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
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

fn tag() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..6].to_uppercase()
}

async fn issue(app: &axum::Router, admin: &Sess, cid: &str) -> Value {
    let r = call(app, "POST", "/api/v1/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true, "items": [{"description": "VPS", "quantity": "1", "unit_price": "10.00", "tax_rate": "0"}]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["invoice"].clone()
}

#[tokio::test]
async fn legal_entities_numbering_terms_and_assignment() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let cid = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "RO", "first_name": "Ana"})),
        &alice.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let bid = call(
        &app,
        "PUT",
        "/api/v1/client/profile",
        Some(json!({"country": "RO", "first_name": "Bob"})),
        &bob.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/legal-entities",
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let (p, c) = (format!("R{}", tag()), format!("C{}", tag()));
    let bad = call(&app, "POST", "/api/v1/admin/legal-entities", Some(json!({"name": "X", "invoice_prefix": p, "credit_prefix": c, "number_format": "{year}-{seq}"})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/legal-entities", Some(json!({
        "name": "Pelty Hosting SRL", "address": "Str. Ștefan cel Mare 1\nIași", "tax_id": "RO123", "bank_name": "Banca Transilvania",
        "iban": "ro49 aaaa 1b31 0075 9384 0000", "swift": "btrlro22", "footer": "Mulțumim!", "terms": "Plata în 14 zile.",
        "invoice_prefix": p, "credit_prefix": c, "number_format": "{prefix}/{yy}/{seq:4}", "due_days": 14
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let eid = r.body["id"].as_str().unwrap().to_string();
    let yy = format!("{:02}", time::OffsetDateTime::now_utc().year() % 100);
    assert_eq!(r.body["iban"], "RO49AAAA1B31007593840000");
    assert_eq!(r.body["is_default"], false);
    assert_eq!(r.body["next_number"], format!("{p}/{yy}/0001"));
    // Prefix uniqueness (case-insensitive).
    let dup = call(&app, "POST", "/api/v1/admin/legal-entities", Some(json!({"name": "Y", "invoice_prefix": p.to_lowercase(), "credit_prefix": format!("Z{}", tag())})), &admin.h()).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    // Assign alice → per-entity, per-year numbering and due terms.
    let r = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/customers/{cid}/legal-entity"),
        Some(json!({"legal_entity_id": eid})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let a1 = issue(&app, &admin, &cid).await;
    let a2 = issue(&app, &admin, &cid).await;
    assert_eq!(a1["number"], format!("{p}/{yy}/0001"));
    assert_eq!(a2["number"], format!("{p}/{yy}/0002"));
    let issued = time::OffsetDateTime::parse(
        a1["issued_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let due = time::OffsetDateTime::parse(
        a1["due_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert_eq!((due - issued).whole_days(), 14);

    // Bob (no entity, no default) keeps legacy numbering.
    let b1 = issue(&app, &admin, &bid).await;
    assert!(b1["number"].as_str().unwrap().starts_with("INV-"), "{b1}");

    // Snapshot is frozen: editing the entity does not change issued documents.
    let a1id = a1["id"].as_str().unwrap();
    let r = call(&app, "PUT", &format!("/api/v1/admin/legal-entities/{eid}"), Some(json!({"name": "Renamed SRL", "invoice_prefix": p, "credit_prefix": c, "number_format": "{prefix}/{yy}/{seq:4}"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["next_number"], format!("{p}/{yy}/0003"));
    let snap: Value =
        sqlx::query_scalar("SELECT seller_snapshot FROM invoices WHERE id = $1::uuid")
            .bind(a1id)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(snap["name"], "Pelty Hosting SRL");
    assert_eq!(snap["terms"], "Plata în 14 zile.");
    let (code, _, pdf) = get_raw(
        &app,
        &format!("/api/v1/admin/invoices/{a1id}/pdf"),
        &admin.h(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let text = pdf_text(&pdf);
    for want in [
        "Pelty Hosting SRL",
        "Ștefan cel Mare",
        "RO49AAAA1B31007593840000",
        "BTRLRO22",
        "Plata în 14 zile.",
        "Mulțumim!",
    ] {
        assert!(text.contains(want), "missing {want}: {text}");
    }
    assert!(!text.contains("Renamed SRL"));

    // Credit note follows the credited invoice's entity.
    let r = call(&app, "POST", &format!("/api/v1/admin/invoices/{a1id}/payments"), Some(json!({"amount": "10.00", "idempotency_key": uuid::Uuid::new_v4().to_string(), "reference": format!("le-{}", tag()), "gateway": "manual"})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{a1id}/credit-note"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["number"], format!("{c}/{yy}/0001"), "{:?}", r.body);

    // Default entity applies to unassigned customers; cleared again afterwards.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/legal-entities/{eid}/default"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let b2 = issue(&app, &admin, &bid).await;
    assert_eq!(b2["number"], format!("{p}/{yy}/0003"));
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/legal-entities/default",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(issue(&app, &admin, &bid).await["number"]
        .as_str()
        .unwrap()
        .starts_with("INV-"));

    // In use → cannot delete; an unused one can.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/legal-entities/{eid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let r = call(&app, "POST", "/api/v1/admin/legal-entities", Some(json!({"name": "Temp", "invoice_prefix": format!("T{}", tag()), "credit_prefix": format!("U{}", tag())})), &admin.h()).await;
    let tid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/legal-entities/{tid}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/customers/{cid}/legal-entity"),
            Some(json!({"legal_entity_id": tid})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
