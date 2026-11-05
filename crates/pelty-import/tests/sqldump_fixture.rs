//! WHMCS SQL-export path end to end: tests/fixtures/whmcs.sql (mysqldump
//! format, every imported table plus card columns that must be ignored) ->
//! sqldump::parse -> cc_encryption_hash decrypt -> import into a real
//! PeltyCMS Postgres (TEST_DATABASE_URL). No MySQL needed; the live MySQL
//! loader (`--features mysql`) is not covered here.
use pelty_import::{sqldump, WhmcsData};

const CC: &str = "Xk2v9QpL7mZr4tYw1sN8cB3dF6gH0jKa";

fn load(tag: &str) -> WhmcsData {
    let sql = include_str!("fixtures/whmcs.sql").replace("@T@", tag);
    let t = sqldump::parse(&sql).expect("parse fixture");
    WhmcsData::from_tables(&t)
}

/// Test "sealer": reversible and visibly different from the plaintext.
fn fake_seal(p: &[u8]) -> Vec<u8> {
    let mut v = b"sealed:".to_vec();
    v.extend(p.iter().rev());
    v
}

#[test]
fn fixture_parses_and_decrypts() {
    let mut d = load("x");
    assert_eq!(d.clients.len(), 3);
    assert_eq!(d.clients[1].companyname, "O'Brien \"Web\" Ltd");
    assert_eq!(d.clients[1].lastname, "O'Brien");
    // tblusers (WHMCS 8) overrides the empty tblclients.password by email
    assert!(d.clients[0].password.starts_with("$2y$10$"));
    assert_eq!(d.ticket_replies.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(d.announcements.len(), 1, "translations (parentid>0) skipped");
    assert_eq!(d.kb_articles.len(), 1);
    assert_eq!(d.servers.len(), 3);
    assert_eq!(d.promotions.len(), 4);
    assert_eq!(d.tax_rules.len(), 3);
    assert_eq!(d.contacts.len(), 2);
    assert_eq!(d.config_pricing.len(), 1);
    assert_eq!(d.pricing.len(), 5);
    let w = d.decrypt_server_passwords("wrong-key");
    assert!(!w.is_empty(), "wrong key must be reported");
    let mut d = load("x");
    let w = d.decrypt_server_passwords(CC);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(d.servers[0].password, "R00t-Pa$$-for-whm-server-01!");
    assert!(d.servers[0].password_plain);
    // the model has no field that could hold card data
    let dbg = format!("{d:?}");
    for card in ["4242", "DEADBEEF", "9F2C11AB", "5555"] {
        assert!(!dbg.contains(card), "card data {card} leaked into the import model");
    }
}

#[tokio::test]
async fn sql_export_imports_exactly_once() {
    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/peltycms_test".into());
    let pool = pelty_database::connect(&url, 4).await.expect("connect");
    pelty_database::migrate(&pool).await.expect("migrate");
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    // Tests in other binaries run sequentially; start from an empty map so
    // the fixture's WHMCS ids are new (rows are unique through the tag).
    sqlx::query("DELETE FROM import_map WHERE source='whmcs'").execute(&pool).await.unwrap();
    let source = "whmcs";
    let mut data = load(&tag);
    assert!(data.decrypt_server_passwords(CC).is_empty());

    let dry = pelty_import::run_with(&pool, &data, true, source, Some(&fake_seal)).await.expect("dry run");
    println!("{}", dry.render());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM import_map WHERE source=$1").bind(source).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0, "dry run must roll back");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM customers WHERE email LIKE $1").bind(format!("%-{tag}@%")).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);

    let rep = pelty_import::run_with(&pool, &data, false, source, Some(&fake_seal)).await.expect("import");
    println!("{}", rep.render());
    let body = |r: &pelty_import::Report| r.render().split_once('\n').unwrap().1.to_string();
    assert_eq!(body(&rep), body(&dry), "dry run must report exactly what the real run does");
    let created = |k: &str| rep.entities.get(k).map(|c| c.created).unwrap_or_default();
    for (k, n) in [
        ("customers", 3), ("users", 3), ("password_hashes_kept", 2), ("contacts", 2), ("client_groups", 1),
        ("product_groups", 2), ("products", 4), ("product_prices", 11), ("addons", 1), ("addon_prices", 2),
        ("config_options", 1), ("config_choices", 2), ("servers", 2), ("services", 4), ("service_addons", 1),
        ("domains", 2), ("payments", 2), ("credit", 1),
        ("ticket_departments", 2), ("tickets", 2), ("ticket_replies", 3), ("announcements", 1),
        ("kb_categories", 1), ("kb_articles", 1), ("custom_fields", 1), ("custom_values", 1),
        ("coupons", 2), ("tax_rules", 3),
    ] {
        assert_eq!(created(k), n, "{k}");
    }
    // invoice 1003 has no invoicenum, so it is numbered "1003": on a DB
    // that already holds an earlier run it is skipped with a warning.
    let clash = rep.warnings.iter().any(|w| w.contains("invoice 1003 number"));
    assert_eq!(created("invoices"), if clash { 2 } else { 3 });
    assert_eq!(created("invoice_items"), if clash { 4 } else { 5 });
    assert!(rep.warnings.iter().any(|w| w.contains("Free Setup")));
    assert!(rep.warnings.iter().any(|w| w.contains("no hostname or IP")));

    // login hashes: bcrypt (from tblusers) and legacy md5 kept, unknown -> reset
    let hash = |e: String| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, String>("SELECT password_hash FROM users WHERE lower(email)=lower($1)").bind(e).fetch_one(&pool).await.unwrap() }
    };
    let ana = hash(format!("ana-{tag}@example.com")).await;
    assert!(ana.starts_with("whmcs:$2y$"), "{ana}");
    assert!(pelty_auth::password::verify("Ana-Pass-2024", &ana) && !pelty_auth::password::verify("wrong", &ana));
    let bob = hash(format!("bob-{tag}@example.org")).await;
    assert!(pelty_auth::password::verify("Bob-Old-Pass", &bob) && pelty_auth::password::needs_rehash(&bob));
    assert_eq!(hash(format!("carla-{tag}@example.de")).await, pelty_import::import::LOCKED_PASSWORD);

    // servers: decrypted password sealed, accesshash preferred, linked to services
    let (url, sealed, enabled, user, maxs): (String, Vec<u8>, bool, String, Option<i32>) = sqlx::query_as(
        "SELECT base_url, api_key_sealed, enabled, config->>'username', max_services FROM servers WHERE name=$1",
    ).bind(format!("WHM-01 {tag}")).fetch_one(&pool).await.unwrap();
    assert_eq!(url, "https://whm01.example.net:2087");
    assert_eq!(sealed, fake_seal(b"R00t-Pa$$-for-whm-server-01!"));
    assert!(enabled);
    assert_eq!(user, "root");
    assert_eq!(maxs, Some(200));
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT api_key_sealed FROM servers WHERE name=$1")
        .bind(format!("Virt-01 {tag}")).fetch_one(&pool).await.unwrap();
    assert_eq!(sealed, fake_seal(b"VIRT-ACCESS-HASH-123"));
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM services sv JOIN servers s ON s.id=sv.server_id WHERE s.name=$1 AND sv.module_data->>'username' IN ('anapop','obrienw')",
    ).bind(format!("WHM-01 {tag}")).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 2);

    // promotions -> coupons, tbltax -> tax rules, contacts
    let (kind, max, uses): (String, Option<i32>, i32) = sqlx::query_as("SELECT kind::text, max_uses, uses FROM coupons WHERE upper(code)=upper($1)")
        .bind(format!("SPRING{tag}")).fetch_one(&pool).await.unwrap();
    assert_eq!((kind.as_str(), max, uses), ("percent", Some(100), 7));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tax_rules WHERE name LIKE $1").bind(format!("% {tag}")).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 3);
    let billing: bool = sqlx::query_scalar("SELECT is_billing FROM customer_contacts WHERE email=$1")
        .bind(format!("billing-{tag}@example.com")).fetch_one(&pool).await.unwrap();
    assert!(billing);

    // no card data anywhere in the imported customers / users
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM customers c WHERE c.email LIKE $1 AND (row_to_json(c)::text ~ '4242|5555|DEADBEEF|9F2C11AB')",
    ).bind(format!("%-{tag}@%")).fetch_one(&pool).await.unwrap();
    assert_eq!(leaked, 0);

    // idempotent: a second run creates nothing
    let again = pelty_import::run_with(&pool, &data, false, source, Some(&fake_seal)).await.expect("rerun");
    assert!(again.entities.values().all(|c| c.created == 0), "{}", again.render());
    assert_eq!(again.entities["customers"].skipped_existing, 3);
}
