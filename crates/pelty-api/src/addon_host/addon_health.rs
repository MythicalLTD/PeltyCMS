//! Addon call isolation: every hook, mixin and filter call goes through
//! [`guarded_call`], which
//! - gives the wasm store the listener's own deadline and fuel (the store
//!   is interrupted, not merely abandoned, when the budget runs out),
//! - records per-addon statistics (calls, failures, timeouts, latency), and
//! - trips a per-addon circuit breaker after `PELTY_ADDON_BREAKER_FAILS`
//!   (default 5) consecutive failures: the addon is skipped for
//!   `PELTY_ADDON_BREAKER_COOLDOWN_SECS` (default 60) and then retried.
//!
//! Admins read the numbers at `GET /api/v1/admin/addons/{id}/health` and
//! reset the breaker with `POST /api/v1/admin/addons/{id}/health/reset`.
use crate::AppState;
use pelty_wasm::WasmAddon;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone, Serialize)]
pub struct Stats {
    pub calls: u64,
    pub failures: u64,
    pub timeouts: u64,
    /// Calls skipped while the breaker was open.
    pub skipped: u64,
    pub consecutive_failures: u32,
    pub total_ms: u64,
    pub max_ms: u64,
    pub last_error: Option<String>,
    /// Unix seconds of the last failure.
    pub last_error_at: Option<i64>,
    /// Unix seconds until which the breaker stays open.
    pub tripped_until: Option<i64>,
    /// Per call kind (`hook`, `mixin`, `filter`, `fn`): calls / failures.
    pub by_kind: std::collections::BTreeMap<String, (u64, u64)>,
    #[serde(skip)]
    open_until: Option<Instant>,
}

#[derive(Default)]
pub struct Health(Mutex<HashMap<String, Stats>>);

/// Consecutive failed hook/mixin/filter calls that trip an addon's circuit breaker (default 5).
const ENV_FAILS: &str = "PELTY_ADDON_BREAKER_FAILS";
/// Seconds a tripped addon is skipped before it is retried (default 60).
const ENV_COOL: &str = "PELTY_ADDON_BREAKER_COOLDOWN_SECS";

fn env_num(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Health {
    pub fn get(&self, addon: &str) -> Stats {
        self.0.lock().ok().and_then(|m| m.get(addon).cloned()).unwrap_or_default()
    }
    pub fn all(&self) -> Vec<(String, Stats)> {
        let mut v: Vec<_> = self.0.lock().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
    pub fn reset(&self, addon: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(addon);
        }
    }
    /// `true` while the breaker is open (the call is then counted as skipped).
    pub fn open(&self, addon: &str) -> bool {
        let Ok(mut m) = self.0.lock() else { return false };
        let st = m.entry(addon.to_string()).or_default();
        match st.open_until {
            Some(t) if Instant::now() < t => {
                st.skipped += 1;
                true
            }
            Some(_) => {
                // Half-open: let the next call through; one more failure re-trips.
                st.open_until = None;
                st.tripped_until = None;
                st.consecutive_failures = env_num(ENV_FAILS, 5).saturating_sub(1) as u32;
                false
            }
            None => false,
        }
    }
    pub fn record(&self, addon: &str, kind: &str, elapsed: Duration, err: Option<&str>, timed_out: bool) {
        let Ok(mut m) = self.0.lock() else { return };
        let st = m.entry(addon.to_string()).or_default();
        let ms = elapsed.as_millis() as u64;
        st.calls += 1;
        st.total_ms += ms;
        st.max_ms = st.max_ms.max(ms);
        let k = st.by_kind.entry(kind.to_string()).or_default();
        k.0 += 1;
        match err {
            None => st.consecutive_failures = 0,
            Some(e) => {
                k.1 += 1;
                st.failures += 1;
                st.timeouts += u64::from(timed_out);
                st.consecutive_failures += 1;
                st.last_error = Some(e.chars().take(300).collect());
                st.last_error_at = Some(now_unix());
                let limit = env_num(ENV_FAILS, 5).max(1) as u32;
                if st.consecutive_failures >= limit {
                    let cool = env_num(ENV_COOL, 60);
                    st.open_until = Some(Instant::now() + Duration::from_secs(cool));
                    st.tripped_until = Some(now_unix() + cool as i64);
                    tracing::warn!(target: "pelty::addon_audit", addon, failures = st.consecutive_failures, cool, "addon circuit breaker tripped");
                }
            }
        }
    }
}

/// Budget of one guarded call.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub timeout_ms: u64,
    pub fuel: Option<u64>,
}

/// Call op `kind` (`hook`, `mixin`, `filter`; also the stats kind) on `a`
/// with the per-listener budget, the breaker and stats. `context` is what the
/// addon reads through `host.context()`.
pub async fn guarded_call(s: &AppState, a: &Arc<WasmAddon>, kind: &str, input: Value, budget: Budget, depth: u8, context: Value) -> Result<Value, String> {
    if s.addon_health.open(&a.id) {
        return Err("circuit breaker open".into());
    }
    let timeout = Duration::from_millis(budget.timeout_ms.clamp(1, 5000));
    let ctx = pelty_wasm::CallCtx { settings: crate::wasm_addons::addon_settings(s, &a.id).await, event_depth: depth, timeout: Some(timeout), fuel: budget.fuel, context, ..Default::default() };
    let started = Instant::now();
    // The store stops itself at `timeout`; the outer timeout only guards
    // against a blocked host call.
    let r = match tokio::time::timeout(timeout + Duration::from_millis(250), a.call_async(ctx, kind.to_string(), input)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("{kind} timed out")),
    };
    let err = r.as_ref().err().map(String::as_str);
    let timed_out = err.is_some_and(|e| e.contains("timed out") || e.contains("fuel"));
    s.addon_health.record(&a.id, kind, started.elapsed(), err, timed_out);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaker_trips_and_recovers() {
        let h = Health::default();
        for _ in 0..4 {
            h.record("a", "hook", Duration::from_millis(3), Some("boom"), false);
        }
        assert!(!h.open("a"));
        h.record("a", "hook", Duration::from_millis(3), Some("timed out"), true);
        assert!(h.open("a"));
        let st = h.get("a");
        assert_eq!((st.calls, st.failures, st.timeouts, st.skipped), (5, 5, 1, 1));
        assert!(st.tripped_until.is_some());
        h.reset("a");
        assert!(!h.open("a"));
        h.record("a", "filter", Duration::from_millis(1), None, false);
        assert_eq!(h.get("a").consecutive_failures, 0);
        assert_eq!(h.get("a").by_kind.get("filter"), Some(&(1, 0)));
    }
}
