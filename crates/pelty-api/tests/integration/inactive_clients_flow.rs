use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn inactive_clients_dry_run_then_erase_with_safeguards() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let ah = admin.h();
    let off = json!({"enabled": false, "inactive_days": 0, "max_per_run": 0});
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/inactive-clients", Some(off.clone()), &ah).await.status, StatusCode::OK);
    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/inactive-clients", Some(json!({"enabled": true, "inactive_days": 30, "max_per_run": 5})), &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    let old = sess(&app, &st, "customer").await;
    let busy = sess(&app, &st, "customer").await;
    let old_c = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &old.h()).await.body["id"].as_str().unwrap().to_string();
    let busy_c = call(&app, "PUT", "/api/v1/client/profile", Some(json!({"country": "DE"})), &busy.h()).await.body["id"].as_str().unwrap().to_string();
    // Age both accounts by three years.
    let uid = |e: String| { let p = st.pool.clone(); async move { sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE email = $1").bind(e).fetch_one(&p).await.unwrap() } };
    let (old_u, busy_u) = (uid(old.email.clone()).await, uid(busy.email.clone()).await);
    for (c, u) in [(&old_c, old_u), (&busy_c, busy_u)] {
        sqlx::query("UPDATE customers SET created_at = now() - interval '3 years', updated_at = now() - interval '3 years' WHERE id = $1::uuid").bind(c).execute(&st.pool).await.unwrap();
        sqlx::query("UPDATE sessions SET created_at = now() - interval '3 years' WHERE user_id = $1").bind(u).execute(&st.pool).await.unwrap();
        sqlx::query("UPDATE users SET last_login_at = now() - interval '3 years' WHERE id = $1").bind(u).execute(&st.pool).await.unwrap();
    }
    // Safeguard: an open ticket keeps "busy" out.
    let dep = Uuid::new_v4();
    sqlx::query("INSERT INTO ticket_departments (id, name) VALUES ($1, $2)").bind(dep).bind(format!("IC {}", &dep.simple().to_string()[..8])).execute(&st.pool).await.unwrap();
    sqlx::query("INSERT INTO tickets (id, customer_id, department_id, subject, last_reply_at) VALUES ($1, $2::uuid, $3, 'Help', now() - interval '3 years')")
        .bind(Uuid::new_v4()).bind(&busy_c).bind(dep).execute(&st.pool).await.unwrap();
    

    let r = call(&app, "GET", "/api/v1/admin/inactive-clients/preview?days=730", None, &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let ids: Vec<&str> = r.body["items"].as_array().unwrap().iter().map(|i| i["customer_id"].as_str().unwrap()).collect();
    assert!(ids.contains(&old_c.as_str()));
    assert!(!ids.contains(&busy_c.as_str()));
    assert_eq!(call(&app, "GET", "/api/v1/admin/inactive-clients/preview?days=10", None, &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(call(&app, "GET", "/api/v1/admin/inactive-clients/preview?days=730", None, &old.h()).await.status, StatusCode::FORBIDDEN);

    // Dry run changed nothing; a disabled run is a no-op.
    assert_eq!(call(&app, "POST", "/api/v1/admin/inactive-clients/run", None, &ah).await.body["erased"], 0);
    let status: String = sqlx::query_scalar("SELECT status FROM customers WHERE id = $1::uuid").bind(&old_c).fetch_one(&st.pool).await.unwrap();
    assert_ne!(status, "closed");

    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/inactive-clients", Some(json!({"enabled": true, "inactive_days": 730, "max_per_run": 500})), &ah).await.status, StatusCode::OK);
    let r = call(&app, "POST", "/api/v1/admin/inactive-clients/run", None, &ah).await;
    assert!(r.body["erased"].as_u64().unwrap() >= 1, "{:?}", r.body);
    let (status, email): (String, String) = sqlx::query_as("SELECT status, email::text FROM customers WHERE id = $1::uuid").bind(&old_c).fetch_one(&st.pool).await.unwrap();
    assert_eq!(status, "closed");
    assert!(email.ends_with("@erased.invalid"));
    let gdpr: i64 = sqlx::query_scalar("SELECT count(*) FROM gdpr_requests WHERE user_id = $1 AND status = 'completed' AND decided_by IS NULL").bind(old_u).fetch_one(&st.pool).await.unwrap();
    assert_eq!(gdpr, 1);
    let busy_status: String = sqlx::query_scalar("SELECT status FROM customers WHERE id = $1::uuid").bind(&busy_c).fetch_one(&st.pool).await.unwrap();
    assert_ne!(busy_status, "closed");

    assert_eq!(call(&app, "PUT", "/api/v1/admin/settings/inactive-clients", Some(off), &ah).await.status, StatusCode::OK);
}
