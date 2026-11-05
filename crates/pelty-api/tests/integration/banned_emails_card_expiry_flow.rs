use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn banned_domain_blocks_registration() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let dom = format!("spam{}.example", uuid::Uuid::new_v4().simple());
    assert_eq!(call(&app, "POST", "/api/v1/admin/banned-emails", Some(json!({"domain": "bad domain"})), &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "POST", "/api/v1/admin/banned-emails", Some(json!({"domain": format!("@{}", dom.to_uppercase()), "reason": "x"})), &admin.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/admin/banned-emails", None, &cust.h()).await.status, StatusCode::FORBIDDEN);
    let reg = json!({"email": format!("a@mx.{dom}"), "password": "Correct-Horse-9-battery", "display_name": "A"});
    let r = call(&app, "POST", "/api/v1/auth/register", Some(reg.clone()), &[]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert!(r.body.to_string().contains("error.email_banned"), "{:?}", r.body);
    let l = call(&app, "GET", "/api/v1/admin/banned-emails", None, &admin.h()).await;
    let row = l.body["items"].as_array().unwrap().iter().find(|i| i["domain"] == dom.as_str()).cloned().unwrap();
    assert_eq!(row["hits"], 1);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/banned-emails/{dom}"), None, &admin.h()).await.status, StatusCode::OK);
    let r = call(&app, "POST", "/api/v1/auth/register", Some(reg), &[]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
}

#[tokio::test]
async fn card_expiry_notice_sent_once() {
    let (app, st) = app().await;
    let cust = sess(&app, &st, "customer").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&cust.email).fetch_one(&st.pool).await.unwrap();
    let cid = pelty_database::customers::id_for_user(&st.pool, uid).await.unwrap();
    let today = time::macros::date!(2031 - 03 - 15);
    for (m, y, last4) in [(4i16, 2031i16, "4242"), (9, 2031, "1111"), (2, 2031, "2222")] {
        sqlx::query("INSERT INTO payment_methods (id, customer_id, gateway, token, brand, last4, exp_month, exp_year) VALUES ($1,$2,'stripe','tok','visa',$3,$4,$5)")
            .bind(uuid::Uuid::new_v4()).bind(cid).bind(last4).bind(m).bind(y).execute(&st.pool).await.unwrap();
    }
    let n = pelty_api::autopay::card_expiry_notices(&st, today).await.unwrap();
    assert!(n >= 1);
    let mails = mails_to(&st, &cust.email).await;
    let hits: Vec<_> = mails.iter().filter(|m| m.text.contains("4242")).collect();
    assert_eq!(hits.len(), 1);
    assert!(!mails.iter().any(|m| m.text.contains("1111") || m.text.contains("2222")));
    pelty_api::autopay::card_expiry_notices(&st, today).await.unwrap();
    assert_eq!(mails_to(&st, &cust.email).await.iter().filter(|m| m.text.contains("4242")).count(), 1);
}
