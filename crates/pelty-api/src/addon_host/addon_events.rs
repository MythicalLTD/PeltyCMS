//! Addon event lifecycle (ABI 0.3) and mixins.
//!
//! Every event goes through four phases, like Bukkit/Minecraft events:
//! - `before`: synchronous, in priority order; listeners may `modify` the
//!   payload or `cancel` it (`uncancel` too when `ignore_cancelled`).
//!   `monitor` listeners only observe. Money and secret rules apply.
//! - `during`: delivered while the action runs (concurrently, observe only).
//! - `after`: the action succeeded; the result is in the payload.
//! - `finished`: always last, with `status` = `ok` | `failed` | `cancelled`.
//!
//! Events are core API requests (`<METHOD> <route template>`), typed core
//! events (`invoice.before_create`, …) and addon events (`<addon>:<name>`,
//! emitted through the `events` WIT interface and declared in `emits`).
//!
//! Mixins wrap core API routes: `before` rewrites the request body,
//! `after` rewrites the JSON response within its schema, `around` does
//! both with shared state, `return` replaces the response (same schema).
//! They need admin approval (`mixins`, `mixins.return`), run under the
//! addon's fuel/time limits and can be switched off per addon.
use crate::AppState;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use pelty_core::{AppError, AppResult};
use pelty_wasm::{split_phase, Hook, Mixin, WasmAddon, PRIORITY_MONITOR};
use serde_json::{json, Value};
use std::sync::Arc;

pub const MIXIN_AT: &[&str] = &["before", "after", "around", "return"];
const KILL_KEY: &str = "addon.mixins_disabled";

/// Result of a `before` phase.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub payload: Value,
    /// `Some(reason)` when the event ended cancelled.
    pub cancelled: Option<String>,
}

pub(crate) fn loaded(s: &AppState) -> Vec<Arc<WasmAddon>> {
    let mut v: Vec<_> = s
        .wasm_addons
        .read()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v.dedup_by(|a, b| a.id == b.id);
    v
}

/// Does a hook/mixin target (phase prefix stripped) match `event`?
/// Request patterns use `before_matches` semantics; other names match
/// exactly or by a trailing `*`.
pub fn target_matches(target: &str, event: &str) -> bool {
    if pelty_wasm::is_request_pattern(target) || pelty_wasm::is_request_pattern(event) {
        return pelty_wasm::is_request_pattern(event)
            && event.split_once(' ').is_some_and(|(m, r)| {
                crate::addon_ext::before_matches(&format!("before:{target}"), m, r)
            });
    }
    match target.strip_suffix('*') {
        Some(p) => event.starts_with(p),
        None => target == event,
    }
}

/// Listeners of `event` in `phase`, sorted by priority then addon id.
/// One hook per addon (its lowest priority match).
pub fn listeners(s: &AppState, phase: &str, event: &str) -> Vec<(Arc<WasmAddon>, Hook)> {
    let mut v: Vec<_> = loaded(s)
        .into_iter()
        .filter_map(|a| {
            a.caps
                .hooks
                .iter()
                .filter(|h| {
                    let (p, t) = split_phase(&h.event);
                    p == phase && target_matches(t, event)
                })
                .min_by_key(|h| h.priority)
                .cloned()
                .map(|h| (a, h))
        })
        .collect();
    v.sort_by(|x, y| {
        x.1.priority
            .cmp(&y.1.priority)
            .then_with(|| x.0.id.cmp(&y.0.id))
    });
    v
}

async fn call_hook(
    s: &AppState,
    a: &Arc<WasmAddon>,
    h: &Hook,
    input: Value,
    depth: u8,
) -> Result<Value, String> {
    let context = json!({ "kind": "hook", "event": input.get("event"), "phase": input.get("phase"), "priority": h.priority, "depth": depth });
    crate::addon_health::guarded_call(
        s,
        a,
        "hook",
        input,
        crate::addon_health::Budget {
            timeout_ms: h.timeout_ms,
            fuel: h.fuel,
        },
        depth,
        context,
    )
    .await
}

/// The `before` phase. Errors only for a failing `fail_open: false` hook.
pub async fn run_before(
    s: &AppState,
    event: &str,
    subs: Vec<(Arc<WasmAddon>, Hook)>,
    payload: Value,
    depth: u8,
) -> AppResult<Outcome> {
    let mut subs = subs;
    subs.sort_by(|x, y| {
        x.1.priority
            .cmp(&y.1.priority)
            .then_with(|| x.0.id.cmp(&y.0.id))
    });
    let mut payload = payload;
    let mut cancelled: Option<String> = None;
    for (a, h) in subs {
        if cancelled.is_some() && !h.ignore_cancelled {
            continue;
        }
        let monitor = h.priority >= PRIORITY_MONITOR;
        let input = json!({ "event": event, "phase": "before", "payload": payload, "cancelled": cancelled.is_some(), "monitor": monitor });
        match call_hook(s, &a, &h, input, depth).await {
            Ok(out) if monitor => {
                if out
                    .get("action")
                    .and_then(Value::as_str)
                    .is_some_and(|x| x != "continue")
                {
                    tracing::warn!(addon = %a.id, event, "monitor listener tried to change the event; ignored");
                }
            }
            Ok(out) => match out
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("continue")
            {
                "cancel" => {
                    let reason = out
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(200)
                        .collect::<String>();
                    tracing::info!(addon = %a.id, event, reason, "event cancelled by addon");
                    cancelled = Some(reason);
                }
                "uncancel" if h.ignore_cancelled => cancelled = None,
                "modify" => match out.get("payload") {
                    Some(p) if p.is_object() => {
                        payload = crate::addon_ext::apply_modify(s, &a, event, &payload, p).await
                    }
                    _ => {
                        tracing::warn!(addon = %a.id, event, "modify without an object payload ignored")
                    }
                },
                _ => {}
            },
            Err(e) if h.fail_open => {
                tracing::warn!(addon = %a.id, event, error = %e, "hook failed (fail-open)")
            }
            Err(e) => {
                tracing::warn!(addon = %a.id, event, error = %e, "hook failed (fail-closed)");
                return Err(AppError::Conflict("error.addon_hook_failed"));
            }
        }
    }
    Ok(Outcome { payload, cancelled })
}

/// Observe-only phases (`during`, `after`, `finished`): delivered in
/// priority order in the background; results are ignored.
pub fn notify(
    s: &AppState,
    phase: &'static str,
    event: &str,
    payload: Value,
    depth: u8,
) -> Option<tokio::task::JoinHandle<()>> {
    let subs = listeners(s, phase, event);
    if subs.is_empty() {
        return None;
    }
    let (s, event) = (s.clone(), event.to_string());
    Some(tokio::spawn(async move {
        for (a, h) in subs {
            let input = json!({ "event": event, "phase": phase, "payload": payload });
            if let Err(e) = call_hook(&s, &a, &h, input, depth).await {
                tracing::warn!(addon = %a.id, event, phase, error = %e, "hook failed");
            }
        }
    }))
}

/// Host side of the `events.emit` import: run an addon event.
pub struct Bus(pub AppState);

impl pelty_wasm::EventBus for Bus {
    fn filter<'a>(
        &'a self,
        addon: &'a str,
        name: &'a str,
        value: Value,
        context: Value,
        depth: u8,
    ) -> pelty_payments::BoxFuture<'a, Value> {
        Box::pin(crate::addon_filters::apply_custom(
            &self.0, addon, name, value, context, depth,
        ))
    }
    fn emit<'a>(
        &'a self,
        addon: &'a str,
        name: &'a str,
        payload: Value,
        depth: u8,
    ) -> pelty_payments::BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            let s = &self.0;
            let subs = listeners(s, "before", name);
            let o = run_before(s, name, subs, payload, depth)
                .await
                .map_err(|_| "a listener failed (fail-closed)".to_string())?;
            let status = if o.cancelled.is_some() {
                "cancelled"
            } else {
                "ok"
            };
            if o.cancelled.is_none() {
                notify(
                    s,
                    "after",
                    name,
                    json!({ "source": addon, "payload": o.payload }),
                    depth,
                );
            }
            notify(
                s,
                "finished",
                name,
                json!({ "source": addon, "status": status, "payload": o.payload }),
                depth,
            );
            let _ = pelty_database::audit::record(
                &s.pool,
                None,
                "addon.event_emitted",
                Some(name),
                None,
                json!({ "addon": addon, "status": status }),
            )
            .await;
            Ok(
                json!({ "cancelled": o.cancelled.is_some(), "reason": o.cancelled, "payload": o.payload }),
            )
        })
    }
}

// ----- mixins -----

/// Load the per-addon mixin kill switch from settings.
pub async fn load_kill_switch(s: &AppState) {
    let v = pelty_database::settings::get(&s.pool, KILL_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or(json!([]));
    let set: std::collections::HashSet<String> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect();
    if let Ok(mut g) = s.mixins_off.write() {
        *g = set;
    }
}

pub fn mixins_enabled(s: &AppState, addon: &str) -> bool {
    s.mixins_off
        .read()
        .map(|g| !g.contains(addon))
        .unwrap_or(true)
}

/// Turn an addon's mixins on or off (persisted, audited by the caller).
pub async fn set_mixins_enabled(s: &AppState, addon: &str, on: bool) -> AppResult<()> {
    let list: Vec<String> = {
        let mut g = s
            .mixins_off
            .write()
            .map_err(|_| AppError::Internal("lock".into()))?;
        if on {
            g.remove(addon);
        } else {
            g.insert(addon.to_string());
        }
        let mut v: Vec<String> = g.iter().cloned().collect();
        v.sort();
        v
    };
    pelty_database::settings::set(&s.pool, KILL_KEY, json!(list)).await
}

/// A mixin target is a core API route pattern: `<METHOD> /api/v1/...`
/// (`GET` allowed, trailing `*` allowed). Addon routes cannot be wrapped.
pub fn mixin_ok(m: &Mixin) -> bool {
    if m.target.starts_with("fn ") {
        // Core-function mixins (see `addon_functions`): no `return`.
        return pelty_extensions::functions::target_function(&m.target).is_some()
            && ["before", "after", "around"].contains(&m.at.as_str())
            && (1..=5000).contains(&m.timeout_ms);
    }
    let Some((meth, path)) = m.target.split_once(' ') else {
        return false;
    };
    MIXIN_AT.contains(&m.at.as_str())
        && ["GET", "POST", "PUT", "PATCH", "DELETE", "*"].contains(&meth)
        && path.starts_with("/api/v1/")
        && !path.starts_with("/api/v1/addons/")
        && !path.starts_with("/api/v1/auth/")
        && path.len() <= 200
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_{}*".contains(&b))
        && (1..=5000).contains(&m.timeout_ms)
}

/// Mixins of `method route`, priority order, kill switch applied.
pub fn mixins_for(s: &AppState, method: &str, route: &str) -> Vec<(Arc<WasmAddon>, Mixin)> {
    let ev = format!("{method} {route}");
    let mut v: Vec<_> = loaded(s)
        .into_iter()
        .filter(|a| mixins_enabled(s, &a.id))
        .flat_map(|a| {
            a.caps
                .mixins
                .iter()
                .filter(|m| target_matches(&m.target, &ev))
                .cloned()
                .map(|m| (a.clone(), m))
                .collect::<Vec<_>>()
        })
        .collect();
    v.sort_by(|x, y| {
        x.1.priority
            .cmp(&y.1.priority)
            .then_with(|| x.0.id.cmp(&y.0.id))
    });
    v
}

/// Conflicts: two addons with a `return` mixin on overlapping targets
/// (only the first by priority applies at runtime).
pub fn conflicts(s: &AppState) -> Vec<Value> {
    let all: Vec<(String, Mixin)> = loaded(s)
        .into_iter()
        .flat_map(|a| {
            a.caps
                .mixins
                .iter()
                .filter(|m| m.at == "return")
                .cloned()
                .map(|m| (a.id.clone(), m))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut out = vec![];
    for (i, (a, x)) in all.iter().enumerate() {
        for (b, y) in &all[i + 1..] {
            if a != b && overlap(&x.target, &y.target) {
                out.push(json!({ "kind": "return", "a": a, "b": b, "target_a": x.target, "target_b": y.target, "winner": if (x.priority, a) <= (y.priority, b) { a } else { b } }));
            }
        }
    }
    out
}

fn overlap(x: &str, y: &str) -> bool {
    let (Some((mx, px)), Some((my, py))) = (x.split_once(' '), y.split_once(' ')) else {
        return false;
    };
    if mx != my && mx != "*" && my != "*" {
        return false;
    }
    let pre = |p: &str| p.strip_suffix('*').map(str::to_string);
    match (pre(px), pre(py)) {
        (Some(a), Some(b)) => a.starts_with(&b) || b.starts_with(&a),
        (Some(a), None) => py.starts_with(&a),
        (None, Some(b)) => px.starts_with(&b),
        (None, None) => px == py,
    }
}

/// `new` keeps the JSON schema of `old`: same type everywhere, objects keep
/// exactly the same keys, arrays may change length/order but each element
/// must match some element of `old` (empty `old` arrays stay empty).
pub fn same_schema(old: &Value, new: &Value) -> bool {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|w| same_schema(v, w)))
        }
        (Value::Array(a), Value::Array(b)) => b.iter().all(|w| a.iter().any(|t| same_schema(t, w))),
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => true,
        // Nullable fields may flip between null and a value.
        (Value::Null, _) | (_, Value::Null) => {
            !old.is_object() && !old.is_array() && !new.is_object() && !new.is_array()
        }
        _ => false,
    }
}

pub(crate) async fn call_mixin(
    s: &AppState,
    a: &Arc<WasmAddon>,
    m: &Mixin,
    input: Value,
) -> Result<Value, String> {
    let context =
        json!({ "kind": "mixin", "target": m.target, "at": m.at, "priority": m.priority });
    crate::addon_health::guarded_call(
        s,
        a,
        "mixin",
        input,
        crate::addon_health::Budget {
            timeout_ms: m.timeout_ms,
            fuel: m.fuel,
        },
        0,
        context,
    )
    .await
}

const RESP_MAX: usize = 1024 * 1024;

/// Middleware for every `/api/v1` request: the request lifecycle
/// (`before`/`during`/`after`/`finished` of `<METHOD> <route>`, mutating
/// methods only) and mixins (any method).
pub async fn lifecycle(
    axum::extract::State(s): axum::extract::State<AppState>,
    req: Request,
    next: axum::middleware::Next,
) -> Response {
    let method = req.method().as_str().to_string();
    let Some(route) = req
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|m| m.as_str().to_string())
    else {
        return next.run(req).await;
    };
    if route.starts_with("/api/v1/addons/") {
        return next.run(req).await;
    }
    let mutating = ["POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str());
    let event = format!("{method} {route}");
    let before = if mutating {
        listeners(&s, "before", &event)
    } else {
        vec![]
    };
    let has_phase = mutating
        && pelty_wasm::PHASES[1..]
            .iter()
            .any(|p| !listeners(&s, p, &event).is_empty());
    let mixins = mixins_for(&s, &method, &route);
    if before.is_empty() && !has_phase && mixins.is_empty() {
        return next.run(req).await;
    }
    let json_body = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.starts_with("application/json"));
    let (mut parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, crate::addon_ext::PRE_MAX_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return crate::error::ApiError(AppError::BadRequest("error.payload_too_large"))
                .into_response()
        }
    };
    let orig: Value = if json_body && !bytes.is_empty() {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let mut saved = vec![];
    let mut body_v = orig.clone();
    crate::addon_ext::redact(&mut body_v, &mut saved, "");
    let redacted_orig = body_v.clone();
    let base = |b: &Value| json!({ "method": method, "path": parts.uri.path(), "route": route, "query": parts.uri.query().unwrap_or(""), "body": b });

    // before (hooks)
    if !before.is_empty() {
        let legacy = format!("before:{event}");
        match run_before(&s, &legacy, before, base(&body_v), 0).await {
            Ok(o) if o.cancelled.is_some() => {
                notify(
                    &s,
                    "finished",
                    &event,
                    json!({ "request": base(&body_v), "status": "cancelled", "reason": o.cancelled }),
                    0,
                );
                return crate::error::ApiError(AppError::Conflict("error.addon_hook_cancelled"))
                    .into_response();
            }
            Ok(o) => body_v = o.payload.get("body").cloned().unwrap_or(Value::Null),
            Err(e) => {
                notify(
                    &s,
                    "finished",
                    &event,
                    json!({ "request": base(&body_v), "status": "failed" }),
                    0,
                );
                return crate::error::ApiError::from(e).into_response();
            }
        }
    }
    // before / around (mixins): rewrite the input
    let mut states: Vec<Value> = vec![Value::Null; mixins.len()];
    if json_body && body_v.is_object() {
        for (i, (a, m)) in mixins
            .iter()
            .enumerate()
            .filter(|(_, (_, m))| m.at == "before" || m.at == "around")
        {
            match call_mixin(&s, a, m, json!({ "target": m.target, "at": m.at, "phase": "before", "request": base(&body_v) })).await {
                Ok(out) => {
                    if let Some(b) = out.get("body").filter(|b| b.is_object()) {
                        body_v = crate::addon_ext::apply_modify(&s, a, &event, &body_v, b).await;
                    }
                    states[i] = out.get("state").cloned().unwrap_or(Value::Null);
                }
                Err(e) => tracing::warn!(addon = %a.id, target = %m.target, error = %e, "mixin failed; skipped"),
            }
        }
    }
    let new_bytes = if json_body && body_v != redacted_orig {
        let mut b = body_v.clone();
        crate::addon_ext::restore(&mut b, saved);
        let v = axum::body::Bytes::from(b.to_string());
        parts
            .headers
            .insert(axum::http::header::CONTENT_LENGTH, v.len().into());
        v
    } else {
        bytes
    };
    let req_info = base(&body_v);
    // during: runs concurrently with the handler
    if mutating {
        notify(&s, "during", &event, json!({ "request": req_info }), 0);
    }
    let resp = next
        .run(Request::from_parts(
            parts,
            axum::body::Body::from(new_bytes),
        ))
        .await;
    let status = resp.status();
    let wants_out = mixins.iter().any(|(_, m)| m.at != "before");
    let is_json = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.starts_with("application/json"));
    if !status.is_success() || !is_json || (!wants_out && !has_phase) {
        if mutating && has_phase {
            // Success without a JSON body (e.g. 204) still has an after phase.
            if status.is_success() {
                notify(
                    &s,
                    "after",
                    &event,
                    json!({ "request": req_info, "http_status": status.as_u16(), "response": Value::Null }),
                    0,
                );
            }
            notify(
                &s,
                "finished",
                &event,
                json!({ "request": req_info, "status": if status.is_success() { "ok" } else { "failed" }, "http_status": status.as_u16() }),
                0,
            );
        }
        return resp;
    }
    let (mut rparts, rbody) = resp.into_parts();
    let Ok(rbytes) = axum::body::to_bytes(rbody, RESP_MAX).await else {
        return crate::error::ApiError(AppError::Internal("response too large for mixins".into()))
            .into_response();
    };
    let mut out: Value = serde_json::from_slice(&rbytes).unwrap_or(Value::Null);
    let mut rsaved = vec![];
    crate::addon_ext::redact(&mut out, &mut rsaved, "");
    let out_orig = out.clone();
    let mut returned = false;
    for (i, (a, m)) in mixins
        .iter()
        .enumerate()
        .filter(|(_, (_, m))| m.at != "before")
    {
        if m.at == "return" && returned {
            tracing::warn!(addon = %a.id, target = %m.target, "conflicting return mixin skipped");
            continue;
        }
        let input = json!({ "target": m.target, "at": m.at, "phase": "after", "request": req_info, "status": status.as_u16(), "response": out, "state": states[i] });
        match call_mixin(&s, a, m, input).await {
            Ok(r) => {
                if let Some(nb) = r.get("response") {
                    if !same_schema(&out, nb) {
                        tracing::warn!(addon = %a.id, target = %m.target, "mixin output changed the schema; ignored");
                        continue;
                    }
                    out = crate::addon_ext::apply_modify(&s, a, &event, &out, nb).await;
                    returned |= m.at == "return";
                }
            }
            Err(e) => {
                tracing::warn!(addon = %a.id, target = %m.target, error = %e, "mixin failed; skipped")
            }
        }
    }
    if mutating && has_phase {
        notify(
            &s,
            "after",
            &event,
            json!({ "request": req_info, "http_status": status.as_u16(), "response": out }),
            0,
        );
        notify(
            &s,
            "finished",
            &event,
            json!({ "request": req_info, "status": "ok", "http_status": status.as_u16() }),
            0,
        );
    }
    let body = if out != out_orig {
        crate::addon_ext::restore(&mut out, rsaved);
        let b = axum::body::Bytes::from(out.to_string());
        rparts
            .headers
            .insert(axum::http::header::CONTENT_LENGTH, b.len().into());
        b
    } else {
        rbytes
    };
    Response::from_parts(rparts, axum::body::Body::from(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema() {
        assert!(same_schema(
            &json!({"a": 1, "b": [{"x": "y"}]}),
            &json!({"a": 2, "b": [{"x": "z"}, {"x": "w"}]})
        ));
        assert!(!same_schema(
            &json!({"a": 1}),
            &json!({"a": 1, "extra": true})
        ));
        assert!(!same_schema(&json!({"a": 1}), &json!({"a": "1"})));
        assert!(!same_schema(&json!({"a": []}), &json!({"a": [1]})));
        assert!(same_schema(&json!({"a": null}), &json!({"a": "x"})));
        assert!(!same_schema(&json!({"a": null}), &json!({"a": {}})));
    }

    #[test]
    fn matching() {
        assert!(target_matches(
            "POST /api/v1/admin/*",
            "POST /api/v1/admin/invoices"
        ));
        assert!(target_matches("*", "DELETE /api/v1/x"));
        assert!(!target_matches(
            "POST /api/v1/admin/*",
            "GET /api/v1/admin/x"
        ));
        assert!(target_matches("a:b.*", "a:b.c"));
        assert!(!target_matches("a:b", "a:bc"));
        assert!(!target_matches("*", "a:b"));
        assert_eq!(split_phase("during:a:b"), ("during", "a:b"));
        assert_eq!(
            split_phase("invoice.before_create"),
            ("before", "invoice.before_create")
        );
    }

    #[test]
    fn overlaps() {
        assert!(overlap(
            "GET /api/v1/client/*",
            "GET /api/v1/client/invoices"
        ));
        assert!(overlap("* /api/v1/a", "POST /api/v1/a"));
        assert!(!overlap("GET /api/v1/a", "POST /api/v1/a"));
    }

    #[test]
    fn mixin_targets() {
        let m = |t: &str, at: &str| Mixin {
            target: t.into(),
            at: at.into(),
            priority: 0,
            timeout_ms: 500,
            fuel: None,
        };
        assert!(mixin_ok(&m("GET /api/v1/client/invoices", "after")));
        assert!(!mixin_ok(&m("GET /api/v1/addons/x/y", "after")));
        assert!(!mixin_ok(&m("POST /api/v1/auth/login", "before")));
        assert!(!mixin_ok(&m("GET /api/v1/x", "replace")));
        assert!(mixin_ok(&m("fn checkout.place", "before")));
        assert!(mixin_ok(&m("fn cart.view", "around")));
        assert!(!mixin_ok(&m("fn cart.view", "return")));
        assert!(!mixin_ok(&m("fn unknown.fn", "after")));
    }
}
