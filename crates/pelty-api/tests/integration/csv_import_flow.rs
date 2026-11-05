use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn csv_import_clients_services_invoices() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let c = sess(&app, &st, "customer").await;
    let (pid, _, _) = seed_product(&app, &admin).await;
    let slug: String = sqlx::query_scalar("SELECT slug FROM products WHERE id = $1::uuid")
        .bind(&pid)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let (e1, e2) = (format!("a{tag}@imp.test"), format!("b{tag}@imp.test"));
    let ah = admin.h();
    let post = |p: &'static str, b: serde_json::Value| call(&app, "POST", p, Some(b), &ah);

    let clients = format!("E-mail;First Name;Company Name;Country\n{e1};Ana;\"Acme; SRL\";ro\n{e2};Bob;;DE\nnot-an-email;X;;\n");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/import/csv/analyze",
            Some(json!({"kind": "clients", "csv": clients})),
            &c.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let a = post(
        "/api/v1/admin/import/csv/analyze",
        json!({"kind": "clients", "csv": clients}),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK, "{:?}", a.body);
    assert_eq!(a.body["rows"], 3);
    assert_eq!(a.body["preview"][0][2], "Acme; SRL");
    let mapping = a.body["mapping"].clone();
    assert_eq!(mapping["email"], "E-mail");
    assert_eq!(mapping["organization"], "Company Name");

    // Dry run reports the bad row and writes nothing.
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "clients", "csv": clients, "mapping": mapping, "dry_run": true}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["created"].as_u64(),
            r.body["error_count"].as_u64(),
            r.body["committed"].as_bool()
        ),
        (Some(2), Some(1), Some(false))
    );
    assert_eq!(r.body["errors"][0]["row"], 3);
    assert_eq!(r.body["errors"][0]["code"], "error.import_bad_email");
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM customers WHERE email::text LIKE $1")
            .bind(format!("%{tag}@imp.test"))
            .fetch_one(&st.pool)
            .await
            .unwrap()
    };
    assert_eq!(count().await, 0);
    // A live run with errors is refused as a whole.
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "clients", "csv": clients, "mapping": mapping, "dry_run": false}),
    )
    .await;
    assert_eq!(r.body["committed"], false);
    assert_eq!(count().await, 0);
    // Fixed file imports; re-running skips duplicates.
    let fixed: String = clients.lines().take(3).collect::<Vec<_>>().join("\n");
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "clients", "csv": fixed, "mapping": mapping, "dry_run": false}),
    )
    .await;
    assert_eq!(
        (r.body["created"].as_u64(), r.body["committed"].as_bool()),
        (Some(2), Some(true)),
        "{:?}",
        r.body
    );
    assert_eq!(count().await, 2);
    let org: String =
        sqlx::query_scalar("SELECT organization FROM customers WHERE email = $1::citext")
            .bind(&e1)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(org, "Acme; SRL");
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "clients", "csv": fixed, "mapping": mapping, "dry_run": false}),
    )
    .await;
    assert_eq!(
        (r.body["created"].as_u64(), r.body["skipped"].as_u64()),
        (Some(0), Some(2))
    );

    // Mapping must cover required fields.
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "clients", "csv": fixed, "mapping": {"first_name": "First Name"}}),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Services by product slug.
    let svc = format!("client,plan,billing cycle,price,next due date\n{e1},{slug},Monthly,9.99,2027-01-15\n{e2},nope,monthly,1,\n");
    let m = post(
        "/api/v1/admin/import/csv/analyze",
        json!({"kind": "services", "csv": svc}),
    )
    .await
    .body["mapping"]
        .clone();
    let mut m = m.as_object().unwrap().clone();
    m.insert("email".into(), json!("client"));
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "services", "csv": svc, "mapping": m, "dry_run": true}),
    )
    .await;
    assert_eq!(
        r.body["errors"][0]["code"], "error.import_unknown_product",
        "{:?}",
        r.body
    );
    let ok: String = svc.lines().take(2).collect::<Vec<_>>().join("\n");
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "services", "csv": ok, "mapping": m, "dry_run": false}),
    )
    .await;
    assert_eq!(r.body["created"], 1, "{:?}", r.body);
    let (amt, due): (rust_decimal::Decimal, Option<time::Date>) = sqlx::query_as("SELECT s.recurring_amount, s.next_due_date FROM services s JOIN customers c ON c.id = s.customer_id WHERE c.email = $1::citext").bind(&e1).fetch_one(&st.pool).await.unwrap();
    assert_eq!(amt.to_string(), "9.9900");
    assert_eq!(due.unwrap().to_string(), "2027-01-15");

    // Invoices: finalized with their number; duplicate numbers skipped.
    let num = format!("IMP-{tag}");
    let inv = format!("email,invoice number,total,status,date\n{e2},{num},120.50,paid,2026-03-01\n{e2},{num},1,unpaid,\n");
    let m = post(
        "/api/v1/admin/import/csv/analyze",
        json!({"kind": "invoices", "csv": inv}),
    )
    .await
    .body["mapping"]
        .clone();
    let r = post(
        "/api/v1/admin/import/csv/run",
        json!({"kind": "invoices", "csv": inv, "mapping": m, "dry_run": false}),
    )
    .await;
    assert_eq!(
        (
            r.body["created"].as_u64(),
            r.body["skipped"].as_u64(),
            r.body["committed"].as_bool()
        ),
        (Some(1), Some(1), Some(true)),
        "{:?}",
        r.body
    );
    let (st_, total, paid): (String, rust_decimal::Decimal, rust_decimal::Decimal) =
        sqlx::query_as("SELECT status, total, amount_paid FROM invoices WHERE number = $1")
            .bind(&num)
            .fetch_one(&st.pool)
            .await
            .unwrap();
    assert_eq!(
        (
            st_.as_str(),
            total.to_string().as_str(),
            paid.to_string().as_str()
        ),
        ("paid", "120.5000", "120.5000")
    );
}
