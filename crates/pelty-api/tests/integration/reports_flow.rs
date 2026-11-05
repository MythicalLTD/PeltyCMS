use crate::common;
use axum::http::StatusCode;
use common::*;

async fn customer(st: &pelty_api::AppState, created: &str) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO customers (id, email, currency, created_at) VALUES ($1, $2, 'EUR', $3::timestamptz)")
        .bind(id).bind(email()).bind(created).execute(&st.pool).await.unwrap();
    id
}

async fn service(
    st: &pelty_api::AppState,
    cid: uuid::Uuid,
    pid: &str,
    cycle: &str,
    amount: &str,
    from: &str,
    until: Option<&str>,
) {
    sqlx::query("INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, activated_at, terminated_at)
                 VALUES ($1, $2, $3::uuid, CASE WHEN $7::timestamptz IS NULL THEN 'active' ELSE 'terminated' END, $4, 'EUR', $5::numeric, $6::timestamptz, $7::timestamptz)")
        .bind(uuid::Uuid::new_v4()).bind(cid).bind(pid).bind(cycle).bind(amount).bind(from).bind(until)
        .execute(&st.pool).await.unwrap();
}

/// All fixtures live in 1990, a window no other test writes to.
#[tokio::test]
async fn revenue_mrr_and_churn_by_month() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let cust = sess(&app, &st, "customer").await;
    let (pid, _, _) = seed_product(&app, &admin).await;
    for q in [
        "DELETE FROM payments WHERE applied_at < '1991-01-01'",
        "DELETE FROM services WHERE activated_at < '1991-01-01'",
        "DELETE FROM customers WHERE created_at < '1991-01-01'",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(q))
            .execute(&st.pool)
            .await
            .unwrap();
    }

    let a = customer(&st, "1990-01-05T00:00:00Z").await;
    let b = customer(&st, "1990-01-06T00:00:00Z").await;
    // a: 10/month + 120/year (=10/month) from Jan, both live.
    service(
        &st,
        a,
        &pid,
        "monthly",
        "10.00",
        "1990-01-10T00:00:00Z",
        None,
    )
    .await;
    service(
        &st,
        a,
        &pid,
        "annual",
        "120.00",
        "1990-01-20T00:00:00Z",
        None,
    )
    .await;
    // b: 30/quarter (=10/month) from Jan, terminated mid-March → churns in March.
    service(
        &st,
        b,
        &pid,
        "quarterly",
        "30.00",
        "1990-01-15T00:00:00Z",
        Some("1990-03-15T00:00:00Z"),
    )
    .await;
    for (amt, at, status) in [
        ("10.00", "1990-02-01T10:00:00Z", "succeeded"),
        ("0.10", "1990-02-28T23:59:59Z", "succeeded"),
        ("99", "1990-02-10T00:00:00Z", "failed"),
    ] {
        sqlx::query("INSERT INTO payments (id, gateway, amount, currency, status, idempotency_key, customer_id, applied_at) VALUES ($1, 'manual', $2::numeric, 'EUR', $3, $4, $5, $6::timestamptz)")
            .bind(uuid::Uuid::new_v4()).bind(amt).bind(status).bind(uuid::Uuid::new_v4().to_string()).bind(a).bind(at)
            .execute(&st.pool).await.unwrap();
    }

    let url = "/api/v1/admin/reports?from=1990-01&to=1990-04";
    assert_eq!(
        call(&app, "GET", url, None, &cust.h()).await.status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", url, None, &staff.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let b_ = &r.body;
    assert_eq!(b_["revenue"].as_array().unwrap().len(), 1, "{b_}");
    assert_eq!(b_["revenue"][0]["month"], "1990-02");
    assert_eq!(b_["revenue"][0]["amount"], "10.1000");
    assert_eq!(b_["revenue"][0]["count"], 2);

    let mrr: Vec<(String, String)> = b_["mrr"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["month"].as_str().unwrap().to_string(),
                p["amount"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        mrr,
        vec![
            ("1990-01".into(), "30.0000".into()),
            ("1990-02".into(), "30.0000".into()),
            ("1990-03".into(), "20.0000".into()),
            ("1990-04".into(), "20.0000".into()),
        ]
    );

    let churn = b_["churn"].as_array().unwrap();
    assert_eq!(churn.len(), 4);
    assert_eq!(
        (
            churn[0]["customers_start"].as_i64(),
            churn[0]["gained"].as_i64(),
            churn[0]["new_signups"].as_i64()
        ),
        (Some(0), Some(2), Some(2))
    );
    assert_eq!(churn[1]["churned"], 0);
    assert_eq!(
        (
            churn[2]["customers_start"].as_i64(),
            churn[2]["churned"].as_i64()
        ),
        (Some(2), Some(1))
    );
    assert_eq!(churn[2]["churn_rate"], "0.5000");
    assert_eq!(churn[3]["customers_start"], 1);

    for bad in [
        "from=1990-05&to=1990-01",
        "from=1980-01&to=1990-01",
        "from=1990-1",
    ] {
        let r = call(
            &app,
            "GET",
            &format!("/api/v1/admin/reports?{bad}"),
            None,
            &admin.h(),
        )
        .await;
        assert_eq!(r.body["error"]["code"], "error.invalid_range", "{bad}");
    }
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/reports", None, &admin.h())
            .await
            .status,
        StatusCode::OK
    );
}
