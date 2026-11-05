//! WASM host services: DB-backed kv, event delivery by subscription,
//! `secret` settings sealed at rest and masked in the admin API.
use crate::common;
use axum::http::StatusCode;
use common::*;
use pelty_extensions::events::DomainEvent;
use serde_json::{json, Value};

const ID: &str = "integration-eventlog";

async fn log(st: &pelty_api::AppState) -> Vec<String> {
    let a = pelty_api::wasm_addons::loaded(st, "integration", "eventlog").expect("loaded");
    let v: Value = a.call_async(Default::default(), "log".into(), json!({})).await.unwrap();
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn kv_events_and_sealed_secrets() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(ID).execute(&st.pool).await.unwrap();
    sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1").bind(ID).execute(&st.pool).await.unwrap();
    pelty_api::addons::accept_package(&st, &fixture_zip("fixture-events", "pelty-addon-fixture-events"), true).await.expect("fixture package");
    let base = format!("/api/v1/admin/addons/{ID}");
    let r = call(&app, "POST", &format!("{base}/install"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", &format!("{base}/enable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    assert_eq!(log(&st).await, Vec::<String>::new());

    // Subscribed events reach on-event; others don't. State lives in addon_kv.
    st.emit(DomainEvent::new("fixture.ping", json!({})));
    st.emit(DomainEvent::new("service.created", json!({})));
    st.emit(DomainEvent::new("invoice.paid", json!({"invoice_id": "x"})));
    let mut got = vec![];
    for _ in 0..100 {
        got = log(&st).await;
        if got.len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    got.sort();
    assert_eq!(got, vec!["fixture.ping".to_string(), "invoice.paid".to_string()]);
    let (raw, sealed): (Vec<u8>, bool) = sqlx::query_as("SELECT value, sealed FROM addon_kv WHERE addon_id = $1 AND key = 'log'").bind(ID).fetch_one(&st.pool).await.unwrap();
    if st.secret_box.is_some() {
        assert!(sealed && !String::from_utf8_lossy(&raw).contains("fixture.ping"), "kv sealed at rest");
    }

    // Secret settings: masked on read, sealed in the DB, MASK keeps the value.
    let sp = format!("{base}/settings");
    assert_eq!(call(&app, "PUT", &sp, Some(json!({"token": "tok-123"})), &admin.h()).await.status, StatusCode::OK);
    let g = call(&app, "GET", &sp, None, &admin.h()).await;
    assert_eq!(g.body["values"]["token"], pelty_api::addon_secrets::MASK);
    assert!(!g.body.to_string().contains("tok-123"));
    let stored: Value = sqlx::query_scalar("SELECT settings FROM installed_addons WHERE id = $1").bind(ID).fetch_one(&st.pool).await.unwrap();
    if st.secret_box.is_some() {
        assert!(!stored.to_string().contains("tok-123"), "sealed at rest: {stored}");
    }
    assert_eq!(call(&app, "PUT", &sp, Some(json!({"token": pelty_api::addon_secrets::MASK})), &admin.h()).await.status, StatusCode::OK);
    let stored: Value = sqlx::query_scalar("SELECT settings FROM installed_addons WHERE id = $1").bind(ID).fetch_one(&st.pool).await.unwrap();
    assert_eq!(pelty_api::addon_secrets::open(&st, &stored)["token"], "tok-123");

    // Disable unregisters; uninstall drops the kv rows.
    assert_eq!(call(&app, "POST", &format!("{base}/disable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    assert!(pelty_api::wasm_addons::loaded(&st, "integration", "eventlog").is_none());
    assert_eq!(call(&app, "DELETE", &base, None, &admin.h()).await.status, StatusCode::NO_CONTENT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM addon_kv WHERE addon_id = $1").bind(ID).fetch_one(&st.pool).await.unwrap();
    assert_eq!(n, 0);
}
