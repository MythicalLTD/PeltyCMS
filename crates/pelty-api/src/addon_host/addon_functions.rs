//! Core-function mixins: addons wrap a named host function
//! (`pelty_extensions::functions::FUNCTIONS`) with a mixin whose target is
//! `fn <name>`. Op `mixin`, input
//! `{function, at, phase: "before", context, input}` → reply `{input?, state?}`,
//! then `{function, at, phase: "after", context, input, output, state}` →
//! reply `{output?}`. Replies keep the JSON schema and never change money
//! values; mixins fail open and honour the per-addon kill switch.
use crate::AppState;
use pelty_core::AppResult;
use pelty_wasm::{Mixin, WasmAddon};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::future::Future;
use std::sync::Arc;

/// Mixins on function `name`, in run order (priority, then addon id).
pub fn mixins(s: &AppState, name: &str) -> Vec<(Arc<WasmAddon>, Mixin)> {
    let mut v: Vec<_> = crate::addon_events::loaded(s)
        .into_iter()
        .filter(|a| crate::addon_events::mixins_enabled(s, &a.id))
        .flat_map(|a| a.caps.mixins.iter().filter(|m| pelty_extensions::functions::target_function(&m.target) == Some(name)).cloned().map(|m| (a.clone(), m)).collect::<Vec<_>>())
        .collect();
    v.sort_by(|x, y| x.1.priority.cmp(&y.1.priority).then_with(|| x.0.id.cmp(&y.0.id)));
    v
}

/// Input-only functions (no serializable output, e.g. a remote call): run
/// the `before` phase of `before`/`around` mixins and return the rewritten
/// input. `after` mixins are not called for these functions.
pub async fn rewrite_input<I>(s: &AppState, name: &'static str, context: Value, input: I) -> I
where
    I: Serialize + DeserializeOwned,
{
    debug_assert!(pelty_extensions::functions::is_function(name), "unregistered function {name}");
    let ms = mixins(s, name);
    if ms.iter().all(|(_, m)| m.at == "after") {
        return input;
    }
    let label = format!("fn:{name}");
    let mut iv = serde_json::to_value(&input).unwrap_or(Value::Null);
    if iv.is_null() {
        return input;
    }
    for (a, m) in ms.iter().filter(|(_, m)| m.at != "after") {
        let req = json!({ "function": name, "at": m.at, "phase": "before", "context": context, "input": iv });
        match crate::addon_events::call_mixin(s, a, m, req).await {
            Ok(out) => {
                if let Some(new) = out.get("input") {
                    if let Some(v) = crate::addon_filters::accept(s, a, &label, &iv, new, false).await {
                        iv = v;
                    }
                }
            }
            Err(e) => tracing::warn!(addon = %a.id, function = name, error = %e, "function mixin failed; skipped"),
        }
    }
    serde_json::from_value(iv).unwrap_or(input)
}

/// Run core function `name` through its mixins. Without mixins this is just
/// `core(input)`. A rewritten input/output that no longer deserializes into
/// `I`/`O` is dropped (the original is used).
pub async fn run<I, O, F, Fut>(s: &AppState, name: &'static str, context: Value, input: I, core: F) -> AppResult<O>
where
    I: Serialize + DeserializeOwned,
    O: Serialize + DeserializeOwned,
    F: FnOnce(I) -> Fut,
    Fut: Future<Output = AppResult<O>>,
{
    debug_assert!(pelty_extensions::functions::is_function(name), "unregistered function {name}");
    let ms = mixins(s, name);
    if ms.is_empty() {
        return core(input).await;
    }
    let label = format!("fn:{name}");
    let mut iv = serde_json::to_value(&input).unwrap_or(Value::Null);
    let mut states = vec![Value::Null; ms.len()];
    for (i, (a, m)) in ms.iter().enumerate().filter(|(_, (_, m))| m.at != "after") {
        let req = json!({ "function": name, "at": m.at, "phase": "before", "context": context, "input": iv });
        match crate::addon_events::call_mixin(s, a, m, req).await {
            Ok(out) => {
                if let Some(new) = out.get("input").filter(|_| !iv.is_null()) {
                    if let Some(v) = crate::addon_filters::accept(s, a, &label, &iv, new, false).await {
                        iv = v;
                    }
                }
                states[i] = out.get("state").cloned().unwrap_or(Value::Null);
            }
            Err(e) => tracing::warn!(addon = %a.id, function = name, error = %e, "function mixin failed; skipped"),
        }
    }
    let input = match serde_json::from_value::<I>(iv.clone()) {
        Ok(i) => i,
        Err(_) => {
            tracing::warn!(function = name, "rewritten input does not fit; original used");
            iv = serde_json::to_value(&input).unwrap_or(Value::Null);
            input
        }
    };
    let out = core(input).await?;
    if ms.iter().all(|(_, m)| m.at == "before") {
        return Ok(out);
    }
    let mut ov = serde_json::to_value(&out).unwrap_or(Value::Null);
    for (i, (a, m)) in ms.iter().enumerate().filter(|(_, (_, m))| m.at != "before") {
        let req = json!({ "function": name, "at": m.at, "phase": "after", "context": context, "input": iv, "output": ov, "state": states[i] });
        match crate::addon_events::call_mixin(s, a, m, req).await {
            Ok(r) => {
                if let Some(new) = r.get("output") {
                    if let Some(v) = crate::addon_filters::accept(s, a, &label, &ov, new, false).await {
                        ov = v;
                    }
                }
            }
            Err(e) => tracing::warn!(addon = %a.id, function = name, error = %e, "function mixin failed; skipped"),
        }
    }
    Ok(serde_json::from_value(ov).unwrap_or(out))
}
