//! Test fixture (not bundled): hook isolation (deadline, breaker), filters,
//! core-function mixins and the call context.
use pelty_addon_sdk::{host, op, reply, serde_json::json, serde_json::Value};

fn hook(input: Value) -> Result<Value, String> {
    let p = &input["payload"];
    match input["event"].as_str().unwrap_or("") {
        // Triage: refuse spam, tag and escalate everything else.
        "ticket.before_open" => {
            let subject = p["subject"].as_str().unwrap_or("");
            if subject.contains("spam-fixture") {
                return Ok(reply::cancel("no spam"));
            }
            // Only test tickets: other suites may share the database.
            let Some(rest) = subject.strip_prefix("triage:") else { return Ok(reply::proceed()) };
            let subject = rest.trim();
            let mut out = p.clone();
            out["subject"] = json!(format!("[triaged] {subject}"));
            out["priority"] = json!("urgent");
            return Ok(reply::modify(out));
        }
        // Redaction of a known secret in replies.
        "ticket.before_reply" => {
            let mut out = p.clone();
            out["body"] = json!(p["body"].as_str().unwrap_or("").replace("hunter2", "[redacted]"));
            return Ok(reply::modify(out));
        }
        _ => {}
    }
    if p["body"]["city"] == "Spin" {
        // Runs until the host interrupts the store at the hook deadline.
        let mut x: u64 = 0;
        loop {
            x = std::hint::black_box(x.wrapping_add(1));
            if x == u64::MAX {
                break;
            }
        }
    }
    Ok(reply::proceed())
}

/// Filters: hide `hidden-fixture` products; a `pricey-fixture` product gets
/// a doubled price, which the host must refuse (no `money.modify`).
fn filter(input: Value) -> Result<Value, String> {
    let mut v = input["value"].clone();
    if input["filter"] == "catalog.public" {
        let products = v["products"].as_array().cloned().unwrap_or_default();
        let keep: Vec<Value> = products.into_iter().filter(|p| !p["name"].as_str().unwrap_or("").starts_with("hidden-fixture")).collect();
        let pricey = keep.iter().any(|p| p["name"].as_str().unwrap_or("").starts_with("pricey-fixture"));
        v["products"] = json!(keep);
        if pricey {
            for p in v["products"].as_array_mut().into_iter().flatten() {
                for pr in p["prices"].as_array_mut().into_iter().flatten() {
                    pr["amount"] = json!("999999.00");
                }
            }
        }
    }
    // Subscribed to its own point: the host must skip the applying addon.
    if input["filter"] == "integration-hooks:greeting" {
        v["text"] = json!(format!("{} [self!]", v["text"].as_str().unwrap_or("")));
    }
    if input["filter"] == "invoice.view" && v["notes"].as_str().is_some_and(|n| n.starts_with("filter-fixture")) {
        v["notes"] = json!(format!("{} [filtered]", v["notes"].as_str().unwrap_or("")));
    }
    Ok(json!({ "value": v }))
}

fn http(input: Value) -> Result<Value, String> {
    let m = input["method"].as_str().unwrap_or("");
    let path = input["path"].as_str().unwrap_or("");
    Ok(match (m, path) {
        ("GET", "/state") => json!({ "status": 200, "body": { "last": host::kv_get("last") } }),
        // Own filter point: other addons rewrite the greeting.
        ("GET", "/greeting") => json!({ "status": 200, "body": pelty_addon_sdk::apply_filter("integration-hooks:greeting", &json!({ "text": "hello" }), &json!({ "who": "test" })) }),
        _ => json!({ "status": 404, "body": null }),
    })
}

// Exported through the SDK's typed dispatcher (`integration_addon!`).
struct Hooks;
impl pelty_addon_sdk::Addon for Hooks {
    fn describe() -> Value {
        json!({ "id": "hooks", "name": "Hook power" })
    }
    fn hook(h: &op::Hook) -> Result<Value, String> {
        hook(h.raw.clone())
    }
    fn filter(f: &op::Filter) -> Result<Value, String> {
        filter(f.raw.clone())
    }
    fn http(r: &op::Http) -> Result<Value, String> {
        http(r.raw.clone())
    }
    /// `fn ticket.open` (around): `fnmix:` subjects are rewritten before the
    /// ticket is created; the after phase marks the returned subject, using
    /// the state from the before phase.
    fn mixin(m: &op::Mixin) -> Result<Value, String> {
        match (m.function.as_deref(), m.phase.as_str()) {
            (Some("ticket.open"), "before") => {
                let subject = m.input["subject"].as_str().unwrap_or("");
                let Some(rest) = subject.strip_prefix("fnmix:") else { return Ok(reply::mixin_keep()) };
                let mut i = m.input.clone();
                i["subject"] = json!(format!("[fn] {}", rest.trim()));
                // The mark is only set when `host context` (ABI 0.4) reports this mixin.
                let ctx = pelty_addon_sdk::context();
                let mark = if ctx["kind"] == "mixin" && ctx["target"] == "fn ticket.open" { "✓" } else { "?" };
                Ok(reply::fn_input(Some(i), json!({ "mark": mark, "customer": m.context["customer_id"] })))
            }
            (Some("ticket.open"), "after") if m.state["mark"].is_string() => {
                let mut o = m.output.clone();
                o["ticket"]["subject"] = json!(format!("{} {}", o["ticket"]["subject"].as_str().unwrap_or(""), m.state["mark"].as_str().unwrap_or("")));
                Ok(reply::fn_output(o))
            }
            _ => Ok(reply::mixin_keep()),
        }
    }
}
pelty_addon_sdk::integration_addon!(Hooks);
