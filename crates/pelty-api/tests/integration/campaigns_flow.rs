use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn cid(app: &axum::Router, s: &Sess) -> String {
    call(app, "GET", "/api/v1/client/profile", None, &s.h()).await.body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn campaign_segment_queue_send_and_unsubscribe() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let staff = sess(&app, &st, "staff").await;
    let ah = admin.h();
    let a = sess(&app, &st, "customer").await;
    let b = sess(&app, &st, "customer").await;
    let c = sess(&app, &st, "customer").await;
    let g = call(&app, "POST", "/api/v1/admin/client-groups", Some(json!({"name": format!("cg{}", &uuid::Uuid::new_v4().simple().to_string()[..8])})), &ah).await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let gid = g.body["group"]["id"].as_str().unwrap().to_string();
    for s in [&a, &b, &c] {
        let id = cid(&app, s).await;
        let r = call(&app, "POST", &format!("/api/v1/admin/customers/{id}/group"), Some(json!({"group_id": gid})), &ah).await;
        assert!(r.status.is_success(), "{:?}", r.body);
    }
    // c opts out from the client area.
    assert_eq!(call(&app, "PUT", "/api/v1/client/marketing", Some(json!({"opt_out": true})), &c.h()).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/marketing", None, &c.h()).await.body["opt_out"], true);

    let seg = json!({"client_group_id": gid});
    assert_eq!(call(&app, "POST", "/api/v1/admin/campaigns/preview", Some(seg.clone()), &staff.h()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", "/api/v1/admin/campaigns/preview", Some(seg.clone()), &ah).await.body["recipients"], 2);
    assert_eq!(call(&app, "POST", "/api/v1/admin/campaigns", Some(json!({"name": "N", "subject": "a\nb", "body": "x", "segment": seg})), &ah).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", "/api/v1/admin/campaigns", Some(json!({"name": "News", "subject": "Hello {{first_name}}", "body": "Big news for {{email}}", "segment": seg})), &ah).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();

    // Queue: snapshot 2 recipients; b opts out before the worker runs → skipped at send time.
    let q = call(&app, "POST", &format!("/api/v1/admin/campaigns/{id}/send"), None, &ah).await;
    assert_eq!(q.status, StatusCode::OK, "{:?}", q.body);
    assert_eq!(q.body["recipients"], 2);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/campaigns/{id}/send"), None, &ah).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "PUT", &format!("/api/v1/admin/campaigns/{id}"), Some(json!({"name": "x", "subject": "y", "body": "z"})), &ah).await.status, StatusCode::CONFLICT);
    call(&app, "PUT", "/api/v1/client/marketing", Some(json!({"opt_out": true})), &b.h()).await;

    let mut status = String::new();
    for _ in 0..300 {
        pelty_api::worker::run_once(&st, "test-campaign", 20).await.unwrap();
        let d = call(&app, "GET", &format!("/api/v1/admin/campaigns/{id}"), None, &ah).await;
        status = d.body["campaign"]["status"].as_str().unwrap().to_string();
        if status == "sent" {
            assert_eq!(d.body["campaign"]["sent"], 1);
            assert_eq!(d.body["campaign"]["skipped"], 1);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(status, "sent");
    let got: Vec<_> = mailbox().sent.lock().unwrap().iter().filter(|m| m.to == a.email || m.to == b.email || m.to == c.email).cloned().collect();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].to, a.email);
    assert!(got[0].text.contains(&format!("Big news for {}", a.email)));
    let lu = got[0].list_unsubscribe.clone().expect("List-Unsubscribe set on campaign mail");
    assert!(lu.contains("/api/v1/unsubscribe/"));
    let url = got[0].text.lines().find(|l| l.contains("/unsubscribe/")).unwrap().trim().to_string();
    let token = url.rsplit('/').next().unwrap();

    // Unsubscribe: GET never changes anything; POST opts out.
    let l = call(&app, "GET", &format!("/api/v1/unsubscribe/{token}"), None, &[]).await;
    assert_eq!(l.status, StatusCode::OK);
    assert!(l.body["email"].as_str().unwrap().contains("***@"));
    assert_eq!(call(&app, "GET", "/api/v1/client/marketing", None, &a.h()).await.body["opt_out"], false);
    assert_eq!(call(&app, "POST", &format!("/api/v1/unsubscribe/{token}"), None, &[]).await.status, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/v1/client/marketing", None, &a.h()).await.body["opt_out"], true);
    assert_eq!(call(&app, "POST", &format!("/api/v1/unsubscribe/{}", uuid::Uuid::new_v4()), None, &[]).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "POST", "/api/v1/admin/campaigns/preview", Some(json!({"client_group_id": gid})), &ah).await.body["recipients"], 0);

    // A new draft for an empty segment can't be queued; cancel a draft.
    let r = call(&app, "POST", "/api/v1/admin/campaigns", Some(json!({"name": "E", "subject": "s", "body": "b", "segment": {"client_group_id": gid}})), &ah).await;
    let e = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/campaigns/{e}/send"), None, &ah).await.status, StatusCode::CONFLICT);
    assert_eq!(call(&app, "POST", &format!("/api/v1/admin/campaigns/{e}/cancel"), None, &ah).await.status, StatusCode::OK);
    assert_eq!(call(&app, "DELETE", &format!("/api/v1/admin/campaigns/{e}"), None, &ah).await.status, StatusCode::OK);
}
