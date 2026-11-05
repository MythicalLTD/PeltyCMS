//! Redis paths of `pelty_api::shared`. Run with TEST_REDIS_URL set (e.g.
//! redis://127.0.0.1:6379/15); skipped otherwise. Uses unique key names.
use pelty_api::shared::{RedisCounter, Shared};
use pelty_auth::rate_limit::RateLimiter;
use std::sync::Arc;
use std::time::Duration;

fn url() -> Option<String> {
    std::env::var("TEST_REDIS_URL").ok().filter(|v| !v.is_empty())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rate_limits_are_shared_between_instances() {
    let Some(url) = url() else { return eprintln!("TEST_REDIS_URL unset; skipped") };
    let key = format!("t:{}", uuid::Uuid::new_v4());
    let (a, b) = (RateLimiter::new(3, Duration::from_secs(60)), RateLimiter::new(3, Duration::from_secs(60)));
    a.set_shared(Arc::new(RedisCounter(Arc::new(Shared::new(Some(url.clone()))))));
    b.set_shared(Arc::new(RedisCounter(Arc::new(Shared::new(Some(url))))));
    assert!(a.check(&key) && b.check(&key));
    assert!(!a.exceeded(&key));
    assert!(b.check(&key));
    // The third hit was on instance b; a sees the budget used up.
    assert!(a.exceeded(&key));
    assert!(!a.check(&key));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window_expires_and_down_redis_falls_back() {
    let Some(url) = url() else { return eprintln!("TEST_REDIS_URL unset; skipped") };
    let key = format!("t:{}", uuid::Uuid::new_v4());
    let r = RateLimiter::new(1, Duration::from_millis(300));
    r.set_shared(Arc::new(RedisCounter(Arc::new(Shared::new(Some(url))))));
    assert!(r.check(&key));
    assert!(!r.check(&key));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(r.check(&key), "window expired in redis");
    // Unreachable Redis: local counting, still limited.
    let down = RateLimiter::new(1, Duration::from_secs(60));
    down.set_shared(Arc::new(RedisCounter(Arc::new(Shared::new(Some("redis://127.0.0.1:1/0".into()))))));
    assert!(down.check("x"));
    assert!(!down.check("x"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scheduler_lease_has_one_holder_and_moves_on_expiry() {
    let Some(url) = url() else { return eprintln!("TEST_REDIS_URL unset; skipped") };
    let key = format!("pelty:test:leader:{}", uuid::Uuid::new_v4());
    let s = Shared::new(Some(url));
    let ttl = Duration::from_millis(400);
    assert_eq!(s.lease(&key, "w1", ttl).await, Some(true));
    assert_eq!(s.lease(&key, "w2", ttl).await, Some(false));
    // renewal by the holder
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(s.lease(&key, "w1", ttl).await, Some(true));
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(s.lease(&key, "w2", ttl).await, Some(false), "renewed lease still held");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(s.lease(&key, "w2", ttl).await, Some(true), "expired lease taken over");
    assert!(s.ping().await.is_ok());
    assert!(s.status_line().await.starts_with("redis: ok"));
    let off = Shared::new(None);
    assert_eq!(off.lease(&key, "w1", ttl).await, None);
    assert!(off.status_line().await.starts_with("redis: off"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_writes_are_published_to_other_instances() {
    use futures_util::StreamExt;
    let Some(url) = url() else { return eprintln!("TEST_REDIS_URL unset; skipped") };
    let (_, mut state) = crate::common::app().await;
    state.shared = Arc::new(Shared::new(Some(url.clone())));
    let app = pelty_api::router(state.clone());
    let admin = crate::common::sess(&app, &state, "admin").await;
    let mut ps = redis::Client::open(url).unwrap().get_async_pubsub().await.unwrap();
    ps.subscribe(pelty_api::cluster::channel(&state)).await.unwrap();
    let mut msgs = ps.into_on_message();
    let r = crate::common::call(&app, "PUT", "/api/v1/admin/automation/tickets.escalate/settings", Some(serde_json::json!({ "enabled": true, "run_hour": null })), &admin.h()).await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let msg = tokio::time::timeout(Duration::from_secs(5), msgs.next()).await.expect("published").unwrap();
    let v: serde_json::Value = serde_json::from_str(&msg.get_payload::<String>().unwrap()).unwrap();
    assert_eq!(v["topic"], "settings");
    assert_eq!(v["from"], state.shared.instance.as_str());
    // Reads are not published.
    let r = crate::common::call(&app, "GET", "/api/v1/admin/automation", None, &admin.h()).await;
    assert!(r.status.is_success());
    assert!(tokio::time::timeout(Duration::from_millis(500), msgs.next()).await.is_err(), "GET published");
}
