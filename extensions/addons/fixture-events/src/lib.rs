//! Test fixture (not bundled): an integration addon that records every
//! subscribed event in its host kv store and reads it back via `call("log")`.
use pelty_addon_sdk::{host, serde_json};

struct PeltyAddon;
impl pelty_addon_sdk::Guest for PeltyAddon {
    fn describe() -> String {
        serde_json::json!({ "id": "eventlog", "name": "Event log" }).to_string()
    }
    fn call(op: String, _input: String) -> Result<String, String> {
        match op.as_str() {
            "log" => Ok(host::kv_get("log").unwrap_or_else(|| "[]".into())),
            "secret" => {
                Ok(serde_json::json!({ "token": pelty_addon_sdk::setting("token") }).to_string())
            }
            // Runs after integration-extdemo (priority 200 > 50).
            "hook" => {
                let mut v: serde_json::Value =
                    serde_json::from_str(&_input).map_err(|e| e.to_string())?;
                let mut p = v["payload"].take();
                for it in p["items"].as_array_mut().into_iter().flatten() {
                    let d = format!("{} (2)", it["description"].as_str().unwrap_or(""));
                    it["description"] = serde_json::json!(d);
                }
                Ok(serde_json::json!({ "action": "modify", "payload": p }).to_string())
            }
            _ => Err(format!("unknown op {op}")),
        }
    }
    fn on_event(name: String, _payload: String) -> Result<(), String> {
        let mut log: Vec<String> =
            serde_json::from_str(&host::kv_get("log").unwrap_or_else(|| "[]".into()))
                .map_err(|e| e.to_string())?;
        log.push(name);
        host::kv_set(
            "log",
            &serde_json::to_string(&log).map_err(|e| e.to_string())?,
        )
    }
}
pelty_addon_sdk::export!(PeltyAddon with_types_in pelty_addon_sdk);
