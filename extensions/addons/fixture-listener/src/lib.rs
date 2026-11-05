//! Test fixture (not bundled): listens to another addon's custom event.
use pelty_addon_sdk::{host, reply, serde_json, serde_json::json, serde_json::Value};

fn bump(k: &str) {
    let n: u64 = host::kv_get(k).and_then(|v| v.parse().ok()).unwrap_or(0);
    let _ = host::kv_set(k, &(n + 1).to_string());
}

fn hook(input: Value) -> Result<Value, String> {
    let phase = input["phase"].as_str().unwrap_or("before");
    bump(&format!("greet:{phase}"));
    if phase != "before" {
        return Ok(reply::proceed());
    }
    let mut p = input["payload"].clone();
    let name = p["name"].as_str().unwrap_or("").to_string();
    Ok(match name.as_str() {
        "nope" | "rescue" => reply::cancel("not welcome"),
        _ => {
            p["name"] = json!(format!("{name}!"));
            reply::modify(p)
        }
    })
}

fn http(input: Value) -> Result<Value, String> {
    Ok(
        match (
            input["method"].as_str().unwrap_or(""),
            input["path"].as_str().unwrap_or(""),
        ) {
            ("GET", "/log") => {
                let get = |k: &str| {
                    host::kv_get(k)
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0)
                };
                json!({ "status": 200, "body": { "before": get("greet:before"), "after": get("greet:after"), "finished": get("greet:finished") } })
            }
            _ => json!({ "status": 404, "body": null }),
        },
    )
}

struct PeltyAddon;
impl pelty_addon_sdk::Guest for PeltyAddon {
    fn describe() -> String {
        json!({ "id": "listener", "name": "Event listener" }).to_string()
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
