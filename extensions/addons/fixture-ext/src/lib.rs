//! Test fixture (not bundled): pre-event hooks, HTTP routes and declarative
//! UI. `login.before` cancels `@blocked.test`; `client.before_register`
//! upper-cases the display name; `invoice.before_create` prefixes line
//! descriptions; `order.before_place` traps on coupon `BOOM` (fail-closed)
//! and spins forever on `SLOW` (timeout).
use pelty_addon_sdk::{host, serde_json, serde_json::json, serde_json::Value};

fn counter(k: &str) -> u64 {
    host::kv_get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn bump(k: &str) {
    let _ = host::kv_set(k, &(counter(k) + 1).to_string());
}

fn hook(input: Value) -> Result<Value, String> {
    let event = input["event"].as_str().unwrap_or("");
    let mut p = input["payload"].clone();
    bump(&format!("hook:{event}"));
    Ok(match event {
        "login.before"
            if p["email"]
                .as_str()
                .is_some_and(|e| e.ends_with("@blocked.test")) =>
        {
            json!({ "action": "cancel", "reason": "blocked domain" })
        }
        "client.before_register" => {
            let d = p["display_name"].as_str().unwrap_or("").to_uppercase();
            p["display_name"] = json!(d);
            json!({ "action": "modify", "payload": p })
        }
        "invoice.before_create" => {
            for it in p["items"].as_array_mut().into_iter().flatten() {
                let d = format!("[x] {}", it["description"].as_str().unwrap_or(""));
                it["description"] = json!(d);
                it["unit_price"] = json!("0.01"); // ignored by the host
            }
            json!({ "action": "modify", "payload": p })
        }
        "order.before_place" => match p["coupon"].as_str() {
            Some("BOOM") => return Err("boom".into()),
            Some("SLOW") => loop {
                bump("spin");
            },
            _ => json!({ "action": "continue" }),
        },
        _ => json!({ "action": "continue" }),
    })
}

fn http(input: Value) -> Result<Value, String> {
    let m = input["method"].as_str().unwrap_or("");
    let path = input["path"].as_str().unwrap_or("");
    Ok(match (m, path) {
        ("GET", "/ping") => {
            json!({ "status": 200, "body": { "pong": true, "q": input["query"]["q"] } })
        }
        ("GET", p) if p.starts_with("/items/") => {
            json!({ "status": 200, "body": { "id": input["params"]["id"] } })
        }
        ("GET", "/stats") => json!({ "status": 200, "body": [
            { "label": "Logins checked", "value": counter("hook:login.before") },
            { "label": "Invoices", "value": counter("hook:invoice.before_create") }
        ] }),
        ("POST", "/notes") => {
            let text = input["body"]["text"].as_str().unwrap_or("").to_string();
            if text.is_empty() {
                return Ok(json!({ "status": 422, "body": { "error": "text required" } }));
            }
            let user = input["user"]["id"].as_str().unwrap_or("").to_string();
            let key = format!("notes:{user}");
            let mut notes: Vec<Value> =
                serde_json::from_str(&host::kv_get(&key).unwrap_or_else(|| "[]".into()))
                    .map_err(|e| e.to_string())?;
            notes.push(json!({ "text": text }));
            host::kv_set(
                &key,
                &serde_json::to_string(&notes).map_err(|e| e.to_string())?,
            )?;
            json!({ "status": 201, "body": { "ok": true } })
        }
        ("GET", "/notes") => {
            let user = input["user"]["id"].as_str().unwrap_or("");
            let notes: Value = serde_json::from_str(
                &host::kv_get(&format!("notes:{user}")).unwrap_or_else(|| "[]".into()),
            )
            .map_err(|e| e.to_string())?;
            json!({ "status": 200, "body": notes })
        }
        _ => json!({ "status": 404, "body": null }),
    })
}

struct PeltyAddon;
impl pelty_addon_sdk::Guest for PeltyAddon {
    fn describe() -> String {
        json!({ "id": "extdemo", "name": "Extension demo" }).to_string()
    }
    fn call(op: String, input: String) -> Result<String, String> {
        let v: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        let out = match op.as_str() {
            "hook" => hook(v)?,
            "http" => http(v)?,
            _ => return Err(format!("unknown op {op}")),
        };
        Ok(out.to_string())
    }
    fn on_event(_name: String, _payload: String) -> Result<(), String> {
        Ok(())
    }
}
pelty_addon_sdk::export!(PeltyAddon with_types_in pelty_addon_sdk);
