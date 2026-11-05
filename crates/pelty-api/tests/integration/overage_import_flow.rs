//! Overage billing from server-module usage + server account import, against
//! a local fake panel driven through the `webhook` module.
use crate::common;
use axum::http::StatusCode;
use axum::routing::get;
use common::*;
use serde_json::json;

async fn fake_panel() -> String {
    let r = axum::Router::new()
        .route("/accounts", get(|| async { axum::Json(json!({"accounts": [{"id": "acc-1", "username": "alice", "domain": "alice.test", "package": "basic"}, {"remote_id": 42, "username": "bob"}, {"username": "no-id"}]})) }))
        .route("/usage/{rid}", get(|| async { axum::Json(json!({"disk_mb": 3072, "bw": {"gb": "12.5"}})) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn import_accounts_and_sample_module_usage() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let base = fake_panel().await;
    let actions = json!({"test": {"method": "GET", "path": "/accounts"}, "list_accounts": {"method": "GET", "path": "/accounts"}, "usage": {"method": "GET", "path": "/usage/{{remote_id}}"}});
    let srv = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": "Fake", "module": "webhook", "base_url": base, "api_key": "k", "config": {"actions": actions}})), &admin.h()).await;
    assert_eq!(srv.status, StatusCode::OK, "{:?}", srv.body);
    let sid = srv.body["id"].as_str().unwrap().to_string();

    // A server whose module can't list accounts → 409, not a crash.
    let lic = call(&app, "POST", "/api/v1/admin/servers", Some(json!({"name": "L", "module": "licensing", "base_url": "https://x.example.com", "api_key": "lic-signing-secret-0123456789"})), &admin.h()).await.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/servers/{lic}/accounts"), None, &admin.h()).await.status, StatusCode::CONFLICT);

    let acc = call(&app, "GET", &format!("/api/v1/admin/servers/{sid}/accounts"), None, &admin.h()).await;
    assert_eq!(acc.status, StatusCode::OK, "{:?}", acc.body);
    let items = acc.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "account without id is dropped");
    assert_eq!(items[0]["remote_id"], "acc-1");
    assert_eq!(items[1]["remote_id"], "42");
    assert!(items[0]["linked_service_id"].is_null());
    assert_eq!(call(&app, "GET", &format!("/api/v1/admin/servers/{sid}/accounts"), None, &cust.h()).await.status, StatusCode::FORBIDDEN);

    let (pid, _, _) = seed_product(&app, &admin).await;
    let (other_pid, _, _) = seed_product(&app, &admin).await;
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/products/{pid}/module"), Some(json!({"module": "webhook", "server_id": sid})), &admin.h()).await.status, StatusCode::OK);
    call(&app, "PUT", "/api/v1/client/profile", Some(json!({"city": "Cluj"})), &cust.h()).await; // materialise the customer row
    let cid: uuid::Uuid = sqlx::query_scalar("SELECT c.id FROM customers c JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1").bind(&cust.email).fetch_one(&st.pool).await.unwrap();

    let due = (time::OffsetDateTime::now_utc().date() + time::Duration::days(15)).to_string();
    let item = |rid: &str, p: &str| json!({"remote_id": rid, "customer_id": cid, "product_id": p, "cycle": "monthly", "recurring_amount": "9.99", "next_due_date": due});
    let imp = call(&app, "POST", &format!("/api/v1/admin/servers/{sid}/import"), Some(json!({"items": [item("acc-1", &pid), item("ghost", &pid), item("42", &other_pid)]})), &admin.h()).await;
    assert_eq!(imp.status, StatusCode::OK, "{:?}", imp.body);
    assert_eq!(imp.body["imported"], 1);
    let res = imp.body["results"].as_array().unwrap();
    assert_eq!(res[1]["error"], "error.import_unknown_account");
    assert_eq!(res[2]["error"], "error.import_product_module");
    let svc = uuid::Uuid::parse_str(res[0]["service_id"].as_str().unwrap()).unwrap();
    let row: (String, String, String, String, rust_decimal::Decimal) = sqlx::query_as("SELECT status, provision_status, remote_id, currency::text, recurring_amount FROM services WHERE id = $1").bind(svc).fetch_one(&st.pool).await.unwrap();
    assert_eq!((row.0.as_str(), row.1.as_str(), row.2.as_str()), ("active", "ok", "acc-1"));
    assert_eq!(row.4, rust_decimal::Decimal::new(999, 2));
    // Second import of the same account conflicts; listing shows the link.
    let again = call(&app, "POST", &format!("/api/v1/admin/servers/{sid}/import"), Some(json!({"items": [item("acc-1", &pid)]})), &admin.h()).await;
    assert_eq!(again.body["results"][0]["error"], "error.import_already_linked");
    let acc = call(&app, "GET", &format!("/api/v1/admin/servers/{sid}/accounts"), None, &admin.h()).await;
    assert_eq!(acc.body["items"][0]["linked_service_id"], svc.to_string());

    // Metrics: disk (MB→GB, peak) and bandwidth (sum); invalid source rejected.
    let bad = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "x", "name": "X", "unit": "u", "unit_price": "1", "source_key": "a b"})), &admin.h()).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let m1 = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "disk", "name": "Disk", "unit": "GB", "unit_price": "0.50", "included_quantity": "1", "source_key": "disk_mb", "source_divisor": "1024", "aggregate": "max"})), &admin.h()).await;
    assert_eq!(m1.status, StatusCode::OK, "{:?}", m1.body);
    assert_eq!(m1.body["aggregate"], "max");
    let m2 = call(&app, "POST", "/api/v1/admin/usage-metrics", Some(json!({"product_id": pid, "key": "bw", "name": "BW", "unit": "GB", "unit_price": "0.10", "source_key": "bw.gb"})), &admin.h()).await;
    assert_eq!(m2.status, StatusCode::OK, "{:?}", m2.body);

    let s1 = call(&app, "POST", "/api/v1/admin/usage/sample", None, &admin.h()).await;
    assert_eq!(s1.status, StatusCode::OK, "{:?}", s1.body);
    assert!(s1.body["recorded"].as_u64().unwrap() >= 2, "{:?}", s1.body);
    // Same hour → idempotent.
    let s2 = call(&app, "POST", "/api/v1/admin/usage/sample", None, &admin.h()).await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM usage_records WHERE service_id = $1").bind(svc).fetch_one(&st.pool).await.unwrap();
    assert_eq!(before, 2, "{:?}", s2.body);
    // An earlier-hour sample: disk max stays 3, bandwidth sums.
    sqlx::query("UPDATE usage_records SET recorded_at = recorded_at - interval '1 hour', idempotency_key = idempotency_key || '-old' WHERE service_id = $1").bind(svc).execute(&st.pool).await.unwrap();
    call(&app, "POST", "/api/v1/admin/usage/sample", None, &admin.h()).await;
    let u = call(&app, "GET", &format!("/api/v1/admin/services/{svc}/usage"), None, &admin.h()).await;
    assert_eq!(u.status, StatusCode::OK, "{:?}", u.body);
    let txt = u.body.to_string();
    let lines = u.body["items"][0]["metrics"].as_array().cloned().unwrap_or_default();
    let q = |k: &str| lines.iter().find(|l| l["key"] == k || l["metric_key"] == k).map(|l| l["quantity"].as_str().map(str::to_string).unwrap_or(l["quantity"].to_string()));
    assert_eq!(q("disk").as_deref().map(|s| s.trim_end_matches('0').trim_end_matches('.').to_string()), Some("3".into()), "{txt}");
    assert_eq!(q("bw").as_deref().map(|s| s.trim_end_matches('0').trim_end_matches('.').to_string()), Some("25".into()), "{txt}");
    let disk = lines.iter().find(|l| l["key"] == "disk").unwrap();
    assert_eq!(disk["charge"].as_str().unwrap().parse::<rust_decimal::Decimal>().unwrap(), rust_decimal::Decimal::new(100, 2), "(3-1 GB) x 0.50");
    assert_eq!(call(&app, "POST", "/api/v1/admin/usage/sample", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
}
