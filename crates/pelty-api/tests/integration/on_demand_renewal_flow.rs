use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn renew_now_invoices_next_period_once() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let bob = sess(&app, &st, "customer").await;
    let (ah, ch) = (admin.h(), alice.h());
    let off = json!({"enabled": false, "max_days_ahead": 0});
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/on-demand-renewals", Some(off.clone()), &ah).await.status, StatusCode::OK);
    let cid = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &ch).await.body["id"].as_str().unwrap().to_string();
    let _ = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &bob.h()).await;

    let tag = Uuid::new_v4().simple().to_string();
    let (gid, pid, sid) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    sqlx::query("INSERT INTO product_groups (id, name, slug) VALUES ($1,'G',$2)").bind(gid).bind(format!("odr-{}", &tag[..10])).execute(&st.pool).await.unwrap();
    sqlx::query("INSERT INTO products (id, group_id, name, slug) VALUES ($1,$2,'VPS',$3)").bind(pid).bind(gid).bind(format!("odrp-{}", &tag[..10])).execute(&st.pool).await.unwrap();
    sqlx::query("INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date) VALUES ($1,$2::uuid,$3,'active','monthly','EUR',12.50, CURRENT_DATE + 20)")
        .bind(sid).bind(&cid).bind(pid).execute(&st.pool).await.unwrap();
    let url = format!("/api/v1/client/services/{sid}/renew-now");

    // Disabled for clients by default.
    assert_eq!(call(&app, "POST", &url, None, &ch).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/on-demand-renewals", Some(json!({"enabled": true, "max_days_ahead": 400})), &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/on-demand-renewals", Some(json!({"enabled": true, "max_days_ahead": 10})), &ah).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/renewals/config", None, &ch).await.body["enabled"], true);
    // 20 days out, limit 10.
    let r = call(&app, "POST", &url, None, &ch).await;
    assert_eq!((r.status, r.body["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("error.renewal_too_early")), "{:?}", r.body);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/on-demand-renewals", Some(json!({"enabled": true, "max_days_ahead": 30})), &ah).await.status, StatusCode::OK);
    // Someone else's service looks absent.
    assert_eq!(call(&app, "POST", &url, None, &bob.h()).await.status, StatusCode::NOT_FOUND);

    let r = call(&app, "POST", &url, None, &ch).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let inv = r.body["invoice_id"].as_str().unwrap().to_string();
    let (next, total, status): (time::Date, rust_decimal::Decimal, String) = sqlx::query_as(
        "SELECT s.next_due_date, i.total, i.status FROM services s, invoices i WHERE s.id = $1 AND i.id = $2::uuid").bind(sid).bind(&inv).fetch_one(&st.pool).await.unwrap();
    assert_eq!(status, "unpaid");
    assert!(total >= "12.50".parse().unwrap());
    assert_eq!(r.body["period_end"], next.to_string());
    // Open invoice blocks another early renewal (client and admin).
    let r = call(&app, "POST", &url, None, &ch).await;
    assert_eq!((r.status, r.body["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("error.renewal_pending")));
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/services/{sid}/renew-now"), None, &ah).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/services/{sid}/renew-now"), None, &ch).await.status, StatusCode::FORBIDDEN);
    // Terminated services cannot be renewed.
    sqlx::query("UPDATE invoices SET status = 'void' WHERE id = $1::uuid").bind(&inv).execute(&st.pool).await.unwrap();
    sqlx::query("UPDATE services SET status = 'terminated' WHERE id = $1").bind(sid).execute(&st.pool).await.unwrap();
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/services/{sid}/renew-now"), None, &ah).await.status, StatusCode::CONFLICT);

    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/on-demand-renewals", Some(off), &ah).await.status, StatusCode::OK);
}
