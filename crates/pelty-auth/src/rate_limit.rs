//! In-memory fixed-window rate limiter (per key, e.g. IP). Single-node only.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    limit: AtomicU32,
    window: Duration,
    hits: Mutex<HashMap<String, (Instant, u32)>>,
    shared: std::sync::OnceLock<std::sync::Arc<dyn SharedCounter>>,
}

/// A counter store shared by several instances (Redis). `None` from either
/// method means "unavailable right now": the limiter then counts locally.
pub trait SharedCounter: Send + Sync {
    /// Count one hit for `key` in a fixed `window`; returns the new count.
    fn incr(&self, key: &str, window: Duration) -> Option<u32>;
    /// Current count for `key` (0 when absent), without counting.
    fn get(&self, key: &str) -> Option<u32>;
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        RateLimiter {
            limit: AtomicU32::new(limit),
            window,
            hits: Mutex::new(HashMap::new()),
            shared: std::sync::OnceLock::new(),
        }
    }

    /// Count in a shared store from now on (set once, at startup).
    pub fn set_shared(&self, s: std::sync::Arc<dyn SharedCounter>) {
        let _ = self.shared.set(s);
    }

    /// Change the per-window budget at runtime (admin security policy).
    pub fn set_limit(&self, limit: u32) {
        self.limit.store(limit.max(1), Ordering::Relaxed);
    }
    pub fn limit(&self) -> u32 {
        self.limit.load(Ordering::Relaxed)
    }

    /// Returns true if the request is allowed.
    pub fn check(&self, key: &str) -> bool {
        if let Some(n) = self.shared.get().and_then(|s| s.incr(key, self.window)) {
            return n <= self.limit.load(Ordering::Relaxed);
        }
        let now = Instant::now();
        // A poisoned lock must not disable limiting (fail closed on state).
        let mut m = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() > 100_000 {
            m.retain(|_, (t, _)| now.duration_since(*t) < self.window);
        }
        let e = m.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(e.0) >= self.window {
            *e = (now, 0);
        }
        e.1 += 1;
        e.1 <= self.limit.load(Ordering::Relaxed)
    }

    /// True if `key` already used up its budget in the current window.
    /// Does not count as a hit (used to throttle only failed attempts).
    pub fn exceeded(&self, key: &str) -> bool {
        if let Some(n) = self.shared.get().and_then(|s| s.get(key)) {
            return n >= self.limit.load(Ordering::Relaxed);
        }
        let m = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        m.get(key).is_some_and(|(t, n)| Instant::now().duration_since(*t) < self.window && *n >= self.limit.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits() {
        let r = RateLimiter::new(2, Duration::from_secs(60));
        assert!(r.check("a"));
        assert!(r.check("a"));
        assert!(!r.check("a"));
        assert!(r.check("b"));
    }
    #[test]
    fn shared_store_wins_and_falls_back() {
        struct Fake(Mutex<HashMap<String, u32>>, bool);
        impl SharedCounter for Fake {
            fn incr(&self, k: &str, _: Duration) -> Option<u32> {
                self.1.then(|| { let mut m = self.0.lock().unwrap(); let n = m.entry(k.into()).or_default(); *n += 1; *n })
            }
            fn get(&self, k: &str) -> Option<u32> {
                self.1.then(|| self.0.lock().unwrap().get(k).copied().unwrap_or(0))
            }
        }
        let store = std::sync::Arc::new(Fake(Mutex::new(HashMap::from([("a".to_string(), 1)])), true));
        let r = RateLimiter::new(2, Duration::from_secs(60));
        r.set_shared(store.clone());
        // Another instance already counted one hit for "a".
        assert!(r.check("a"));
        assert!(r.exceeded("a"));
        assert!(!r.check("a"));
        let down = RateLimiter::new(1, Duration::from_secs(60));
        down.set_shared(std::sync::Arc::new(Fake(Mutex::new(HashMap::new()), false)));
        assert!(down.check("x"));
        assert!(!down.check("x"));
    }
    #[test]
    fn exceeded_does_not_count() {
        let r = RateLimiter::new(1, Duration::from_secs(60));
        assert!(!r.exceeded("k"));
        assert!(!r.exceeded("k"));
        assert!(r.check("k"));
        assert!(r.exceeded("k"));
    }
}
