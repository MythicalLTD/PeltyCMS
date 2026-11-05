//! Optional Redis (`PELTY_REDIS_URL`) shared by every instance: rate-limit
//! counters and the scheduler leader lock. Without it (or while it is
//! unreachable) everything stays local: in-memory limits, and every instance
//! with `PELTY_SCHEDULER=true` schedules (jobs are deduplicated in Postgres).
//! Cross-instance invalidation and the instance list live in [`crate::cluster`].
use pelty_auth::rate_limit::SharedCounter;
use redis::aio::ConnectionManager;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PREFIX: &str = "pelty:";
pub const LEADER_KEY: &str = "pelty:scheduler:leader";

/// Rate-limit counter: INCR, and set the window on the first hit. A script
/// (not `PEXPIRE NX`) so it works on Redis 5 and 6 too.
const INCR_SCRIPT: &str = "local n = redis.call('INCR', KEYS[1]) if n == 1 then redis.call('PEXPIRE', KEYS[1], ARGV[1]) end return n";

pub struct Shared {
    /// This process: `<host>-<pid>-<random>`; scheduler lease holder and
    /// heartbeat key (`pelty:instance:<id>`).
    pub instance: String,
    url: Option<String>,
    conn: tokio::sync::Mutex<Option<ConnectionManager>>,
    last_try: Mutex<Option<Instant>>,
    last_error: Mutex<Option<String>>,
}

impl Shared {
    pub fn new(url: Option<String>) -> Self {
        let host = std::env::var("HOSTNAME").ok().or_else(|| std::fs::read_to_string("/etc/hostname").ok()).map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).unwrap_or_else(|| "host".into());
        let instance = format!("{host}-{}-{}", std::process::id(), &uuid::Uuid::new_v4().simple().to_string()[..6]);
        Shared { instance, url, conn: Default::default(), last_try: Mutex::new(None), last_error: Mutex::new(None) }
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn enabled(&self) -> bool {
        self.url.is_some()
    }

    /// The connection, opened on first use; reconnects are handled by the
    /// manager. A failed open is retried at most every 10 s.
    pub async fn conn(&self) -> Option<ConnectionManager> {
        let url = self.url.as_deref()?;
        let mut g = self.conn.lock().await;
        if let Some(c) = g.as_ref() {
            return Some(c.clone());
        }
        {
            let mut t = self.last_try.lock().unwrap_or_else(|e| e.into_inner());
            if t.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
                return None;
            }
            *t = Some(Instant::now());
        }
        let res = async {
            let client = redis::Client::open(url).map_err(|e| e.to_string())?;
            // One plain connection first: the manager retries a refused
            // port until the timeout and would only report "timeout".
            let mut probe = tokio::time::timeout(Duration::from_secs(3), client.get_multiplexed_async_connection()).await.map_err(|_| "connect timeout".to_string())?.map_err(|e| e.to_string())?;
            let _: String = redis::cmd("PING").query_async(&mut probe).await.map_err(|e| e.to_string())?;
            tokio::time::timeout(Duration::from_secs(3), ConnectionManager::new(client)).await.map_err(|_| "connect timeout".to_string())?.map_err(|e| e.to_string())
        }
        .await;
        match res {
            Ok(c) => {
                *g = Some(c.clone());
                *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Some(c)
            }
            Err(e) => {
                tracing::warn!(error = %e, "redis unavailable; using local state");
                *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                None
            }
        }
    }

    /// `PING` round trip, for `status` and the setup wizard.
    pub async fn ping(&self) -> Result<Duration, String> {
        let Some(mut c) = self.conn().await else {
            return Err(self.last_error.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| "not configured".into()));
        };
        let t = Instant::now();
        let r: Result<String, _> = tokio::time::timeout(Duration::from_secs(2), redis::cmd("PING").query_async(&mut c)).await.map_err(|_| "timeout".to_string())?;
        r.map(|_| t.elapsed()).map_err(|e| e.to_string())
    }

    /// One-line health for the console `status` command.
    pub async fn status_line(&self) -> String {
        if !self.enabled() {
            return "redis: off (local rate limits; scheduler on every instance with PELTY_SCHEDULER=true)".into();
        }
        match self.ping().await {
            Ok(d) => format!("redis: ok ({} ms)", d.as_millis()),
            Err(e) => format!("redis: DOWN ({e}); using local state"),
        }
    }

    async fn incr(&self, key: &str, window: Duration) -> Option<u32> {
        let mut c = self.conn().await?;
        let k = format!("{PREFIX}rl:{key}");
        redis::cmd("EVAL").arg(INCR_SCRIPT).arg(1).arg(&k).arg(window.as_millis() as u64).query_async(&mut c).await.ok()
    }

    async fn get(&self, key: &str) -> Option<u32> {
        let mut c = self.conn().await?;
        let v: Option<u32> = redis::cmd("GET").arg(format!("{PREFIX}rl:{key}")).query_async(&mut c).await.ok()?;
        Some(v.unwrap_or(0))
    }

    /// Try to hold the scheduler lease for `ttl`. `None`: Redis unavailable.
    pub async fn lease(&self, key: &str, me: &str, ttl: Duration) -> Option<bool> {
        let mut c = self.conn().await?;
        let ms = ttl.as_millis() as u64;
        let got: Option<String> = redis::cmd("SET").arg(key).arg(me).arg("NX").arg("PX").arg(ms).query_async(&mut c).await.ok()?;
        if got.is_some() {
            return Some(true);
        }
        let holder: Option<String> = redis::cmd("GET").arg(key).query_async(&mut c).await.ok()?;
        if holder.as_deref() == Some(me) {
            let _: Result<i64, _> = redis::cmd("PEXPIRE").arg(key).arg(ms).query_async(&mut c).await;
            return Some(true);
        }
        Some(false)
    }
}

/// Whether this worker should run the scheduler pass now. Without Redis:
/// always (as before). With Redis: only the instance holding the lease
/// (renewed each pass, expires after three missed passes). If Redis is down
/// every instance schedules; the Postgres job keys keep that harmless.
pub async fn is_scheduler_leader(s: &crate::AppState, worker: &str, every: Duration) -> bool {
    if !s.shared.enabled() {
        return true;
    }
    s.shared.lease(LEADER_KEY, worker, every * 3).await.unwrap_or(true)
}

/// Bridges the synchronous [`pelty_auth::rate_limit::RateLimiter`] to Redis.
/// Used only on a multi-threaded runtime (the server); elsewhere, or after a
/// 250 ms timeout, it reports "unavailable" and the limiter counts locally.
pub struct RedisCounter(pub Arc<Shared>);

impl RedisCounter {
    fn block<T>(&self, f: impl std::future::Future<Output = Option<T>>) -> Option<T> {
        let h = tokio::runtime::Handle::try_current().ok()?;
        if h.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
            return None;
        }
        tokio::task::block_in_place(|| h.block_on(async { tokio::time::timeout(Duration::from_millis(250), f).await.ok().flatten() }))
    }
}

impl SharedCounter for RedisCounter {
    fn incr(&self, key: &str, window: Duration) -> Option<u32> {
        self.block(self.0.incr(key, window))
    }
    fn get(&self, key: &str) -> Option<u32> {
        self.block(self.0.get(key))
    }
}
