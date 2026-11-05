//! Several instances behind one Redis (`PELTY_REDIS_URL`): change
//! notifications and the live instance list. Without Redis every function
//! here is a no-op and an instance only sees its own changes (as before).
//!
//! PeltyCMS has no server push (SSE / websockets) to fan out; what has to
//! cross instances is in-process state loaded from Postgres: the security
//! policy and sign-in limit, IP rules, trusted proxies, SMTP settings, the
//! active theme, plugin states, translation overrides, the mixin kill switch
//! and the loaded WASM addons (gateways, provisioning, registrars, captcha,
//! SSO). After any successful admin write ([`publish_admin_writes`]) the
//! instance publishes a topic on `pelty:bus` (`pelty:bus:<db>` off db 0); every other instance reloads
//! that part from the database.
use crate::AppState;
use axum::{extract::Request, extract::State, middleware::Next, response::Response};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

pub const CHANNEL: &str = "pelty:bus";

/// Pub/sub ignores the database index, so the channel carries it: two
/// deployments on one Redis (different `/<db>`) do not reload each other.
pub fn channel(s: &AppState) -> String {
    let db = s
        .shared
        .url()
        .and_then(|u| redis::Client::open(u).ok())
        .map_or(0, |c| c.get_connection_info().redis.db);
    if db == 0 {
        CHANNEL.to_string()
    } else {
        format!("{CHANNEL}:{db}")
    }
}
const INSTANCE_PREFIX: &str = "pelty:instance:";
const HEARTBEAT: Duration = Duration::from_secs(10);

/// Subscription state for `status`: 0 off, 1 connecting, 2 subscribed.
static BUS: AtomicU8 = AtomicU8::new(0);

/// What changed. `Addons` reloads the WASM addons, `Settings` everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Settings,
    Addons,
}

impl Topic {
    pub fn as_str(self) -> &'static str {
        match self {
            Topic::Settings => "settings",
            Topic::Addons => "addons",
        }
    }
    pub fn parse(s: &str) -> Option<Topic> {
        match s {
            "settings" => Some(Topic::Settings),
            "addons" => Some(Topic::Addons),
            _ => None,
        }
    }
    /// Topic for an admin write to `path` (`/api/v1/admin/...`).
    pub fn for_path(path: &str) -> Topic {
        let p = path.to_ascii_lowercase();
        if [
            "addon",
            "gateway",
            "plugin",
            "extension",
            "theme",
            "mixin",
            "provisioning",
            "registrar",
            "captcha",
            "sso",
        ]
        .iter()
        .any(|k| p.contains(k))
        {
            Topic::Addons
        } else {
            Topic::Settings
        }
    }
}

/// Tell the other instances that `topic` changed. Best effort.
pub async fn publish(s: &AppState, topic: Topic) {
    let Some(mut c) = s.shared.conn().await else {
        return;
    };
    let msg = json!({ "from": s.shared.instance, "topic": topic.as_str() }).to_string();
    if let Err(e) = redis::cmd("PUBLISH")
        .arg(channel(s))
        .arg(msg)
        .query_async::<i64>(&mut c)
        .await
    {
        tracing::warn!(error = %e, "cluster publish failed");
    }
}

/// Middleware: after a successful POST/PUT/PATCH/DELETE under
/// `/api/v1/admin/`, publish the matching topic.
pub async fn publish_admin_writes(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let write = !matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    );
    let path = req.uri().path().to_string();
    let res = next.run(req).await;
    if write
        && s.shared.enabled()
        && res.status().is_success()
        && path.starts_with("/api/v1/admin/")
    {
        let s = s.clone();
        tokio::spawn(async move { publish(&s, Topic::for_path(&path)).await });
    }
    res
}

/// Reload `topic` from the database (what another instance changed).
pub async fn apply(s: &AppState, topic: Topic) {
    let r: pelty_core::AppResult<()> = async {
        match topic {
            Topic::Settings => {
                pelty_auth::policy::invalidate();
                crate::security::invalidate_caches();
                crate::trusted_proxy::invalidate();
                s.auth_limiter.set_limit(
                    pelty_auth::policy::load(&s.pool)
                        .await?
                        .login_rate_per_minute
                        .unwrap_or(s.cfg.auth_rate_per_minute),
                );
                crate::routes::smtp::reload(s).await?;
                if let Err(e) = crate::translations::reload(s).await {
                    tracing::warn!(error = %e, "translation overrides not reloaded");
                }
                crate::addon_events::load_kill_switch(s).await;
            }
            Topic::Addons => {
                for (id, on) in pelty_database::extensions::plugin_states(&s.pool).await? {
                    s.plugins.set_enabled(&id, on);
                }
                let id = crate::routes::extensions::load_active_id(s).await?;
                s.set_active_theme(&id);
                crate::addon_events::load_kill_switch(s).await;
                crate::wasm_addons::sync(s).await?;
            }
        }
        Ok(())
    }
    .await;
    match r {
        Ok(()) => tracing::info!(
            topic = topic.as_str(),
            "reloaded after a change on another instance"
        ),
        Err(e) => tracing::error!(topic = topic.as_str(), error = %e, "cluster reload failed"),
    }
}

/// Start the subscriber and the heartbeat (only with Redis).
pub fn spawn(s: AppState) {
    if !s.shared.enabled() {
        return;
    }
    let hb = s.clone();
    tokio::spawn(async move {
        loop {
            heartbeat(&hb).await;
            tokio::time::sleep(HEARTBEAT).await;
        }
    });
    tokio::spawn(async move {
        loop {
            BUS.store(1, Ordering::Relaxed);
            if let Err(e) = listen(&s).await {
                tracing::warn!(error = %e, "cluster subscription lost; retrying");
            }
            BUS.store(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn listen(s: &AppState) -> Result<(), String> {
    use futures_util::StreamExt;
    let client =
        redis::Client::open(s.shared.url().unwrap_or_default()).map_err(|e| e.to_string())?;
    let mut ps = tokio::time::timeout(Duration::from_secs(5), client.get_async_pubsub())
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| e.to_string())?;
    ps.subscribe(channel(s)).await.map_err(|e| e.to_string())?;
    BUS.store(2, Ordering::Relaxed);
    let mut stream = ps.into_on_message();
    while let Some(msg) = stream.next().await {
        let Ok(body) = msg.get_payload::<String>() else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        if v["from"].as_str() == Some(s.shared.instance.as_str()) {
            continue;
        }
        let Some(mut topics) = v["topic"].as_str().and_then(Topic::parse).map(|t| vec![t]) else {
            continue;
        };
        // Coalesce a burst (one admin save can be several writes).
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Some(Some(m)) = futures_util::FutureExt::now_or_never(stream.next()) {
            if let Some(t) = m
                .get_payload::<String>()
                .ok()
                .and_then(|b| serde_json::from_str::<Value>(&b).ok())
                .filter(|v| v["from"].as_str() != Some(s.shared.instance.as_str()))
                .and_then(|v| v["topic"].as_str().and_then(Topic::parse))
            {
                if !topics.contains(&t) {
                    topics.push(t);
                }
            }
        }
        for t in topics {
            apply(s, t).await;
        }
    }
    Err("stream ended".into())
}

async fn heartbeat(s: &AppState) {
    let Some(mut c) = s.shared.conn().await else {
        return;
    };
    let v = json!({
        "id": s.shared.instance,
        "version": env!("CARGO_PKG_VERSION"),
        "scheduler": s.cfg.scheduler,
        "bind": s.cfg.bind.to_string(),
        "seen": time::OffsetDateTime::now_utc().unix_timestamp(),
    });
    let _: Result<(), _> = redis::cmd("SET")
        .arg(format!("{INSTANCE_PREFIX}{}", s.shared.instance))
        .arg(v.to_string())
        .arg("PX")
        .arg((HEARTBEAT * 3).as_millis() as u64)
        .query_async(&mut c)
        .await;
}

/// Live instances (heartbeat within 30 s), plus the scheduler lease holder.
pub async fn instances(s: &AppState) -> Option<(Vec<Value>, Option<String>)> {
    let mut c = s.shared.conn().await?;
    let mut keys: Vec<String> = vec![];
    let mut cursor: u64 = 0;
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(format!("{INSTANCE_PREFIX}*"))
            .arg("COUNT")
            .arg(200)
            .query_async(&mut c)
            .await
            .ok()?;
        keys.extend(batch);
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    let mut out = vec![];
    for k in keys {
        if let Ok(Some(v)) = redis::cmd("GET")
            .arg(&k)
            .query_async::<Option<String>>(&mut c)
            .await
        {
            out.extend(serde_json::from_str::<Value>(&v).ok());
        }
    }
    out.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let leader: Option<String> = redis::cmd("GET")
        .arg(crate::shared::LEADER_KEY)
        .query_async(&mut c)
        .await
        .ok()?;
    Some((out, leader))
}

/// `pubsub: subscribed` etc. for the console `status`.
pub fn bus_state() -> &'static str {
    match BUS.load(Ordering::Relaxed) {
        2 => "subscribed",
        1 => "connecting",
        _ => "off",
    }
}

#[cfg(test)]
mod tests {
    use super::Topic;

    #[test]
    fn topics_by_admin_path() {
        assert_eq!(
            Topic::for_path("/api/v1/admin/addons/x/enable"),
            Topic::Addons
        );
        assert_eq!(
            Topic::for_path("/api/v1/admin/gateways/stripe"),
            Topic::Addons
        );
        assert_eq!(
            Topic::for_path("/api/v1/admin/security/policy"),
            Topic::Settings
        );
        assert_eq!(Topic::parse(Topic::Addons.as_str()), Some(Topic::Addons));
        assert_eq!(Topic::parse("nope"), None);
    }
}
