//! Installs `extensions/examples/addon-guide` (the addon whose snippets the
//! generated developer guides embed) and checks each documented feature works.
use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

const ID: &str = "integration-guide";
const DIR: &str = "../examples/addon-guide";
const KRATE: &str = "pelty-addon-guide";

async fn stat(app: &axum::Router, admin: &Sess, key: &str) -> u64 {
    let r = call(app, "GET", &format!("/api/v1/addons/{ID}/stats"), None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["rows"].as_array().unwrap().iter().find(|x| x["key"] == key).and_then(|x| x["value"].as_u64()).unwrap()
}

#[tokio::test]
async fn guide_addon_features_work() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    sqlx::query("DELETE FROM installed_addons WHERE id = $1").bind(ID).execute(&st.pool).await.unwrap();
    sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1").bind(ID).execute(&st.pool).await.unwrap();
    pelty_api::addons::accept_package(&st, &fixture_zip(DIR, KRATE), true).await.expect("guide package");
    let base = format!("/api/v1/admin/addons/{ID}");
    // Iframe UI, request-lifecycle hooks and mixins are dangerous: the admin must tick each.
    let r = call(&app, "GET", "/api/v1/admin/addons", None, &admin.h()).await;
    let e = r.body["items"].as_array().unwrap().iter().find(|a| a["id"] == ID).cloned().expect("listed");
    assert_eq!(e["capabilities"]["dangerous"], json!(["ui.iframe", "hooks.any", "mixins"]));
    assert_eq!(call(&app, "POST", &format!("{base}/install"), None, &admin.h()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(&app, "POST", &format!("{base}/install"), Some(json!({"approve": ["ui.iframe", "hooks.any", "mixins"], "approve_iframe": true})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(call(&app, "POST", &format!("{base}/enable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);

    // routes + settings: public route answers with the default greeting.
    let r = call(&app, "GET", &format!("/api/v1/addons/{ID}/hello"), None, &[]).await;
    assert_eq!((r.status, r.body["message"].clone()), (StatusCode::OK, json!("Hello, world")), "{:?}", r.body);
    // admin route refuses customers.
    let alice = sess(&app, &st, "customer").await;
    assert_eq!(call(&app, "GET", &format!("/api/v1/addons/{ID}/stats"), None, &alice.h()).await.status, StatusCode::FORBIDDEN);

    // hooks: login.before cancel.
    let denied = format!("x{}@denied.test", uuid::Uuid::new_v4().simple());
    st.auth.register(pelty_auth::RegisterInput { email: &denied, password: "Very-Long-Pass-123", display_name: "D" }, "customer", None).await.unwrap();
    let r = call(&app, "POST", "/api/v1/auth/login", Some(json!({"email": denied, "password": "Very-Long-Pass-123"})), &[]).await;
    assert_eq!((r.status, r.body["error"]["code"].clone()), (StatusCode::CONFLICT, json!("error.addon_hook_cancelled")), "{:?}", r.body);

    // hooks: client.before_register modify (trim) + user.registered listener.
    let e = format!("g{}@example.com", uuid::Uuid::new_v4().simple());
    let before = stat(&app, &admin, "registered").await;
    let r = call(&app, "POST", "/api/v1/auth/register", Some(json!({"email": e, "password": "Very-Long-Pass-123", "display_name": "  Spaced  "})), &[]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let dn: String = sqlx::query_scalar("SELECT display_name FROM users WHERE email = $1").bind(&e).fetch_one(&st.pool).await.unwrap();
    assert_eq!(dn, "Spaced");
    let mut ok = false;
    for _ in 0..100 {
        if stat(&app, &admin, "registered").await > before {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ok, "user.registered never reached on_event");

    // custom event: emitted, and our own before-hook cancels "nobody".
    let greet = format!("/api/v1/addons/{ID}/greet");
    let r = call(&app, "POST", &greet, Some(json!({"name": "ana"})), &alice.h()).await;
    assert_eq!((r.status, r.body["greeted"].clone()), (StatusCode::OK, json!("ana")), "{:?}", r.body);
    let r = call(&app, "POST", &greet, Some(json!({"name": "nobody"})), &alice.h()).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(stat(&app, &admin, "greeted").await, 1);

    // mixin: after-mixin rewrites an existing key only when asked.
    assert_eq!(call(&app, "PUT", "/api/v1/client/profile", Some(json!({"city": "Vienna"})), &alice.h()).await.status, StatusCode::OK);
    let p = call(&app, "GET", "/api/v1/client/profile?guide=1", None, &alice.h()).await;
    assert_eq!(p.body["city"], "VIENNA", "{:?}", p.body);
    let p = call(&app, "GET", "/api/v1/client/profile", None, &alice.h()).await;
    assert_eq!(p.body["city"], "Vienna");

    // ui: pages and slots are indexed for the right areas.
    let ui = call(&app, "GET", "/api/v1/plugins/ui", None, &admin.h()).await;
    let s = ui.body.to_string();
    assert!(s.contains("plugin.integration-guide.stats_title") && s.contains("admin.invoices.toolbar"), "{s}");

    // jobs: the declared op is accepted by the job worker.
    let out = pelty_api::addon_ext::run_job(&st, &json!({"addon": ID, "op": "cleanup", "input": {}})).await.ok().expect("cleanup job runs");
    assert_eq!(out, json!({"ok": true}));
    assert_eq!(stat(&app, &admin, "cleanups").await, 1);
    assert!(pelty_api::addon_ext::run_job(&st, &json!({"addon": ID, "op": "undeclared", "input": {}})).await.is_err());

    // Leave the shared test database as we found it.
    assert_eq!(call(&app, "POST", &format!("{base}/disable"), None, &admin.h()).await.status, StatusCode::NO_CONTENT);
}
