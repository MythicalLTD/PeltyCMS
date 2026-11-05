//! Example addon used by the generated developer guides. Every snippet
//! between `// docs:start:<name>` and `// docs:end:<name>` is copied into the
//! docs, and `crates/pelty-api/tests/integration/addon_guide.rs` installs this addon and
//! exercises each feature, so the samples always compile and work.
use pelty_addon_sdk::{host, reply, serde_json, serde_json::json, serde_json::Value};

fn counter(k: &str) -> u64 {
    host::kv_get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
}

// docs:start:kv
/// Addon-private key/value storage (`capabilities.kv: true`).
fn bump(k: &str) {
    let _ = host::kv_set(k, &(counter(k) + 1).to_string());
}
// docs:end:kv

// docs:start:listen
/// Domain events listed in `capabilities.events` arrive here after the fact.
/// They cannot be cancelled; use them to sync, notify or count.
fn on_event(name: &str, payload: &Value) {
    match name {
        "invoice.paid" => bump("paid"),
        "user.registered" => {
            bump("registered");
            let _ = host::kv_set("last_user", payload["user_id"].as_str().unwrap_or(""));
        }
        _ => {}
    }
}
// docs:end:listen

// docs:start:hooks
/// Pre-events and lifecycle phases listed in `capabilities.hooks` call op
/// `hook`. Return `proceed`, `modify(payload)` or `cancel(reason)`.
fn hook(input: &Value) -> Value {
    let event = input["event"].as_str().unwrap_or("");
    if input["phase"].as_str().unwrap_or("before") != "before" {
        return reply::proceed(); // during/after/finished only observe
    }
    let mut p = input["payload"].clone();
    bump(&format!("hook:{event}"));
    match event {
        // Cancel: refuse sign-in for one domain.
        "login.before" if p["email"].as_str().is_some_and(|e| e.ends_with("@denied.test")) => reply::cancel("domain not allowed"),
        // Modify: tidy up the display name before the account is created.
        "client.before_register" => {
            p["display_name"] = json!(p["display_name"].as_str().unwrap_or("").trim().to_string());
            reply::modify(p)
        }
        // Own custom event (declared as `before:integration-guide:greeted`,
        // delivered with `event` = the bare name and `phase` = "before").
        "integration-guide:greeted" if p["name"] == "nobody" => reply::cancel("no greeting for nobody"),
        "order.before_place" => order_hook(input, p),
        _ => reply::proceed(),
    }
}
// docs:end:hooks

// docs:start:uncancel
/// `order.before_place` with `ignore_cancelled: true` and priority `highest`:
/// this runs even after an earlier addon cancelled the order. `uncancel` is
/// only honoured for such hooks. Here: always allow orders using the
/// `GUIDE-VIP` coupon, and normalise coupon codes to upper case (`coupon` is
/// the only key the host reads back from this event).
fn order_hook(input: &Value, mut p: Value) -> Value {
    let coupon = p["coupon"].as_str().unwrap_or("").trim().to_uppercase();
    if input["cancelled"] == true {
        return if coupon == "GUIDE-VIP" { reply::uncancel() } else { reply::proceed() };
    }
    if coupon.is_empty() {
        return reply::proceed();
    }
    p["coupon"] = json!(coupon);
    reply::modify(p)
}
// docs:end:uncancel

// docs:start:mixin
/// Mixins wrap a core route (`capabilities.mixins`). An `after` mixin may
/// change values of keys the response already has; new keys are dropped.
fn mixin(input: &Value) -> Value {
    let mut resp = input["response"].clone();
    // Opt-in per request so this example never changes other callers.
    let wanted = input["request"]["query"].as_str().unwrap_or("").contains("guide=1");
    if input["at"] == "after" && wanted {
        if let Some(c) = resp["city"].as_str() {
            resp["city"] = json!(c.to_uppercase());
        }
        return json!({ "response": resp });
    }
    json!({})
}
// docs:end:mixin

// docs:start:emit
/// Emit a custom event (`capabilities.emits`, name `<addon-id>:<event>`).
/// The `before` phase runs synchronously, so listeners can cancel or modify it.
fn greet(name: &str) -> Result<Value, String> {
    let e = pelty_addon_sdk::emit("integration-guide:greeted", &json!({ "name": name }))?;
    if e.cancelled {
        return Ok(json!({ "status": 409, "body": { "error": e.reason } }));
    }
    bump("greeted");
    Ok(json!({ "status": 200, "body": { "greeted": e.payload["name"] } }))
}
// docs:end:emit

// docs:start:settings
/// Admin settings (addon.json `settings`) and secrets (`capabilities.secrets`).
fn greeting() -> String {
    let _token_present = host::secret("api_token").is_some();
    pelty_addon_sdk::setting("greeting").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "Hello".into())
}
// docs:end:settings

// docs:start:http
/// Outbound HTTP to hosts in `capabilities.http` only.
#[allow(dead_code)]
fn ping_example() -> Result<u16, String> {
    let (status, _body) = pelty_addon_sdk::fetch("GET", "https://example.com/", vec![], String::new())?;
    Ok(status)
}
// docs:end:http

// docs:start:routes
/// Routes in `capabilities.routes` are mounted at `/api/v1/addons/<id>/…`
/// and call op `http` with `{method, path, query, params, body, user}`.
fn http(input: &Value) -> Result<Value, String> {
    let m = input["method"].as_str().unwrap_or("");
    let path = input["path"].as_str().unwrap_or("");
    Ok(match (m, path) {
        ("GET", "/hello") => json!({ "status": 200, "body": { "message": format!("{}, world", greeting()) } }),
        ("POST", "/greet") => greet(input["body"]["name"].as_str().unwrap_or(""))?,
        ("GET", "/stats") => {
            let rows: Vec<Value> = ["paid", "registered", "greeted", "cleanups", "hook:client.before_register"].iter().map(|k| json!({ "key": k, "value": counter(k) })).collect();
            json!({ "status": 200, "body": { "rows": rows } })
        }
        _ => json!({ "status": 404, "body": null }),
    })
}
// docs:end:routes

// docs:start:guest
struct GuideAddon;
impl pelty_addon_sdk::Guest for GuideAddon {
    fn describe() -> String {
        json!({ "id": "guide", "name": "Addon guide example" }).to_string()
    }
    fn call(op: String, input: String) -> Result<String, String> {
        let v: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        let out = match op.as_str() {
            "hook" => hook(&v),
            "mixin" => mixin(&v),
            "http" => http(&v)?,
            // docs:start:jobs
            // `capabilities.jobs` runs this op every `every_secs` seconds.
            "cleanup" => {
                bump("cleanups");
                json!({ "ok": true })
            }
            // docs:end:jobs
            _ => return Err(format!("unknown op {op}")),
        };
        Ok(out.to_string())
    }
    fn on_event(name: String, payload: String) -> Result<(), String> {
        let v: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
        on_event(&name, &v);
        Ok(())
    }
}
pelty_addon_sdk::export!(GuideAddon with_types_in pelty_addon_sdk);
// docs:end:guest
