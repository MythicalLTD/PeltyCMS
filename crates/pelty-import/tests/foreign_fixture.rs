//! Blesta + Paymenter JSON exports imported end-to-end into a real PeltyCMS
//! Postgres (TEST_DATABASE_URL): mapping, dry run, idempotency, exact money.
use rust_decimal::Decimal;
use std::str::FromStr;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

async fn pool() -> sqlx::PgPool {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let pool = pelty_database::connect(&url, 4).await.expect("connect");
    pelty_database::migrate(&pool).await.expect("migrate");
    pool
}

/// Fixture ids repeat between runs on a shared DB; forget earlier mappings
/// of this source so the run starts like a first import.
async fn fresh(pool: &sqlx::PgPool, source: &str) {
    sqlx::query("DELETE FROM import_map WHERE source=$1").bind(source).execute(pool).await.unwrap();
}

fn tag() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

#[tokio::test]
async fn blesta_export_imports_exactly() {
    let pool = pool().await;
    let t = tag();
    fresh(&pool, "blesta").await;
    let json = include_str!("fixtures/blesta.json").replace("TAG", &t);
    let exp = pelty_import::blesta::parse(&json).expect("parse");
    let (data, warn) = pelty_import::blesta::convert(&exp);
    assert!(warn.iter().any(|w| w.contains("client 3 has no primary contact")), "{warn:?}");
    assert!(warn.iter().any(|w| w.contains("service 21")), "{warn:?}");
    assert!(warn.iter().any(|w| w.contains("pricing 103")), "weekly is unsupported: {warn:?}");
    assert_eq!(data.clients.len(), 2);
    assert_eq!(data.transactions.len(), 1, "declined dropped");
    let p = data.pricing.iter().find(|p| p.relid == 10 && p.currency == 1).unwrap();
    assert_eq!((p.monthly, p.msetupfee, p.annually, p.quarterly), (d("5.0000"), d("1.5000"), d("50.0000"), d("-1")));

    let dry = pelty_import::run_source(&pool, &data, true, "blesta").await.expect("dry");
    assert!(dry.dry_run);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM invoices WHERE number=$1").bind(format!("B{t}-1")).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0, "dry run rolls back");

    let rep = pelty_import::run_source(&pool, &data, false, "blesta").await.expect("run");
    assert_eq!(rep.source, "blesta");
    assert_eq!(rep.entities["customers"].created, 2);
    assert_eq!(rep.entities["invoices"].created, 2);
    assert_eq!(rep.entities["services"].created, 1);
    assert_eq!(rep.entities["payments"].created, 1);
    assert_eq!(rep.entities["tickets"].created, 1);
    assert_eq!(rep.entities["ticket_replies"].created, 1, "notes are not replies");

    let (status, total, tax, paid): (String, Decimal, Decimal, Decimal) =
        sqlx::query_as("SELECT status, total, tax_total, amount_paid FROM invoices WHERE number=$1")
            .bind(format!("B{t}-1")).fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), total, tax, paid), ("paid", d("7.74"), d("1.24"), d("7.74")));
    let st: String = sqlx::query_scalar("SELECT status FROM invoices WHERE number=$1").bind(format!("B{t}-2")).fetch_one(&pool).await.unwrap();
    assert_eq!(st, "unpaid");
    let gw: String = sqlx::query_scalar("SELECT p.gateway FROM payments p JOIN invoices i ON i.id=p.invoice_id WHERE i.number=$1")
        .bind(format!("B{t}-1")).fetch_one(&pool).await.unwrap();
    assert_eq!(gw, "blesta:stripe");
    let src: i64 = sqlx::query_scalar("SELECT count(*) FROM import_map WHERE source='blesta' AND entity='invoice' AND source_id IN ('30','31')").fetch_one(&pool).await.unwrap();
    assert!(src >= 2);

    let again = pelty_import::run_source(&pool, &data, false, "blesta").await.expect("rerun");
    assert_eq!(again.entities.get("invoices").map(|c| c.created).unwrap_or(0), 0, "idempotent");
    assert_eq!(again.entities["invoices"].skipped_existing, 2);
    assert!(!rep.render().is_empty());
}

#[tokio::test]
async fn paymenter_export_imports_exactly() {
    let pool = pool().await;
    let t = tag();
    fresh(&pool, "paymenter").await;
    let json = include_str!("fixtures/paymenter.json").replace("TAG", &t);
    let exp = pelty_import::paymenter::parse(&json).expect("parse");
    let (data, warn) = pelty_import::paymenter::convert(&exp);
    assert!(warn.iter().any(|w| w.contains("plan 72")), "daily unsupported: {warn:?}");
    assert_eq!(data.clients.len(), 1, "staff user is not a client");
    assert_eq!(data.clients[0].credit, d("3.25"));
    let p = &data.pricing[0];
    assert_eq!((p.monthly, p.quarterly), (d("8.00"), d("21.60")));
    assert_eq!(data.invoices[0].total, d("9.00"), "8.00 + 2 × 0.50");
    assert_eq!(data.ticket_replies.len(), 1);
    assert!(data.ticket_replies[0].admin.starts_with("staff"));

    let rep = pelty_import::run_source(&pool, &data, false, "paymenter").await.expect("run");
    assert_eq!(rep.entities["customers"].created, 1);
    assert_eq!(rep.entities["invoices"].created, 2);
    assert_eq!(rep.entities["services"].created, 1);
    assert_eq!(rep.entities["payments"].created, 1, "failed transaction skipped");
    let (status, total, paid): (String, Decimal, Decimal) =
        sqlx::query_as("SELECT status, total, amount_paid FROM invoices WHERE number=$1")
            .bind(format!("P{t}-3")).fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), total, paid), ("paid", d("9.00"), d("9.00")));
    let cycle: String = sqlx::query_scalar("SELECT s.cycle FROM services s JOIN customers c ON c.id=s.customer_id JOIN users u ON u.id=c.owner_user_id WHERE u.email=$1")
        .bind(format!("ana-{t}@example.com")).fetch_one(&pool).await.unwrap();
    assert_eq!(cycle, "monthly");
    let again = pelty_import::run_source(&pool, &data, false, "paymenter").await.expect("rerun");
    assert_eq!(again.entities["customers"].skipped_existing, 1);
}
