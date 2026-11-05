use crate::common;
use axum::http::StatusCode;
use common::*;
use uuid::Uuid;

#[tokio::test]
async fn ssl_checks_admin_and_client() {
    let (app, st) = app().await;
    let pool = &st.pool;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&alice.email)
        .fetch_one(pool)
        .await
        .unwrap();
    let cid = pelty_database::customers::id_for_user(pool, uid)
        .await
        .unwrap();
    // Re-runnable: retire a live "localhost" row from an earlier run.
    sqlx::query("UPDATE domains SET status = 'cancelled' WHERE lower(name) = 'localhost' AND status NOT IN ('cancelled', 'failed')").execute(pool).await.unwrap();
    // "localhost" resolves to loopback only, which the probe refuses.
    let did: Uuid = sqlx::query_scalar("INSERT INTO domains (customer_id, name, tld, kind, status, registrar, years) VALUES ($1, 'localhost', 'localhost', 'register', 'active', 'mock', 1) RETURNING id")
        .bind(cid).fetch_one(pool).await.unwrap();

    assert_eq!(
        call(&app, "GET", "/api/v1/admin/ssl", None, &alice.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/domains/{did}/ssl-check"),
            None,
            &alice.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/domains/{}/ssl-check", Uuid::new_v4()),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // Large limit: earlier runs on the same database leave unchecked active domains.
    assert_eq!(
        pelty_database::ssl_checks::due(pool, 24, 100_000)
            .await
            .unwrap()
            .iter()
            .filter(|d| d.0 == did)
            .count(),
        1
    );

    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/domains/{did}/ssl-check"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "unreachable");
    assert_eq!(r.body["domain"], "localhost");
    assert!(
        r.body["error"]
            .as_str()
            .unwrap()
            .contains("no public address"),
        "{:?}",
        r.body
    );
    // Checked: no longer due.
    assert!(pelty_database::ssl_checks::due(pool, 24, 100_000)
        .await
        .unwrap()
        .iter()
        .all(|d| d.0 != did));

    let l = call(
        &app,
        "GET",
        "/api/v1/admin/ssl?status=unreachable",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(l.status, StatusCode::OK);
    assert!(l.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["domain_id"] == did.to_string()));
    let l = call(
        &app,
        "GET",
        "/api/v1/admin/ssl?status=valid",
        None,
        &admin.h(),
    )
    .await;
    assert!(l.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|x| x["domain_id"] != did.to_string()));

    let c = call(&app, "GET", "/api/v1/client/ssl", None, &alice.h()).await;
    assert_eq!(c.status, StatusCode::OK);
    assert_eq!(c.body["items"].as_array().unwrap().len(), 1);
    let other = sess(&app, &st, "customer").await;
    let c = call(&app, "GET", "/api/v1/client/ssl", None, &other.h()).await;
    assert_eq!(c.body["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn probe_handles_closed_ports_and_bad_names() {
    let p = pelty_api::ssl_probe::probe("bad host!", 443, true).await;
    assert_eq!(p.status, "invalid");
    // Nothing listens on this port.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let p = pelty_api::ssl_probe::probe("localhost", port, true).await;
    assert_eq!(p.status, "unreachable", "{}", p.error);
    // A listener that is not TLS.
    let srv = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = srv.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = srv.accept().await {
            use tokio::io::AsyncWriteExt;
            let _ = s.write_all(b"HTTP/1.0 400 nope\r\n\r\n").await;
        }
    });
    let p = pelty_api::ssl_probe::probe("localhost", port, true).await;
    assert_ne!(p.status, "valid");
}
