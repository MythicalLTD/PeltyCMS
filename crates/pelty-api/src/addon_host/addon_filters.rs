//! Value filters (`capabilities.filters`): the host passes a value through
//! every subscribed addon in priority order (op `filter`, input
//! `{filter, value, context}`, reply `{value}`). A reply is kept only when
//! it has the same JSON schema as the value; money keys follow the
//! `money.modify` rules. Filters always fail open.
use crate::AppState;
use serde_json::{json, Value};
use std::sync::Arc;

/// Addons subscribed to `name`, in run order (priority, then id).
fn subscribers(s: &AppState, name: &str) -> Vec<(Arc<pelty_wasm::WasmAddon>, pelty_wasm::Filter)> {
    let mut v: Vec<_> = crate::addon_ext::loaded_all(s)
        .into_iter()
        .filter_map(|a| a.filter(name).cloned().map(|f| (a, f)))
        .collect();
    v.dedup_by(|x, y| x.0.id == y.0.id);
    v.sort_by(|x, y| {
        x.1.priority
            .cmp(&y.1.priority)
            .then_with(|| x.0.id.cmp(&y.0.id))
    });
    v
}

/// True when at least one addon filters `name` (lets callers skip work).
pub fn has(s: &AppState, name: &str) -> bool {
    crate::addon_ext::loaded_all(s)
        .iter()
        .any(|a| a.filter(name).is_some())
}

/// Run filter `name` over `value`. `money`: `false` reverts every money
/// change even for addons holding `money.modify` (stored, immutable data).
pub async fn apply_with(
    s: &AppState,
    name: &str,
    value: Value,
    context: Value,
    money: bool,
) -> Value {
    debug_assert!(
        pelty_extensions::filters::is_filter(name),
        "unregistered filter {name}"
    );
    run(s, name, value, context, money, 0, None).await
}

/// An addon-defined filter point (`<addon>:<point>`) applied by `addon`
/// through WIT `filters.apply`. The applying addon is skipped (no
/// self-recursion), nested calls carry `depth`, and money never changes.
pub async fn apply_custom(
    s: &AppState,
    addon: &str,
    name: &str,
    value: Value,
    context: Value,
    depth: u8,
) -> Value {
    if depth > pelty_wasm::runtime::MAX_ADDON_DEPTH || !name.starts_with(&format!("{addon}:")) {
        return value;
    }
    run(s, name, value, context, false, depth, Some(addon)).await
}

async fn run(
    s: &AppState,
    name: &str,
    value: Value,
    context: Value,
    money: bool,
    depth: u8,
    skip: Option<&str>,
) -> Value {
    let mut value = value;
    for (a, f) in subscribers(s, name) {
        if skip == Some(a.id.as_str()) {
            continue;
        }
        let input = json!({ "filter": name, "value": value, "context": context });
        let ctx =
            json!({ "kind": "filter", "filter": name, "priority": f.priority, "depth": depth });
        let out = match crate::addon_health::guarded_call(
            s,
            &a,
            "filter",
            input,
            crate::addon_health::Budget {
                timeout_ms: f.timeout_ms,
                fuel: f.fuel,
            },
            depth,
            ctx,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(addon = %a.id, filter = name, error = %e, "filter failed, value unchanged");
                continue;
            }
        };
        let Some(new) = out.get("value") else {
            continue;
        };
        if let Some(v) = accept(s, &a, &format!("filter:{name}"), &value, new, money).await {
            value = v;
        }
    }
    value
}

/// Check an addon's replacement `new` for `old`: same JSON schema, and money
/// changes only when `money` is allowed here and the addon holds
/// `money.modify` (each change audited under `label`). `None`: keep `old`.
pub async fn accept(
    s: &AppState,
    a: &pelty_wasm::WasmAddon,
    label: &str,
    old: &Value,
    new: &Value,
    money: bool,
) -> Option<Value> {
    if !crate::addon_events::same_schema(old, new) {
        tracing::warn!(addon = %a.id, label, "addon changed the value's schema, ignored");
        return None;
    }
    let mut changes = vec![];
    money_diff_aligned("", old, new, &mut changes);
    if changes.is_empty() {
        return Some(new.clone());
    }
    if !money || !a.caps.money.modify {
        tracing::warn!(addon = %a.id, label, "addon changed money values without permission, ignored");
        return None;
    }
    if let Some(bad) = changes
        .iter()
        .find(|c| crate::addon_ext::exact_money(&c.2).is_none())
    {
        tracing::warn!(addon = %a.id, label, path = %bad.0, "invalid money value, ignored");
        return None;
    }
    for (path, from, to) in &changes {
        let _ = pelty_database::audit::record(
            &s.pool,
            None,
            "addon.money_modified",
            Some(label),
            None,
            json!({ "addon": a.id, "path": path, "from": from, "to": to }),
        )
        .await;
    }
    Some(new.clone())
}

/// Like `addon_ext::money_diff`, but array elements carrying an `id` are
/// matched by id, so removing or reordering entries is not a money change.
/// An element whose id was not in the original counts as fully changed.
pub fn money_diff_aligned(path: &str, a: &Value, b: &Value, out: &mut Vec<(String, Value, Value)>) {
    let id = |v: &Value| {
        v.get("id")
            .filter(|x| x.is_string() || x.is_number())
            .cloned()
    };
    match (a, b) {
        (Value::Array(x), Value::Array(y))
            if !x.is_empty() && x.iter().chain(y).all(|v| id(v).is_some()) =>
        {
            for (i, nb) in y.iter().enumerate() {
                let p = format!("{path}/{i}");
                match x.iter().find(|o| id(o) == id(nb)) {
                    Some(na) => money_diff_aligned(&p, na, nb, out),
                    None => crate::addon_ext::money_diff(&p, &Value::Null, nb, out),
                }
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for (k, va) in x {
                let vb = y.get(k).unwrap_or(&Value::Null);
                let p = format!("{path}/{k}");
                if crate::addon_ext::MONEY_KEYS.contains(&k.as_str())
                    && !va.is_object()
                    && !va.is_array()
                {
                    if va != vb {
                        out.push((p, va.clone(), vb.clone()));
                    }
                } else {
                    money_diff_aligned(&p, va, vb, out);
                }
            }
        }
        _ => crate::addon_ext::money_diff(path, a, b, out),
    }
}

/// [`apply_with`] with the usual money rules.
pub async fn apply(s: &AppState, name: &str, value: Value, context: Value) -> Value {
    apply_with(s, name, value, context, true).await
}

/// `ui.contributions`: keep only removal, reordering and slot `order`
/// changes of the original entries.
pub fn restrict_ui(orig: &Value, new: &Value) -> Value {
    // Pages are identified by plugin + id; slot entries by their whole
    // content except `order` (an addon may fill one slot more than once).
    let key = |v: &Value, list: &str| -> String {
        if list == "pages" {
            format!(
                "{}\u{0}{}",
                v["plugin"].as_str().unwrap_or(""),
                v["id"].as_str().unwrap_or("")
            )
        } else {
            let mut v = v.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("order");
            }
            v.to_string()
        }
    };
    let pick = |list: &str, order: bool| -> Vec<Value> {
        let src = orig[list].as_array().cloned().unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        new[list]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| {
                let id = key(n, list);
                let o = src.iter().find(|o| key(o, list) == id)?.clone();
                if !seen.insert(id) {
                    return None;
                }
                let mut o = o;
                if order {
                    if let Some(x) = n.get("order").filter(|x| x.is_i64() || x.is_null()) {
                        o["order"] = x.clone();
                    }
                }
                Some(o)
            })
            .collect()
    };
    json!({ "pages": pick("pages", false), "slots": pick("slots", true) })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn removing_priced_entries_is_not_a_money_change() {
        let a = json!({ "products": [{ "id": "1", "prices": [{ "id": "p1", "amount": "5.00" }] }, { "id": "2", "prices": [{ "id": "p2", "amount": "9.00" }] }] });
        let mut out = vec![];
        super::money_diff_aligned(
            "",
            &a,
            &json!({ "products": [{ "id": "2", "prices": [{ "id": "p2", "amount": "9.00" }] }] }),
            &mut out,
        );
        assert!(out.is_empty(), "{out:?}");
        super::money_diff_aligned(
            "",
            &a,
            &json!({ "products": [{ "id": "2", "prices": [{ "id": "p2", "amount": "1.00" }] }] }),
            &mut out,
        );
        assert_eq!(out.len(), 1);
        out.clear();
        super::money_diff_aligned(
            "",
            &a,
            &json!({ "products": [{ "id": "3", "prices": [{ "id": "p3", "amount": "1.00" }] }] }),
            &mut out,
        );
        assert!(!out.is_empty());
    }

    #[test]
    fn ui_restriction_keeps_only_known_entries() {
        let orig = json!({ "pages": [{ "plugin": "a", "id": "p1", "title": "A" }, { "plugin": "b", "id": "p2", "title": "B" }], "slots": [{ "plugin": "a", "slot": "client.dashboard", "order": 1 }] });
        let new = json!({ "pages": [{ "plugin": "b", "id": "p2", "title": "evil", "frame": { "src": "https://x" } }, { "plugin": "z", "id": "new" }], "slots": [{ "plugin": "a", "slot": "client.dashboard", "order": 9 }, { "plugin": "a", "slot": "client.dashboard", "body": "injected" }] });
        let r = super::restrict_ui(&orig, &new);
        assert_eq!(
            r["pages"],
            json!([{ "plugin": "b", "id": "p2", "title": "B" }])
        );
        assert_eq!(r["slots"].as_array().unwrap().len(), 1);
        assert_eq!(r["slots"][0]["order"], 9);
    }
}
