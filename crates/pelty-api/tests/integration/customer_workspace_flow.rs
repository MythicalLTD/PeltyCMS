//! Real PostgreSQL coverage for workspace projections and permission-filtered activity.
use crate::common::*;
use axum::{http::StatusCode, Router};
use pelty_api::AppState;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const ROWS: usize = 53;
const SECTIONS: &[(&str, &str)] = &[
    ("contacts", ""),
    ("services", "service"),
    ("domains", "domain"),
    ("invoices", "invoice"),
    ("transactions", "payment"),
    ("credit_ledger", "credit"),
    ("tickets", "ticket"),
    ("notes", "note"),
    ("refunds", "refund"),
    ("chats", "chat"),
    ("emails", "email"),
    ("failures", "job"),
    ("log", "audit"),
];
const FINANCE: &[&str] = &["invoices", "transactions", "credit_ledger", "refunds"];
const SERVICES: &[&str] = &["services", "domains"];
const SUPPORT: &[&str] = &["tickets", "chats"];

struct Account {
    id: Uuid,
    owner: Uuid,
    ids: BTreeMap<&'static str, BTreeSet<String>>,
}

struct Fixture {
    app: Router,
    state: AppState,
    admin: Sess,
    accounts: Vec<Account>,
}

async fn fixture() -> Fixture {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let (product, _, _) = seed_product(&app, &admin).await;
    let product = Uuid::parse_str(&product).unwrap();
    let mut accounts = Vec::new();
    for _ in 0..2 {
        let customer = sess(&app, &state, "customer").await;
        let r = call(&app, "GET", "/api/v1/client/profile", None, &customer.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let id = Uuid::parse_str(r.body["id"].as_str().unwrap()).unwrap();
        let owner = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE email=$1")
            .bind(&customer.email)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE customers SET currency='EUR',created_at='2025-01-01T00:00:00Z' WHERE id=$1",
        )
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
        accounts.push(Account {
            id,
            owner,
            ids: BTreeMap::new(),
        });
    }
    // Every event has the same timestamp. Payments deliberately disagree with
    // their invoice's customer_id: invoice ownership must be authoritative.
    let inserts = [
        ("contacts", "INSERT INTO customer_contacts(id,customer_id,name,email,created_at) SELECT gen_random_uuid(),cid,'Workspace contact','contact@pelty.test',at FROM seed"),
        ("services", "INSERT INTO services(id,customer_id,product_id,status,cycle,currency,recurring_amount,module_data,provision_error,created_at) SELECT gen_random_uuid(),cid,pid,'active','monthly','EUR',10.1234,'{\"password\":\"workspace-secret\"}'::jsonb,'workspace-secret',at FROM seed"),
        ("domains", "INSERT INTO domains(id,customer_id,name,tld,kind,status,registrar,years,contact,last_error,created_at) SELECT gen_random_uuid(),cid,gen_random_uuid()::text||'.test','test','register','active','mock',1,'{\"secret\":\"workspace-secret\"}'::jsonb,'workspace-secret',at FROM seed"),
        ("invoices", "INSERT INTO invoices(id,customer_id,number,currency,status,total,amount_paid,finalized_at,due_at,created_at) SELECT gen_random_uuid(),cid,'workspace-'||gen_random_uuid()::text,'EUR','unpaid',10.1234,1.0001,at,at,at FROM seed"),
        ("transactions", "INSERT INTO payments(id,invoice_id,customer_id,gateway,amount,currency,status,idempotency_key,created_at) SELECT gen_random_uuid(),i.id,other,'mock',1.0001,'EUR','succeeded',gen_random_uuid()::text,at FROM (SELECT DISTINCT cid,other,at FROM seed) s JOIN invoices i ON i.customer_id=s.cid"),
        ("credit_ledger", "INSERT INTO credit_ledger(id,customer_id,currency,amount,kind,created_at) SELECT gen_random_uuid(),cid,'EUR',0.1001,'adjustment',at FROM seed"),
        ("tickets", "INSERT INTO tickets(customer_id,department_id,subject,created_at) SELECT cid,'00000000-0000-0000-0002-000000000001'::uuid,'Workspace ticket',at FROM seed"),
        ("notes", "INSERT INTO customer_notes(customer_id,author_id,body,created_at) SELECT cid,uid,'Workspace note',at FROM seed"),
        ("refunds", "INSERT INTO refunds(id,payment_id,invoice_id,customer_id,gateway,amount,currency,status,idempotency_key,error,created_at) SELECT gen_random_uuid(),p.id,p.invoice_id,cid,'mock',0.1001,'EUR','succeeded',gen_random_uuid()::text,'workspace-secret',at FROM (SELECT DISTINCT cid,at FROM seed) s JOIN invoices i ON i.customer_id=s.cid JOIN payments p ON p.invoice_id=i.id"),
        ("chats", "INSERT INTO chat_sessions(id,token_hash,customer_id,name,email,created_at) SELECT gen_random_uuid(),decode(replace(gen_random_uuid()::text,'-',''),'hex'),cid,'Linked visitor','different@pelty.test',at FROM seed"),
        ("emails", "INSERT INTO email_log(user_id,recipient,template,locale,subject,body_text,status,error,mailer,created_at) SELECT uid,'recipient@pelty.test','workspace','en','Workspace email','workspace-secret','failed','workspace-secret','mock',at FROM seed"),
        ("failures", "INSERT INTO jobs(kind,payload,status,result,last_error,created_at) SELECT 'provision',jsonb_build_object('service_id',s.id,'customer_id',other,'password','workspace-secret'),'failed','{\"secret\":\"workspace-secret\"}'::jsonb,'workspace-secret',at FROM (SELECT DISTINCT cid,other,at FROM seed) x JOIN services s ON s.customer_id=x.cid"),
        ("log", "INSERT INTO audit_logs(actor_id,action,target,metadata,created_at) SELECT uid,'workspace.test',cid::text,'{\"secret\":\"workspace-secret\"}'::jsonb,at FROM seed"),
    ];
    for index in 0..2 {
        let other = accounts[1 - index].id;
        let account = &mut accounts[index];
        for (section, insert) in inserts {
            let sql = format!("WITH seed AS (SELECT $1::uuid cid,$2::uuid uid,$3::uuid pid,$4::uuid other,'2025-01-01T00:00:00Z'::timestamptz at FROM generate_series(1,53)) {insert} RETURNING id::text");
            let ids: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .bind(account.id)
                .bind(account.owner)
                .bind(product)
                .bind(other)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_else(|e| panic!("seed {section}: {e}"));
            assert_eq!(ids.len(), ROWS, "seed {section}");
            account.ids.insert(section, ids.into_iter().collect());
        }
        let invoice: String = sqlx::query_scalar("INSERT INTO invoices(id,customer_id,number,currency,status,total,amount_paid,finalized_at,due_at,created_at) VALUES(gen_random_uuid(),$1,$2,'USD','unpaid',7.0002,2.0001,'2025-01-01T00:00:00Z','2025-01-01T00:00:00Z','2025-01-01T00:00:00Z') RETURNING id::text")
            .bind(account.id).bind(format!("workspace-usd-{}", account.id))
            .fetch_one(&state.pool).await.unwrap();
        account.ids.get_mut("invoices").unwrap().insert(invoice);
        let credit: String = sqlx::query_scalar("INSERT INTO credit_ledger(id,customer_id,currency,amount,kind,created_at) VALUES(gen_random_uuid(),$1,'USD',0.2002,'adjustment','2025-01-01T00:00:00Z') RETURNING id::text")
            .bind(account.id).fetch_one(&state.pool).await.unwrap();
        account.ids.get_mut("credit_ledger").unwrap().insert(credit);
        // An anonymous visitor's matching email is not verified ownership.
        sqlx::query("INSERT INTO chat_sessions(id,token_hash,name,email,created_at) SELECT gen_random_uuid(),decode(replace(gen_random_uuid()::text,'-',''),'hex'),'Unverified',email,'2025-01-02T00:00:00Z' FROM customers WHERE id=$1")
            .bind(account.id).execute(&state.pool).await.unwrap();
        // Matching recipients and actors are not ownership links either.
        sqlx::query("INSERT INTO email_log(recipient,template,locale,subject,body_text,status,mailer,created_at) SELECT email,'workspace','en','Unverified recipient','workspace-secret','sent','mock','2025-01-02T00:00:00Z' FROM customers WHERE id=$1")
            .bind(account.id).execute(&state.pool).await.unwrap();
        sqlx::query("INSERT INTO audit_logs(actor_id,action,target,metadata,created_at) VALUES($1,'workspace.unrelated',gen_random_uuid()::text,'{\"secret\":\"workspace-secret\"}'::jsonb,'2025-01-02T00:00:00Z')")
            .bind(account.owner).execute(&state.pool).await.unwrap();
        // Malformed/unrelated payloads must neither crash nor be customer matches.
        for payload in [
            json!({"customer_id": account.id, "service_id": "not-a-uuid"}),
            json!({"customer_id": account.id}),
        ] {
            sqlx::query("INSERT INTO jobs(kind,payload,status,last_error,created_at) VALUES('provision',$1,'failed','workspace-secret','2025-01-02T00:00:00Z')")
                .bind(payload).execute(&state.pool).await.unwrap();
        }
    }
    // A refund's direct customer alone cannot override foreign invoice ownership.
    sqlx::query("INSERT INTO refunds(id,payment_id,invoice_id,customer_id,gateway,amount,currency,status,idempotency_key,created_at) SELECT gen_random_uuid(),p.id,p.invoice_id,$1,'mock',0.1001,'EUR','failed',gen_random_uuid()::text,'2025-01-02T00:00:00Z' FROM payments p JOIN invoices i ON i.id=p.invoice_id WHERE i.customer_id=$2 LIMIT 1")
        .bind(accounts[0].id).bind(accounts[1].id).execute(&state.pool).await.unwrap();
    Fixture {
        app,
        state,
        admin,
        accounts,
    }
}

async fn get(app: &Router, id: Uuid, suffix: &str, headers: &[(&str, &str)]) -> Value {
    let r = call(
        app,
        "GET",
        &format!("/api/v1/admin/customers/{id}/{suffix}"),
        None,
        headers,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{suffix}: {:?}", r.body);
    r.body
}

async fn reader(f: &Fixture, extra: &[&str]) -> Sess {
    let role = format!("ws_{}", Uuid::new_v4().simple());
    let mut permissions = vec!["admin.access", "customers.read"];
    permissions.extend_from_slice(extra);
    let r = call(
        &f.app,
        "POST",
        "/api/v1/admin/roles",
        Some(json!({"name": role, "permissions": permissions})),
        &f.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    sess(&f.app, &f.state, &role).await
}

async fn key(f: &Fixture, scopes: &[&str]) -> String {
    let r = call(
        &f.app,
        "POST",
        "/api/v1/api-keys",
        Some(json!({"name": "workspace reader", "scopes": scopes})),
        &f.admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    format!("Bearer {}", r.body["secret"].as_str().unwrap())
}

fn ids(items: &Value) -> BTreeSet<String> {
    items
        .as_array()
        .expect("items array")
        .iter()
        .map(|v| v["id"].as_str().expect("string id").to_owned())
        .collect()
}

fn assert_hidden(v: &Value, forbidden: &[&str]) {
    for section in forbidden {
        assert_eq!(v[*section], json!([]), "{section}: {v}");
        assert_eq!(v["totals"][*section], Value::Null, "{section}");
        assert_eq!(
            v["meta"][*section],
            json!({"total": null, "returned": 0, "truncated": false, "accessible": false}),
            "{section}"
        );
    }
    if forbidden.contains(&"invoices") {
        assert_eq!(v["balances"], json!([]));
        assert!(!v["attention"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "invoice"));
    }
    for (section, counts) in [
        ("services", &["services_total", "services_active"][..]),
        ("domains", &["domains_total"][..]),
        ("tickets", &["tickets_open"][..]),
    ] {
        if forbidden.contains(&section) {
            for count in counts {
                assert_eq!(v["counts"][*count], Value::Null, "{count}");
                if let Some(stats) = v.get("stats") {
                    assert_eq!(stats[*count], Value::Null, "legacy {count}");
                }
            }
        }
    }
    for (section, kind) in [("tickets", "ticket"), ("failures", "job")] {
        if forbidden.contains(&section) {
            assert!(!v["attention"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["kind"] == kind));
        }
    }
    if forbidden.contains(&"invoices") && v.get("stats").is_some() {
        for field in ["invoices_unpaid", "amount_due", "amount_paid", "credit"] {
            assert_eq!(v["stats"][field], Value::Null, "legacy {field}");
        }
    }
}

// Encode the entire opaque cursor, including quotes, colons, plus signs and spaces.
fn encode(value: &str) -> String {
    value.bytes().map(|b| format!("%{b:02X}")).collect()
}

async fn activity(
    app: &Router,
    id: Uuid,
    kind: &str,
    limit: usize,
    headers: &[(&str, &str)],
) -> Vec<Value> {
    let mut result = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = BTreeSet::new();
    for _ in 0..100 {
        let mut suffix = format!("activity?kind={kind}&limit={limit}");
        if let Some(ref before) = cursor {
            suffix.push_str(&format!("&before={}", encode(before)));
        }
        let page = get(app, id, &suffix, headers).await;
        let items = page["items"].as_array().unwrap();
        assert!(items.len() <= limit);
        for item in items {
            assert!(
                seen.insert(item["id"].as_str().unwrap().to_owned()),
                "duplicate across pages: {item}"
            );
        }
        result.extend(items.iter().cloned());
        match page["next_cursor"].as_str() {
            None => return result,
            Some(next) => {
                assert_eq!(items.len(), limit, "short page with next cursor");
                assert!(!items.is_empty());
                cursor = Some(next.to_owned());
            }
        }
    }
    panic!("activity cursor did not terminate");
}

#[tokio::test]
async fn authentication_permissions_and_narrow_api_keys_do_not_leak_sections() {
    let f = fixture().await;
    let id = f.accounts[0].id;
    let customer = sess(&f.app, &f.state, "customer").await;
    for endpoint in ["workspace", "activity", "summary"] {
        let uri = format!("/api/v1/admin/customers/{id}/{endpoint}");
        assert_eq!(
            call(&f.app, "GET", &uri, None, &[]).await.status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&f.app, "GET", &uri, None, &customer.h()).await.status,
            StatusCode::FORBIDDEN
        );
        let missing = format!("/api/v1/admin/customers/{}/{endpoint}", Uuid::new_v4());
        assert_eq!(
            call(&f.app, "GET", &missing, None, &f.admin.h())
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }
    let ro = reader(&f, &[]).await;
    let bearer = key(&f, &["admin.access", "customers.read"]).await;
    for headers in [ro.h().to_vec(), vec![("authorization", bearer.as_str())]] {
        let forbidden = [FINANCE, SERVICES, SUPPORT, &["failures", "log"]].concat();
        for endpoint in ["workspace", "summary"] {
            let v = get(&f.app, id, endpoint, &headers).await;
            assert_eq!(v["customer"]["id"], id.to_string());
            assert_eq!(
                v["access"],
                json!({"finance": false, "services": false, "support": false, "operations": false, "audit": false, "manage_customer": false})
            );
            assert_hidden(&v, &forbidden);
            for section in ["contacts", "notes", "emails"] {
                assert_eq!(v["totals"][section], ROWS);
                assert_eq!(v[section].as_array().unwrap().len(), 50);
            }
            assert!(!v.to_string().contains("workspace-secret"));
        }
        let items = activity(&f.app, id, "all", 7, &headers).await;
        assert_eq!(items.len(), 2 * ROWS + 1);
        assert!(items
            .iter()
            .all(|v| matches!(v["kind"].as_str(), Some("note" | "email" | "customer"))));
        for (_, kind) in SECTIONS
            .iter()
            .filter(|(section, kind)| !kind.is_empty() && forbidden.contains(section))
        {
            let v = get(
                &f.app,
                id,
                &format!("activity?kind={kind}&limit=1"),
                &headers,
            )
            .await;
            assert_eq!(
                v,
                json!({"items": [], "next_cursor": null}),
                "forbidden {kind}"
            );
        }
        // All forbidden rows are as new as or newer than these allowed rows:
        // permission filtering must precede LIMIT, not yield a short/empty page.
        let page = get(&f.app, id, "activity?limit=100", &headers).await;
        assert_eq!(page["items"].as_array().unwrap().len(), 100);
        assert!(page["next_cursor"].is_string());
    }
    let no_customer_scope = key(&f, &["admin.access", "invoices.read"]).await;
    assert_eq!(
        call(
            &f.app,
            "GET",
            &format!("/api/v1/admin/customers/{id}/workspace"),
            None,
            &[("authorization", no_customer_scope.as_str())]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn per_resource_read_permissions_and_jobs_require_both_scopes() {
    let f = fixture().await;
    let id = f.accounts[0].id;
    for (permissions, allowed) in [
        (vec!["invoices.read"], FINANCE.to_vec()),
        (vec!["services.read"], SERVICES.to_vec()),
        (vec!["tickets.read"], SUPPORT.to_vec()),
        (vec!["audit.read"], vec!["log"]),
        (vec!["jobs.manage"], vec![]),
        (
            vec!["services.read", "jobs.manage"],
            vec!["services", "domains", "failures"],
        ),
    ] {
        let ro = reader(&f, &permissions).await;
        let mut scopes = vec!["admin.access", "customers.read"];
        scopes.extend_from_slice(&permissions);
        let bearer = key(&f, &scopes).await;
        for headers in [ro.h().to_vec(), vec![("authorization", bearer.as_str())]] {
            let forbidden: Vec<_> = SECTIONS
                .iter()
                .map(|(s, _)| *s)
                .filter(|s| !["contacts", "notes", "emails"].contains(s) && !allowed.contains(s))
                .collect();
            for endpoint in ["workspace", "summary"] {
                let v = get(&f.app, id, endpoint, &headers).await;
                assert_hidden(&v, &forbidden);
                for section in &allowed {
                    assert_eq!(
                        v["totals"][*section],
                        f.accounts[0].ids[*section].len(),
                        "{section}"
                    );
                    assert_eq!(v["meta"][*section]["accessible"], true);
                    assert_eq!(v[*section].as_array().unwrap().len(), 50);
                }
                assert!(!v.to_string().contains("workspace-secret"));
            }
            let items = activity(&f.app, id, "all", 100, &headers).await;
            let expected_kinds: BTreeSet<_> = SECTIONS
                .iter()
                .filter(|(s, k)| {
                    !k.is_empty() && (allowed.contains(s) || ["notes", "emails"].contains(s))
                })
                .map(|(_, k)| *k)
                .chain(std::iter::once("customer"))
                .collect();
            assert_eq!(
                items
                    .iter()
                    .map(|v| v["kind"].as_str().unwrap())
                    .collect::<BTreeSet<_>>(),
                expected_kinds
            );
            assert!(!json!(items).to_string().contains("workspace-secret"));
        }
    }
}

#[tokio::test]
async fn snapshots_are_capped_owned_and_currency_exact_with_all_records_reachable() {
    let f = fixture().await;
    for account in &f.accounts {
        let v = get(&f.app, account.id, "workspace", &f.admin.h()).await;
        assert_eq!(v["limits"]["records"], 50);
        for (section, _) in SECTIONS {
            let expected = &account.ids[section];
            assert_eq!(v[*section].as_array().unwrap().len(), 50, "{section}");
            assert_eq!(v["totals"][*section], expected.len(), "{section}");
            assert_eq!(
                v["meta"][*section],
                json!({"total": expected.len(), "returned": 50, "truncated": true, "accessible": true}),
                "{section}"
            );
            assert!(
                ids(&v[*section]).is_subset(expected),
                "foreign {section}: {}",
                v[*section]
            );
        }
        assert_eq!(
            v["counts"],
            json!({"services_active": ROWS, "services_total": ROWS, "domains_total": ROWS, "tickets_open": ROWS})
        );
        assert_eq!(
            v["balances"],
            json!([
                {"currency": "EUR", "amount_due": "483.5349", "amount_paid": "53.0053", "credit": "5.3053", "invoices_unpaid": 53},
                {"currency": "USD", "amount_due": "5.0001", "amount_paid": "2.0001", "credit": "0.2002", "invoices_unpaid": 1}
            ])
        );
        assert!(!v.to_string().contains("workspace-secret"));
        for job in v["failures"].as_array().unwrap() {
            assert_eq!(
                job.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from(["id", "kind", "status", "service_id", "created_at"])
            );
            assert!(account.ids["services"].contains(job["service_id"].as_str().unwrap()));
        }
        let legacy = get(&f.app, account.id, "summary", &f.admin.h()).await;
        assert_eq!(legacy["legacy_currency"], "EUR");
        assert_eq!(legacy["stats"]["currency"], "EUR");
        for field in ["amount_due", "amount_paid", "credit", "invoices_unpaid"] {
            assert_eq!(
                legacy["stats"][field], v["balances"][0][field],
                "legacy must not sum currencies: {field}"
            );
        }
        for (section, _) in SECTIONS {
            assert_eq!(legacy[*section], v[*section], "legacy {section}");
        }
        for (section, kind) in SECTIONS.iter().filter(|(_, k)| !k.is_empty()) {
            let items = activity(&f.app, account.id, kind, 7, &f.admin.h()).await;
            let expected: BTreeSet<_> = account.ids[section]
                .iter()
                .map(|id| format!("{kind}:{id}"))
                .collect();
            assert_eq!(items.len(), expected.len(), "{kind}");
            assert_eq!(ids(&json!(items)), expected, "missing/foreign {kind}");
        }
        // Contacts have no activity kind; the existing account read is the
        // escape hatch for the three contacts beyond the bounded snapshot.
        let detail = call(
            &f.app,
            "GET",
            &format!("/api/v1/admin/customers/{}", account.id),
            None,
            &f.admin.h(),
        )
        .await;
        assert_eq!(detail.status, StatusCode::OK, "{:?}", detail.body);
        assert_eq!(ids(&detail.body["contacts"]), account.ids["contacts"]);
        let all = activity(&f.app, account.id, "all", 17, &f.admin.h()).await;
        let expected: BTreeSet<_> = SECTIONS
            .iter()
            .filter(|(_, k)| !k.is_empty())
            .flat_map(|(s, k)| account.ids[s].iter().map(move |id| format!("{k}:{id}")))
            .chain(std::iter::once(format!("customer:{}", account.id)))
            .collect();
        assert_eq!(ids(&json!(all)), expected);
        assert_eq!(all.len(), expected.len());
        for item in &all {
            assert_eq!(
                item.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from(["id", "kind", "occurred_at", "title", "status", "href"])
            );
            let (kind, resource_id) = item["id"].as_str().unwrap().split_once(':').unwrap();
            assert_eq!(item["kind"], kind);
            if kind == "customer" {
                assert_eq!(resource_id, account.id.to_string());
            } else {
                let (section, _) = SECTIONS.iter().find(|(_, k)| *k == kind).unwrap();
                assert!(
                    account.ids[section].contains(resource_id),
                    "foreign link: {item}"
                );
            }
            let expected_href = match kind {
                "invoice" => format!("/admin/invoices/{resource_id}"),
                "domain" => format!("/admin/domains/{resource_id}"),
                "ticket" => format!("/admin/tickets/{resource_id}"),
                "service" => format!("/admin/services?q={resource_id}"),
                _ => {
                    let tab = match kind {
                        "payment" | "refund" | "credit" => "transactions",
                        "chat" => "chats",
                        "email" => "emails",
                        "note" => "notes",
                        "audit" => "log",
                        "job" => "operations",
                        "customer" => "summary",
                        _ => panic!("unexpected activity kind: {kind}"),
                    };
                    format!("/admin/customers/{}?tab={tab}", account.id)
                }
            };
            assert_eq!(
                item["href"], expected_href,
                "wrong owned-resource link: {item}"
            );
        }
        for pair in all.windows(2) {
            assert_eq!(
                pair[0]["occurred_at"], pair[1]["occurred_at"],
                "fixture timestamps must tie"
            );
            assert!(
                pair[0]["id"].as_str().unwrap() > pair[1]["id"].as_str().unwrap(),
                "tie-breaker must descend"
            );
        }
        assert_eq!(
            all,
            activity(&f.app, account.id, "all", 100, &f.admin.h()).await,
            "page size must not change ordering"
        );
    }
}

async fn small_workspace_fixture() -> (Router, AppState, Sess, [Uuid; 2]) {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let customers = [Uuid::new_v4(), Uuid::new_v4()];
    for id in customers {
        sqlx::query("INSERT INTO customers(id,email,currency) VALUES($1,$2,'EUR')")
            .bind(id)
            .bind(email())
            .execute(&state.pool)
            .await
            .unwrap();
    }
    (app, state, admin, customers)
}

#[tokio::test]
async fn older_pinned_notes_survive_snapshot_limit_without_reordering_activity() {
    let (app, state, admin, [customer, _]) = small_workspace_fixture().await;
    let unpinned: Vec<String> = sqlx::query_scalar("INSERT INTO customer_notes(customer_id,body,sticky,created_at) SELECT $1,'Newer unpinned note',false,'2025-01-01T00:00:00Z' FROM generate_series(1,51) RETURNING id::text")
        .bind(customer).fetch_all(&state.pool).await.unwrap();
    let mut pinned = Vec::new();
    for date in ["2024-01-01T00:00:00Z", "2023-01-01T00:00:00Z"] {
        let id: String = sqlx::query_scalar("INSERT INTO customer_notes(customer_id,body,sticky,created_at) VALUES($1,'Older pinned note',true,$2::text::timestamptz) RETURNING id::text")
            .bind(customer).bind(date).fetch_one(&state.pool).await.unwrap();
        pinned.push(id);
    }
    for endpoint in ["workspace", "summary"] {
        let v = get(&app, customer, endpoint, &admin.h()).await;
        assert_eq!(v["totals"]["notes"], 53);
        assert_eq!(
            v["meta"]["notes"],
            json!({"total": 53, "returned": 50, "truncated": true, "accessible": true})
        );
        let notes = v["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 50);
        assert_eq!(notes[0]["id"], pinned[0]);
        assert_eq!(notes[1]["id"], pinned[1]);
        assert!(notes[..2].iter().all(|note| note["sticky"] == true));
        let mut newest = unpinned.clone();
        newest.sort_by(|a, b| b.cmp(a));
        let returned: Vec<_> = notes[2..]
            .iter()
            .map(|note| {
                assert_eq!(note["sticky"], false);
                note["id"].as_str().unwrap().to_owned()
            })
            .collect();
        assert_eq!(returned, newest[..48]);
    }
    let events = activity(&app, customer, "note", 7, &admin.h()).await;
    let expected: BTreeSet<_> = unpinned
        .iter()
        .chain(&pinned)
        .map(|id| format!("note:{id}"))
        .collect();
    assert_eq!(events.len(), 53);
    assert_eq!(ids(&json!(events)), expected);
    assert_eq!(events[51]["id"], format!("note:{}", pinned[0]));
    assert_eq!(events[52]["id"], format!("note:{}", pinned[1]));
    for event in events {
        assert_eq!(
            event["href"],
            format!("/admin/customers/{customer}?tab=notes")
        );
    }
}

#[tokio::test]
async fn queued_provisioning_retries_require_prior_failure_and_owned_service() {
    let (app, state, admin, customers) = small_workspace_fixture().await;
    let (product, _, _) = seed_product(&app, &admin).await;
    let mut services = Vec::new();
    for customer in customers {
        let service: Uuid = sqlx::query_scalar("INSERT INTO services(id,customer_id,product_id,status,cycle,currency,recurring_amount) VALUES(gen_random_uuid(),$1,$2::text::uuid,'active','monthly','EUR',1) RETURNING id")
            .bind(customer).bind(&product).fetch_one(&state.pool).await.unwrap();
        services.push(service);
    }
    let mut expected = BTreeSet::new();
    for (kind, status, attempts, error, service, visible) in [
        (
            "provision",
            "failed",
            1,
            Some("workspace-secret"),
            services[0],
            true,
        ),
        (
            "provision",
            "dead",
            5,
            Some("workspace-secret"),
            services[0],
            true,
        ),
        (
            "provision",
            "queued",
            1,
            Some("workspace-secret"),
            services[0],
            true,
        ),
        ("provision", "queued", 0, None, services[0], false),
        (
            "provision",
            "queued",
            0,
            Some("workspace-secret"),
            services[0],
            false,
        ),
        ("provision", "queued", 1, None, services[0], false),
        ("provision", "queued", 1, Some(""), services[0], false),
        (
            "provision",
            "running",
            1,
            Some("workspace-secret"),
            services[0],
            false,
        ),
        (
            "provision",
            "succeeded",
            1,
            Some("workspace-secret"),
            services[0],
            false,
        ),
        (
            "email",
            "queued",
            1,
            Some("workspace-secret"),
            services[0],
            false,
        ),
        (
            "provision",
            "queued",
            1,
            Some("workspace-secret"),
            services[1],
            false,
        ),
    ] {
        // The direct customer_id intentionally claims this account even for
        // the foreign service; only the linked service establishes ownership.
        let id: String = sqlx::query_scalar("INSERT INTO jobs(kind,payload,status,attempts,last_error,result) VALUES($1,$2,$3,$4,$5,$6) RETURNING id::text")
            .bind(kind)
            .bind(json!({"service_id": service, "customer_id": customers[0], "password": "workspace-secret"}))
            .bind(status).bind(attempts).bind(error)
            .bind(json!({"secret": "workspace-secret"}))
            .fetch_one(&state.pool).await.unwrap();
        if visible {
            expected.insert(id);
        }
    }
    for endpoint in ["workspace", "summary"] {
        let v = get(&app, customers[0], endpoint, &admin.h()).await;
        assert_eq!(ids(&v["failures"]), expected);
        assert_eq!(v["totals"]["failures"], 3);
        assert_eq!(
            v["meta"]["failures"],
            json!({"total": 3, "returned": 3, "truncated": false, "accessible": true})
        );
        assert_eq!(v["attention"], json!([{"kind": "job", "count": 3}]));
        for job in v["failures"].as_array().unwrap() {
            assert_eq!(job["service_id"], services[0].to_string());
            assert_eq!(
                job.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from(["id", "kind", "status", "service_id", "created_at"])
            );
        }
        assert!(!v.to_string().contains("workspace-secret"));
    }
    let events = activity(&app, customers[0], "job", 1, &admin.h()).await;
    assert_eq!(events.len(), 3);
    assert_eq!(
        ids(&json!(events)),
        expected.iter().map(|id| format!("job:{id}")).collect()
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event["status"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["failed", "dead", "queued"])
    );
    assert!(!json!(events).to_string().contains("workspace-secret"));
    for event in events {
        assert_eq!(
            event["href"],
            format!("/admin/customers/{}?tab=operations", customers[0])
        );
    }
}

#[tokio::test]
async fn service_uuid_deep_links_filter_exactly_and_preserve_pagination_totals() {
    let f = fixture().await;
    let ro = reader(&f, &["services.read"]).await;
    let bearer = key(&f, &["admin.access", "customers.read", "services.read"]).await;
    for headers in [ro.h().to_vec(), vec![("authorization", bearer.as_str())]] {
        for account in &f.accounts {
            let service_id = account.ids["services"].iter().next().unwrap();
            for query in [
                format!("q={service_id}"),
                format!("q={service_id}&status=active"),
            ] {
                let r = call(
                    &f.app,
                    "GET",
                    &format!("/api/v1/admin/services?{query}"),
                    None,
                    &headers,
                )
                .await;
                assert_eq!(r.status, StatusCode::OK, "{query}: {:?}", r.body);
                assert_eq!(r.body["total"], 1, "{query}: {:?}", r.body);
                assert_eq!(r.body["items"].as_array().unwrap().len(), 1);
                assert_eq!(r.body["items"][0]["id"], service_id.as_str());
                assert_eq!(r.body["items"][0]["customer_id"], account.id.to_string());
            }
            for (query, total) in [
                (format!("q={service_id}&status=suspended"), 0),
                (format!("q={service_id}&page=2&per_page=1"), 1),
                (format!("q={}&page=1&per_page=1", Uuid::new_v4()), 0),
            ] {
                let r = call(
                    &f.app,
                    "GET",
                    &format!("/api/v1/admin/services?{query}"),
                    None,
                    &headers,
                )
                .await;
                assert_eq!(r.status, StatusCode::OK, "{query}: {:?}", r.body);
                assert_eq!(r.body["total"], total, "{query}: {:?}", r.body);
                assert_eq!(r.body["items"], json!([]), "{query}: {:?}", r.body);
            }
        }
    }
}

#[tokio::test]
async fn cursors_are_customer_and_filter_bound_and_invalid_inputs_are_422() {
    let f = fixture().await;
    let id = f.accounts[0].id;
    let page = get(&f.app, id, "activity?kind=invoice&limit=1", &f.admin.h()).await;
    let cursor = page["next_cursor"].as_str().expect("second invoice page");
    let again = get(&f.app, id, "activity?kind=invoice&limit=1", &f.admin.h()).await;
    assert_eq!(page, again, "same snapshot position must be stable");
    let mut invalid = vec![
        (id, "activity?limit=0".to_owned()),
        (id, "activity?limit=-1".to_owned()),
        (id, "activity?limit=101".to_owned()),
        (id, "activity?kind=unknown".to_owned()),
        (id, "activity?before=not-json".to_owned()),
        (id, format!("activity?before={}", encode(&"x".repeat(513)))),
        (
            id,
            format!("activity?kind=payment&before={}", encode(cursor)),
        ),
        (id, format!("activity?before={}", encode(cursor))),
        (
            f.accounts[1].id,
            format!("activity?kind=invoice&before={}", encode(cursor)),
        ),
    ];
    let valid: Value = serde_json::from_str(cursor).unwrap();
    for (field, value) in [
        ("at", json!("invalid-time")),
        ("key", json!("invoice:")),
        ("key", json!("invoice:not-a-key")),
        ("key", json!("unknown:123")),
        ("key", json!("all:123")),
        ("key", json!(format!("invoice:{}", "a".repeat(41)))),
        ("unexpected", json!(true)),
    ] {
        let mut malformed = valid.clone();
        malformed[field] = value;
        invalid.push((
            id,
            format!(
                "activity?kind=invoice&before={}",
                encode(&malformed.to_string())
            ),
        ));
    }
    invalid.push((id, format!("activity?kind=invoice&before={}", encode("{}"))));
    for (customer, suffix) in invalid {
        let r = call(
            &f.app,
            "GET",
            &format!("/api/v1/admin/customers/{customer}/{suffix}"),
            None,
            &f.admin.h(),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{suffix}: {:?}",
            r.body
        );
    }
    // Valid boundary sizes and changing page size while retaining the filter.
    let next = get(
        &f.app,
        id,
        &format!("activity?kind=invoice&limit=100&before={}", encode(cursor)),
        &f.admin.h(),
    )
    .await;
    assert_eq!(next["items"].as_array().unwrap().len(), 53);
    assert!(next["next_cursor"].is_null());
    assert!(!ids(&next["items"]).contains(page["items"][0]["id"].as_str().unwrap()));
    let ro = reader(&f, &[]).await;
    let narrowed = get(
        &f.app,
        id,
        &format!("activity?kind=invoice&limit=1&before={}", encode(cursor)),
        &ro.h(),
    )
    .await;
    assert_eq!(
        narrowed,
        json!({"items": [], "next_cursor": null}),
        "cursor must not confer its creator's permissions"
    );
}
