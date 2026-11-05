//! End-to-end import against a real PeltyCMS Postgres (TEST_DATABASE_URL),
//! using an in-memory WHMCS fixture, no MySQL/WHMCS needed.
use pelty_import::whmcs::*;
use rust_decimal::Decimal;
use std::str::FromStr;
use uuid::Uuid;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn date(s: &str) -> Option<time::Date> {
    pelty_import::map::parse_date(s)
}

fn fixture(tag: &str) -> WhmcsData {
    WhmcsData {
        currencies: vec![
            Currency {
                id: 1,
                code: "EUR".into(),
                default: true,
            },
            Currency {
                id: 2,
                code: "USD".into(),
                default: false,
            },
        ],
        clients: vec![
            Client {
                id: 1,
                firstname: "Ana".into(),
                lastname: "Pop".into(),
                email: format!("ana-{tag}@example.com"),
                country: "ro".into(),
                currency: 1,
                status: "Active".into(),
                credit: d("12.50"),
                ..Default::default()
            },
            Client {
                id: 2,
                firstname: "Bob".into(),
                email: "not-an-email".into(),
                currency: 2,
                status: "Closed".into(),
                ..Default::default()
            },
        ],
        product_groups: vec![ProductGroup {
            id: 1,
            name: format!("Shared {tag}"),
            hidden: false,
            order: 1,
        }],
        products: vec![Product {
            id: 7,
            gid: 1,
            name: format!("Starter {tag}"),
            servertype: "cpanel".into(),
            ..Default::default()
        }],
        pricing: vec![Pricing {
            relid: 7,
            currency: 1,
            monthly: d("4.99"),
            annually: d("49.90"),
            msetupfee: d("0.00"),
            quarterly: d("-1.00"),
            semiannually: d("-1.00"),
            biennially: d("-1.00"),
            triennially: d("-1.00"),
            ..Default::default()
        }],
        hosting: vec![Hosting {
            id: 10,
            userid: 1,
            packageid: 7,
            regdate: date("2024-01-01"),
            domain: "ana.ro".into(),
            billingcycle: "Monthly".into(),
            nextduedate: date("2024-03-01"),
            amount: d("4.99"),
            domainstatus: "Active".into(),
            ..Default::default()
        }],
        invoices: vec![
            Invoice {
                id: 100,
                userid: 1,
                invoicenum: format!("W{tag}-100"),
                date: date("2024-01-01"),
                duedate: date("2024-01-08"),
                datepaid: date("2024-01-02"),
                subtotal: d("4.99"),
                tax: d("0.95"),
                tax2: d("0.00"),
                total: d("5.94"),
                taxrate: d("19.00"),
                status: "Paid".into(),
                ..Default::default()
            },
            Invoice {
                id: 101,
                userid: 1,
                invoicenum: format!("W{tag}-101"),
                date: date("2024-02-01"),
                duedate: date("2024-02-08"),
                subtotal: d("4.99"),
                total: d("4.99"),
                status: "Unpaid".into(),
                ..Default::default()
            },
        ],
        invoice_items: vec![
            InvoiceItem {
                id: 1,
                invoiceid: 100,
                type_: "Hosting".into(),
                relid: 10,
                description: "Starter (ana.ro)".into(),
                amount: d("4.99"),
                taxed: true,
            },
            InvoiceItem {
                id: 2,
                invoiceid: 101,
                type_: "Hosting".into(),
                relid: 10,
                description: "Starter (ana.ro)".into(),
                amount: d("4.99"),
                taxed: false,
            },
        ],
        transactions: vec![Transaction {
            id: 5,
            userid: 1,
            currency: 0,
            gateway: "paypal".into(),
            date: date("2024-01-02"),
            amountin: d("5.94"),
            transid: format!("TX-{tag}"),
            invoiceid: 100,
            ..Default::default()
        }],
        client_groups: vec![ClientGroup {
            id: 3,
            groupname: format!("VIP {tag}"),
            discountpercent: d("10.00"),
            susptermexempt: true,
        }],
        addons: vec![Addon {
            id: 4,
            packages: "7".into(),
            name: format!("Backups {tag}"),
            billingcycle: "recurring".into(),
            ..Default::default()
        }],
        addon_pricing: vec![Pricing {
            relid: 4,
            currency: 1,
            monthly: d("1.50"),
            quarterly: d("-1"),
            semiannually: d("-1"),
            annually: d("-1"),
            biennially: d("-1"),
            triennially: d("-1"),
            ..Default::default()
        }],
        hosting_addons: vec![HostingAddon {
            id: 8,
            hostingid: 10,
            addonid: 4,
            name: "Backups".into(),
            recurring: d("1.50"),
            billingcycle: "Monthly".into(),
            status: "Active".into(),
            regdate: date("2024-01-01"),
        }],
        custom_fields: vec![
            CustomField {
                id: 11,
                type_: "product".into(),
                relid: 7,
                fieldname: "Server name".into(),
                fieldtype: "text".into(),
                ..Default::default()
            },
            CustomField {
                id: 12,
                type_: "product".into(),
                relid: 7,
                fieldname: "Managed".into(),
                fieldtype: "tickbox".into(),
                ..Default::default()
            },
            CustomField {
                id: 13,
                type_: "client".into(),
                relid: 0,
                fieldname: "VAT".into(),
                fieldtype: "text".into(),
                ..Default::default()
            },
        ],
        custom_field_values: vec![
            CustomFieldValue {
                fieldid: 11,
                relid: 10,
                value: "srv-ana".into(),
            },
            CustomFieldValue {
                fieldid: 12,
                relid: 10,
                value: "on".into(),
            },
        ],
        ticket_departments: vec![TicketDepartment {
            id: 1,
            name: "Support".into(),
            ..Default::default()
        }],
        tickets: vec![Ticket {
            id: 20,
            tid: "123456".into(),
            did: 1,
            userid: 1,
            title: "Site down".into(),
            message: "Help".into(),
            status: "Customer-Reply".into(),
            urgency: "High".into(),
            date: date("2024-02-02"),
            lastreply: date("2024-02-03"),
        }],
        ticket_replies: vec![
            TicketReply {
                id: 30,
                tid: 20,
                admin: "John".into(),
                date: date("2024-02-02"),
                message: "Looking".into(),
            },
            TicketReply {
                id: 31,
                tid: 20,
                admin: "".into(),
                date: date("2024-02-03"),
                message: "Thanks".into(),
            },
        ],
        announcements: vec![Announcement {
            id: 40,
            date: date("2024-01-05"),
            title: "Hello".into(),
            announcement: "<p>Hi</p>".into(),
            published: true,
        }],
        kb_categories: vec![KbCategory {
            id: 50,
            name: format!("Billing {tag}"),
            description: "".into(),
        }],
        kb_articles: vec![
            KbArticle {
                id: 60,
                categoryid: 50,
                title: format!("Pay {tag}"),
                article: "How to pay".into(),
                views: 5,
                private: false,
            },
            KbArticle {
                id: 61,
                categoryid: 0,
                title: format!("Orphan {tag}"),
                article: "x".into(),
                views: 0,
                private: true,
            },
        ],
        domains: vec![
            Domain {
                id: 70,
                userid: 1,
                type_: "Register".into(),
                domain: format!("ana-{tag}.com"),
                registrar: "Enom".into(),
                registrationperiod: 2,
                registrationdate: date("2024-01-01"),
                expirydate: date("2026-01-01"),
                status: "Active".into(),
                donotrenew: true,
                dnsmanagement: true,
                idprotection: false,
            },
            Domain {
                id: 71,
                userid: 1,
                type_: "Transfer".into(),
                domain: format!("{tag}.co.uk"),
                registrar: "".into(),
                registrationperiod: 1,
                status: "Pending Transfer".into(),
                ..Default::default()
            },
            Domain {
                id: 72,
                userid: 99,
                domain: "orphan.com".into(),
                ..Default::default()
            },
        ],
        config_groups: vec![ConfigGroup {
            id: 80,
            name: "VPS opts".into(),
            product_ids: vec![7],
        }],
        config_options: vec![
            ConfigOption {
                id: 81,
                gid: 80,
                optionname: "ram|RAM".into(),
                optiontype: 1,
                order: 1,
                ..Default::default()
            },
            ConfigOption {
                id: 82,
                gid: 80,
                optionname: "IPs".into(),
                optiontype: 4,
                qtyminimum: 1,
                qtymaximum: 8,
                order: 2,
                ..Default::default()
            },
            ConfigOption {
                id: 83,
                gid: 80,
                optionname: "Hidden".into(),
                optiontype: 1,
                hidden: true,
                ..Default::default()
            },
        ],
        config_subs: vec![
            ConfigSub {
                id: 91,
                configid: 81,
                optionname: "2gb|2 GB".into(),
                sortorder: 1,
                hidden: false,
            },
            ConfigSub {
                id: 92,
                configid: 81,
                optionname: "4 GB".into(),
                sortorder: 2,
                hidden: false,
            },
            ConfigSub {
                id: 93,
                configid: 81,
                optionname: "old".into(),
                sortorder: 3,
                hidden: true,
            },
            ConfigSub {
                id: 94,
                configid: 82,
                optionname: "IP".into(),
                sortorder: 1,
                hidden: false,
            },
        ],
        config_pricing: vec![Pricing {
            relid: 92,
            currency: 1,
            monthly: d("4.25"),
            quarterly: d("-1"),
            semiannually: d("-1"),
            annually: d("40.00"),
            biennially: d("-1"),
            triennially: d("-1"),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn import_is_exact_idempotent_and_dry_run_writes_nothing() {
    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/peltycms_test".into());
    let pool = pelty_database::connect(&url, 4).await.expect("connect");
    pelty_database::migrate(&pool).await.expect("migrate");
    // Unique tag so the test can share a DB with earlier runs.
    sqlx::query("DELETE FROM import_map WHERE source='whmcs'")
        .execute(&pool)
        .await
        .unwrap();
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let data = fixture(&tag);

    // dry run: report but no rows
    let rep = pelty_import::run(&pool, &data, true)
        .await
        .expect("dry run");
    assert_eq!(rep.entities["invoices"].created, 2);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM import_map")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "dry run must roll back");

    // real run
    let rep = pelty_import::run(&pool, &data, false)
        .await
        .expect("import");
    assert_eq!(rep.entities["customers"].created, 2);
    assert_eq!(rep.entities["users"].created, 1);
    assert_eq!(rep.entities["product_prices"].created, 2);
    assert_eq!(rep.entities["services"].created, 1);
    assert_eq!(rep.entities["payments"].created, 1);
    assert!(rep.warnings.iter().any(|w| w.contains("invalid email")));
    assert_eq!(rep.entities["client_groups"].created, 1);
    assert_eq!(rep.entities["addons"].created, 1);
    assert_eq!(rep.entities["service_addons"].created, 1);
    assert_eq!(rep.entities["custom_fields"].created, 2);
    assert_eq!(rep.entities["custom_values"].created, 2);
    assert_eq!(rep.entities["tickets"].created, 1);
    assert_eq!(rep.entities["ticket_replies"].created, 2);
    assert_eq!(rep.entities["announcements"].created, 1);
    assert_eq!(rep.entities["kb_articles"].created, 2);
    assert!(rep
        .warnings
        .iter()
        .any(|w| w.contains("client-level custom field")));
    let email = format!("ana-{tag}@example.com");
    let (msgs, staff): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE m.is_staff) FROM ticket_messages m \
         JOIN tickets t ON t.id=m.ticket_id JOIN customers c ON c.id=t.customer_id WHERE c.email=$1",
    ).bind(&email).fetch_one(&pool).await.unwrap();
    assert_eq!((msgs, staff), (3, 1));
    let cf: serde_json::Value = sqlx::query_scalar(
        "SELECT s.custom_fields FROM services s JOIN customers c ON c.id=s.customer_id WHERE c.email=$1",
    ).bind(&email).fetch_one(&pool).await.unwrap();
    let vals: Vec<&str> = cf
        .as_object()
        .unwrap()
        .values()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(vals.contains(&"srv-ana") && vals.contains(&"true"), "{cf}");

    let (num, status, total, tax, paid): (String, String, Decimal, Decimal, Decimal) =
        sqlx::query_as(
            "SELECT number, status, total, tax_total, amount_paid FROM invoices WHERE number=$1",
        )
        .bind(format!("W{tag}-100"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(num, format!("W{tag}-100"));
    assert_eq!(status, "paid");
    assert_eq!(total, d("5.94"));
    assert_eq!(tax, d("0.95"));
    assert_eq!(paid, d("5.94"));

    assert_eq!(rep.entities["client_groups"].created, 1);
    assert_eq!(rep.entities["addons"].created, 1);
    assert_eq!(rep.entities["service_addons"].created, 1);
    assert_eq!(rep.entities["custom_fields"].created, 2);
    assert_eq!(rep.entities["custom_values"].created, 2);
    assert_eq!(rep.entities["tickets"].created, 1);
    assert_eq!(rep.entities["ticket_replies"].created, 2);
    assert_eq!(rep.entities["announcements"].created, 1);
    assert_eq!(rep.entities["kb_articles"].created, 2);
    assert!(rep
        .warnings
        .iter()
        .any(|w| w.contains("client-level custom field")));
    let (msgs, staff): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE m.is_staff) FROM ticket_messages m JOIN tickets t ON t.id=m.ticket_id \
         JOIN customers c ON c.id=t.customer_id WHERE c.email=$1",
    ).bind(format!("ana-{tag}@example.com")).fetch_one(&pool).await.unwrap();
    assert_eq!((msgs, staff), (3, 1));
    let cf: serde_json::Value = sqlx::query_scalar(
        "SELECT s.custom_fields FROM services s JOIN customers c ON c.id=s.customer_id WHERE c.email=$1",
    ).bind(format!("ana-{tag}@example.com")).fetch_one(&pool).await.unwrap();
    let vals: Vec<&str> = cf
        .as_object()
        .unwrap()
        .values()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(vals.contains(&"srv-ana") && vals.contains(&"true"), "{cf}");
    let grp: Option<Uuid> =
        sqlx::query_scalar("SELECT client_group_id FROM customers WHERE email=$1")
            .bind(format!("ana-{tag}@example.com"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(grp.is_none(), "client 1 has no group in fixture");

    // imported login is locked: no valid PHC hash
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE email=$1")
        .bind(format!("ana-{tag}@example.com"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(hash, pelty_import::import::LOCKED_PASSWORD);

    // domains, configurable options, credit
    assert_eq!(rep.entities["domains"].created, 2);
    assert!(rep.warnings.iter().any(|w| w.contains("domain 72")));
    let (st, ar, years, tld): (String, bool, i32, String) =
        sqlx::query_as("SELECT status, auto_renew, years, tld FROM domains WHERE name=$1")
            .bind(format!("ana-{tag}.com"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (st.as_str(), ar, years, tld.as_str()),
        ("active", false, 2, "com")
    );
    let (st, kind, tld): (String, String, String) =
        sqlx::query_as("SELECT status, kind, tld FROM domains WHERE name=$1")
            .bind(format!("{tag}.co.uk"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (st.as_str(), kind.as_str(), tld.as_str()),
        ("transferring", "transfer", "co.uk")
    );
    assert_eq!(rep.entities["config_options"].created, 2);
    assert_eq!(rep.entities["config_choices"].created, 3);
    let opts: Vec<(String, String, i32, i32)> = sqlx::query_as(
        "SELECT o.name, o.option_type, o.qty_min, o.qty_max FROM product_config_options o JOIN products p ON p.id=o.product_id WHERE p.name=$1 ORDER BY o.sort_order",
    ).bind(format!("Starter {tag}")).fetch_all(&pool).await.unwrap();
    assert_eq!(
        opts,
        vec![
            ("RAM".into(), "dropdown".into(), 0, 100),
            ("IPs".into(), "quantity".into(), 1, 8)
        ]
    );
    let prices: Vec<(String, String, Decimal)> = sqlx::query_as(
        "SELECT ch.label, pr.cycle, pr.amount FROM product_config_prices pr JOIN product_config_choices ch ON ch.id=pr.choice_id \
         JOIN product_config_options o ON o.id=ch.option_id JOIN products p ON p.id=o.product_id WHERE p.name=$1 ORDER BY pr.cycle",
    ).bind(format!("Starter {tag}")).fetch_all(&pool).await.unwrap();
    assert_eq!(
        prices,
        vec![
            ("4 GB".into(), "annual".into(), d("40.00")),
            ("4 GB".into(), "monthly".into(), d("4.25"))
        ]
    );
    let credit: Decimal = sqlx::query_scalar("SELECT sum(l.amount) FROM credit_ledger l JOIN customers c ON c.id=l.customer_id WHERE c.email=$1")
        .bind(format!("ana-{tag}@example.com")).fetch_one(&pool).await.unwrap();
    assert_eq!(credit, d("12.50"));

    // re-run: nothing new
    let rep = pelty_import::run(&pool, &data, false)
        .await
        .expect("re-run");
    for (k, c) in &rep.entities {
        if *k != "currencies" {
            assert_eq!(c.created, 0, "{k} created on re-run");
        }
    }
    assert_eq!(rep.entities["invoices"].skipped_existing, 2);
    sqlx::query("DELETE FROM import_map WHERE source='whmcs'")
        .execute(&pool)
        .await
        .unwrap();
}
