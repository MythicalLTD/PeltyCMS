use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const SUPPORT: &str = "00000000-0000-0000-0002-000000000001";

#[tokio::test]
async fn client_search_is_scoped_to_own_records() {
    let (app, st) = app().await;
    let a = sess(&app, &st, "customer").await;
    let b = sess(&app, &st, "customer").await;
    let tag = format!("zq{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(json!({"department_id": SUPPORT, "subject": format!("Help {tag}"), "body": "x"})),
        &a.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    assert_eq!(
        call(&app, "GET", "/api/v1/client/search?q=ab", None, &[])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/client/search?q=a", None, &a.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let ra = call(
        &app,
        "GET",
        &format!("/api/v1/client/search?q={tag}"),
        None,
        &a.h(),
    )
    .await;
    assert_eq!(ra.status, StatusCode::OK, "{:?}", ra.body);
    assert_eq!(ra.body["tickets"].as_array().unwrap().len(), 1);
    for k in ["invoices", "services", "tickets", "domains", "kb"] {
        assert!(ra.body[k].is_array(), "{k}");
    }

    let rb = call(
        &app,
        "GET",
        &format!("/api/v1/client/search?q={tag}"),
        None,
        &b.h(),
    )
    .await;
    assert_eq!(rb.status, StatusCode::OK);
    assert!(
        rb.body["tickets"].as_array().unwrap().is_empty(),
        "leak: {:?}",
        rb.body
    );
    // Wildcards are escaped: a bare "%%" must not dump other customers' data.
    let w = call(&app, "GET", "/api/v1/client/search?q=%25%25", None, &b.h()).await;
    assert!(w.body["tickets"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn client_search_by_number_partial_and_type_word() {
    let (app, st) = app().await;
    let a = sess(&app, &st, "customer").await;
    let b = sess(&app, &st, "customer").await;
    let tag = format!("zr{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let r = call(
        &app,
        "POST",
        "/api/v1/client/tickets",
        Some(
            json!({"department_id": SUPPORT, "subject": format!("Server down {tag}"), "body": "x"}),
        ),
        &a.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let tid = r.body["id"]
        .as_str()
        .or_else(|| r.body["ticket"]["id"].as_str())
        .expect("ticket id")
        .to_string();
    let ids = |v: &serde_json::Value| {
        v["tickets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };

    // Partial subject match (middle of the tag).
    let p = call(
        &app,
        "GET",
        &format!("/api/v1/client/search?q={}", &tag[2..8]),
        None,
        &a.h(),
    )
    .await;
    assert!(ids(&p.body).contains(&tid), "{:?}", p.body);
    // "#<number>" finds it by ticket number.
    let num = ids(&p.body)
        .iter()
        .position(|x| *x == tid)
        .map(|i| p.body["tickets"][i]["number"].to_string())
        .unwrap();
    let n = call(
        &app,
        "GET",
        &format!("/api/v1/client/search?q=%23{}", num.trim_matches('"')),
        None,
        &a.h(),
    )
    .await;
    assert_eq!(n.status, StatusCode::OK, "{:?}", n.body);
    assert!(ids(&n.body).contains(&tid), "{:?}", n.body);
    // A bare type word lists own recent tickets, in en and ro.
    for w in ["ticket", "tichete"] {
        let k = call(
            &app,
            "GET",
            &format!("/api/v1/client/search?q={w}"),
            None,
            &a.h(),
        )
        .await;
        assert!(ids(&k.body).contains(&tid), "{w}: {:?}", k.body);
        // Isolation: the other customer never sees it, by word, number or subject.
        let o = call(
            &app,
            "GET",
            &format!("/api/v1/client/search?q={w}"),
            None,
            &b.h(),
        )
        .await;
        assert!(!ids(&o.body).contains(&tid), "leak {w}: {:?}", o.body);
    }
    let o = call(
        &app,
        "GET",
        &format!("/api/v1/client/search?q=%23{}", num.trim_matches('"')),
        None,
        &b.h(),
    )
    .await;
    assert!(!ids(&o.body).contains(&tid), "leak #: {:?}", o.body);
    assert_eq!(
        call(&app, "GET", "/api/v1/client/search?q=%23%23", None, &a.h())
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
