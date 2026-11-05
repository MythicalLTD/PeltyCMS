//! Regression: unpaid invoices without a due date are not overdue and must not break the dashboard.
use crate::common::*;
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::BTreeMap;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

fn totals(body: &Value, section: &str, currency: &str) -> (Decimal, i64) {
    body[section]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["currency"] == currency)
        .map(|row| {
            (
                Decimal::from_str(row["amount"].as_str().unwrap()).unwrap(),
                row["count"].as_i64().unwrap(),
            )
        })
        .unwrap_or((Decimal::ZERO, 0))
}

#[tokio::test]
async fn admin_dashboard_handles_undated_unpaid_invoices() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let client = sess(&app, &state, "customer").await;
    let profile = call(&app, "GET", "/api/v1/client/profile", None, &client.h()).await;
    assert_eq!(profile.status, StatusCode::OK, "{}", profile.body);
    let customer: Uuid = profile.body["id"].as_str().unwrap().parse().unwrap();
    let path = "/api/v1/admin/dashboard";
    let baseline = call(&app, "GET", path, None, &admin.h()).await;
    assert_eq!(baseline.status, StatusCode::OK, "{}", baseline.body);
    let mut expected = BTreeMap::new();
    for currency in ["EUR", "USD"] {
        expected.insert(
            currency,
            (
                totals(&baseline.body, "unpaid", currency),
                totals(&baseline.body, "overdue", currency),
            ),
        );
    }
    let now = OffsetDateTime::now_utc();
    for (currency, total, paid, due_at, overdue) in [
        ("EUR", 10, 2, None, false),
        ("EUR", 30, 4, Some(now + time::Duration::days(7)), false),
        ("EUR", 20, 3, Some(now - time::Duration::days(7)), true),
        ("USD", 7, 1, None, false),
    ] {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO invoices(id,customer_id,number,currency,status,total,amount_paid,due_at,finalized_at) VALUES($1,$2,$3,$4,'unpaid',$5,$6,$7,now())")
            .bind(id).bind(customer).bind(format!("dashboard-{id}")).bind(currency)
            .bind(Decimal::from(total)).bind(Decimal::from(paid)).bind(due_at)
            .execute(&state.pool).await.unwrap();
        let (unpaid, past_due) = expected.get_mut(currency).unwrap();
        unpaid.0 += Decimal::from(total - paid);
        unpaid.1 += 1;
        if overdue {
            past_due.0 += Decimal::from(total - paid);
            past_due.1 += 1;
        }
        let response = call(&app, "GET", path, None, &admin.h()).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "currency={currency}, due_at={due_at:?}: {}",
            response.body
        );
        for (currency, (unpaid, past_due)) in &expected {
            assert_eq!(
                totals(&response.body, "unpaid", currency),
                *unpaid,
                "unpaid {currency}"
            );
            assert_eq!(
                totals(&response.body, "overdue", currency),
                *past_due,
                "overdue {currency}"
            );
            assert!(
                response.body["overdue"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["currency"] == *currency)
                    .count()
                    <= 1
            );
        }
    }
}
