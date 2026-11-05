use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use std::sync::Arc;

struct Flagger;
impl pelty_api::fraud::FraudHook for Flagger {
    fn name(&self) -> &'static str {
        "test"
    }
    fn check<'a>(
        &'a self,
        ctx: &'a pelty_api::fraud::OrderContext,
    ) -> pelty_api::fraud::BoxFuture<'a, Result<Vec<pelty_api::fraud::Hit>, String>> {
        Box::pin(async move {
            if ctx.country == "FR" {
                Ok(vec![pelty_api::fraud::Hit {
                    rule: "fr".into(),
                    score: 55,
                }])
            } else if ctx.country == "IT" {
                Err("provider down".into())
            } else {
                Ok(vec![])
            }
        })
    }
}

// One test: fraud settings are global.
#[tokio::test]
async fn fraud_screening_accept_review_reject() {
    let (app, st) = app().await;
    st.fraud_hooks.write().unwrap().push(Arc::new(Flagger));
    let admin = sess(&app, &st, "admin").await;
    let (pid, oid, ch) = seed_product(&app, &admin).await;
    let body = json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]});
    let customer = |country: &'static str| {
        let app = app.clone();
        let st = st.clone();
        async move {
            let c = sess(&app, &st, "customer").await;
            let r = call(
                &app,
                "PUT",
                "/api/v1/client/profile",
                Some(json!({"country": country, "first_name": "F"})),
                &c.h(),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            c
        }
    };

    // Settings: staff-only, validated.
    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/fraud",
        Some(json!({"enabled": true, "review_score": 90, "reject_score": 10})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.fraud_thresholds");
    let r = call(&app, "PUT", "/api/v1/admin/settings/fraud", Some(json!({
        "enabled": true, "review_score": 50, "reject_score": 90, "max_orders_per_ip_day": 0, "max_orders_per_customer_day": 2,
        "max_first_order_total": "0", "blocked_countries": ["kp"]
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["blocked_countries"], json!(["KP"]));

    // Accept; hook error is ignored (fail-open).
    let de = customer("IT").await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/orders",
            Some(body.clone()),
            &de.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    // Review via plugin hook: order goes through but is flagged.
    let fr = customer("FR").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(body.clone()),
        &fr.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let fr_order = r.body["order"]["id"].as_str().unwrap().to_string();
    // Reject: blocked country → order cancelled as fraud, 422.
    let kp = customer("KP").await;
    let r = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(body.clone()),
        &kp.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["error"]["code"], "error.order_rejected");
    let st_kp: String = sqlx::query_scalar("SELECT o.status FROM orders o JOIN customers c ON c.id = o.customer_id JOIN users u ON u.id = c.owner_user_id WHERE u.email = $1")
        .bind(&kp.email).fetch_one(&st.pool).await.unwrap();
    assert_eq!(st_kp, "fraud");

    // Customer velocity: third order in a day → 50 → review.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/orders",
            Some(body.clone()),
            &de.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/client/orders",
            Some(body.clone()),
            &de.h()
        )
        .await
        .status,
        StatusCode::OK
    );

    let rv = call(
        &app,
        "GET",
        "/api/v1/admin/fraud/checks?verdict=review&per_page=100",
        None,
        &admin.h(),
    )
    .await;
    let items = rv.body["items"].as_array().unwrap();
    let f = items
        .iter()
        .find(|x| x["order_id"] == fr_order.as_str())
        .expect("review row");
    assert_eq!(
        (f["score"].clone(), f["reasons"][0]["rule"].clone()),
        (json!(55), json!("test:fr"))
    );
    assert!(items
        .iter()
        .any(|x| x["customer_email"] == de.email.as_str()
            && x["reasons"][0]["rule"] == "customer_velocity"));
    let rj = call(
        &app,
        "GET",
        "/api/v1/admin/fraud/checks?verdict=reject",
        None,
        &admin.h(),
    )
    .await;
    assert!(rj.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["customer_email"] == kp.email.as_str() && x["score"] == 100));
    let cust = sess(&app, &st, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/fraud/checks", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // ---- Review queue ----
    let hold = |order: String| {
        let pool = st.pool.clone();
        async move {
            let id: uuid::Uuid = order.parse().unwrap();
            let rules = pelty_database::order_approval::ApprovalRules::default();
            pelty_database::order_approval::facts(&pool, id, &rules)
                .await
                .unwrap()
                .unwrap()
                .fraud_review
        }
    };
    let status = |order: String| {
        let pool = st.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT status FROM orders WHERE id = $1::uuid")
                .bind(order)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let open = call(
        &app,
        "GET",
        "/api/v1/admin/fraud/checks?open=true&per_page=100",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(open.status, StatusCode::OK, "{:?}", open.body);
    let open_items = open.body["items"].as_array().unwrap();
    assert!(open_items.iter().all(|x| x["verdict"] == "review"
        && x["decision"].is_null()
        && x["order_status"] == "pending"));
    let fr_check = open_items
        .iter()
        .find(|x| x["order_id"] == fr_order.as_str())
        .expect("fr in queue")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let de_row = open_items
        .iter()
        .find(|x| x["customer_email"] == de.email.as_str())
        .expect("de in queue");
    let (de_check, de_order) = (
        de_row["id"].as_str().unwrap().to_string(),
        de_row["order_id"].as_str().unwrap().to_string(),
    );
    assert!(hold(fr_order.clone()).await, "review holds the order");
    let sum = call(&app, "GET", "/api/v1/admin/fraud/summary", None, &admin.h()).await;
    assert!(
        sum.body["open_reviews"].as_i64().unwrap() >= 2 && sum.body["enabled"] == true,
        "{:?}",
        sum.body
    );
    let one = call(
        &app,
        "GET",
        &format!("/api/v1/admin/fraud/checks/{fr_check}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (
            one.body["signals"]["billing_country"].clone(),
            one.body["signals"]["email_domain"].clone()
        ),
        (json!("FR"), json!("pelty.test"))
    );
    // Customers can't decide; notes are bounded.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/fraud/checks/{fr_check}/approve"),
            Some(json!({})),
            &cust.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let long = "x".repeat(2001);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/admin/fraud/checks/{fr_check}/approve"),
            Some(json!({"note": long})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Reject: the order is cancelled as fraud; deciding twice is a conflict.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/fraud/checks/{de_check}/reject"),
        Some(json!({"note": "stolen card"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["check"]["decision"].clone(),
            r.body["check"]["review_note"].clone()
        ),
        (json!("rejected"), json!("stolen card"))
    );
    assert_eq!(r.body["check"]["reviewer_email"], admin.email.as_str());
    assert_eq!(status(de_order.clone()).await, "fraud");
    let again = call(
        &app,
        "POST",
        &format!("/api/v1/admin/fraud/checks/{de_check}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (again.status, again.body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("error.fraud_already_reviewed"))
    );

    // Approve and activate: services are provisioned, the hold is gone.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/fraud/checks/{fr_check}/approve"),
        Some(json!({"note": "called the customer", "approve_order": true})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["order_approved"].clone(),
            r.body["check"]["decision"].clone()
        ),
        (json!(true), json!("approved"))
    );
    assert_eq!(r.body["services"].as_array().unwrap().len(), 1);
    assert_eq!(status(fr_order.clone()).await, "active");
    assert!(!hold(fr_order.clone()).await);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action IN ('fraud.review_approved', 'fraud.review_rejected') AND target IN ($1, $2)")
        .bind(&fr_order).bind(&de_order).fetch_one(&st.pool).await.unwrap();
    assert_eq!(audited, 2);

    // ---- Heuristics: disposable e-mail + client-side proxy headers ----
    let r = call(&app, "PUT", "/api/v1/admin/settings/fraud", Some(json!({
        "enabled": true, "review_score": 50, "reject_score": 90, "max_orders_per_ip_day": 0, "max_orders_per_customer_day": 0,
        "max_customers_per_ip": 0, "disposable_domains": ["Burner.Pelty.test"], "vpn_ranges": ["198.51.100.7", "198.51.100.0/24"]
    })), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["disposable_domains"].clone(),
            r.body["vpn_ranges"].clone()
        ),
        (
            json!(["burner.pelty.test"]),
            json!(["198.51.100.7/32", "198.51.100.0/24"])
        )
    );
    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/fraud",
        Some(json!({"enabled": true, "vpn_ranges": ["nope"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.invalid_cidr");
    let bad = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/fraud",
        Some(json!({"enabled": true, "disposable_domains": ["no spaces.com"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.body["error"]["code"], "error.invalid_domain");

    let burner = customer("DE").await;
    sqlx::query("UPDATE customers SET email = $2 FROM users u WHERE u.id = customers.owner_user_id AND u.email = $1")
        .bind(&burner.email).bind(format!("{}@mailinator.com", uuid::Uuid::new_v4().simple())).execute(&st.pool).await.unwrap();
    let bh = burner.h();
    let h = [bh[0], bh[1], ("via", "1.1 squid")];
    let r = call_from(
        &app,
        "203.0.113.77",
        "POST",
        "/api/v1/client/orders",
        Some(body.clone()),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let b_order = r.body["order"]["id"].as_str().unwrap().to_string();
    let q = call(
        &app,
        "GET",
        &format!("/api/v1/admin/fraud/checks?order_id={b_order}"),
        None,
        &admin.h(),
    )
    .await;
    let row = &q.body["items"][0];
    assert_eq!(
        (row["verdict"].clone(), row["score"].clone()),
        (json!("review"), json!(65)),
        "{row:?}"
    );
    let rules: Vec<&str> = row["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, vec!["disposable_email", "proxy_headers"]);
    assert_eq!(
        (
            row["signals"]["proxy_headers"].clone(),
            row["signals"]["email_domain"].clone()
        ),
        (json!(["via"]), json!("mailinator.com"))
    );
    // Staff approving the order elsewhere also closes its review.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{b_order}/approve"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = call(
        &app,
        "GET",
        &format!("/api/v1/admin/fraud/checks/{}", row["id"].as_str().unwrap()),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        (c.body["decision"].clone(), c.body["review_note"].clone()),
        (json!("approved"), json!("order approved"))
    );

    // VPN range from the same untrusted public peer.
    let vpn = customer("DE").await;
    let r = call_from(
        &app,
        "198.51.100.7",
        "POST",
        "/api/v1/client/orders",
        Some(body.clone()),
        &vpn.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let q = call(
        &app,
        "GET",
        &format!(
            "/api/v1/admin/fraud/checks?customer_id={}",
            r.body["order"]["customer_id"].as_str().unwrap()
        ),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(
        q.body["items"][0]["reasons"],
        json!([{"rule": "vpn_ip", "score": 40}])
    );

    let off = call(
        &app,
        "PUT",
        "/api/v1/admin/settings/fraud",
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(off.status, StatusCode::OK);
}
