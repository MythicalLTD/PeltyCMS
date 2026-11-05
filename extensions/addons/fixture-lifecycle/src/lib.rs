//! Test fixture (not bundled): the full ABI 0.3 event lifecycle, a custom
//! event and mixins.
use pelty_addon_sdk::{host, reply, serde_json, serde_json::json, serde_json::Value};

fn counter(k: &str) -> u64 {
    host::kv_get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn bump(k: &str) {
    let _ = host::kv_set(k, &(counter(k) + 1).to_string());
}

fn hook(input: Value) -> Result<Value, String> {
    let event = input["event"].as_str().unwrap_or("");
    let phase = input["phase"].as_str().unwrap_or("before");
    bump(&format!("phase:{phase}"));
    if phase == "finished" {
        let st = input["payload"]["status"].as_str().unwrap_or("?");
        bump(&format!("finished:{st}"));
    }
    if phase != "before" {
        return Ok(reply::proceed());
    }
    let p = &input["payload"];
    Ok(match event {
        // Request lifecycle: cancel a profile update to the city "Blocked".
        e if e.starts_with("before:PUT ") => {
            if p["body"]["city"] == "Blocked" {
                reply::cancel("city blocked")
            } else {
                reply::proceed()
            }
        }
        // Own custom event, `highest` + `ignore_cancelled`: rescue "rescue".
        "integration-lifecycle:greet" if input["cancelled"] == true && p["name"] == "rescue" => {
            reply::uncancel()
        }
        _ => reply::proceed(),
    })
}

fn mixin(input: Value) -> Result<Value, String> {
    let at = input["at"].as_str().unwrap_or("");
    let phase = input["phase"].as_str().unwrap_or("");
    bump(&format!("mixin:{at}:{phase}"));
    let mut resp = input["response"].clone();
    Ok(match (at, phase, input["target"].as_str().unwrap_or("")) {
        ("around", "before", _) => {
            let mut body = input["request"]["body"].clone();
            if let Some(o) = body["organization"].as_str() {
                body["organization"] = json!(o.to_uppercase());
            }
            json!({ "body": body, "state": { "seen": "around" } })
        }
        ("around", "after", _) => {
            let _ = host::kv_set("around_state", &input["state"].to_string());
            json!({})
        }
        ("after", "after", t)
            if t.starts_with("GET ") && input["request"]["query"] == "break=1" =>
        {
            resp["evil"] = json!(true); // new key: the host must ignore this
            json!({ "response": resp })
        }
        ("after", "after", t) if t.starts_with("GET ") => {
            if let Some(c) = resp["city"].as_str() {
                resp["city"] = json!(format!("{c} (mixin)"));
            }
            json!({ "response": resp })
        }
        ("return", "after", _) => {
            resp["last_name"] = json!("Returned");
            json!({ "response": resp })
        }
        _ => json!({}),
    })
}

fn http(input: Value) -> Result<Value, String> {
    let m = input["method"].as_str().unwrap_or("");
    let path = input["path"].as_str().unwrap_or("");
    Ok(match (m, path) {
        ("POST", "/greet") => {
            let name = input["body"]["name"].as_str().unwrap_or("").to_string();
            let e = pelty_addon_sdk::emit("integration-lifecycle:greet", &json!({ "name": name }))?;
            json!({ "status": 200, "body": { "cancelled": e.cancelled, "reason": e.reason, "payload": e.payload } })
        }
        ("POST", "/greet-other") => {
            match pelty_addon_sdk::emit("integration-listener:spoof", &json!({})) {
                Ok(_) => json!({ "status": 200, "body": { "emitted": true } }),
                Err(e) => json!({ "status": 403, "body": { "error": e } }),
            }
        }
        ("GET", "/log") => {
            let keys = [
                "phase:before",
                "phase:during",
                "phase:after",
                "phase:finished",
                "finished:ok",
                "finished:cancelled",
                "finished:failed",
                "mixin:around:before",
                "mixin:around:after",
                "mixin:after:after",
                "mixin:return:after",
            ];
            let rows: Vec<Value> = keys
                .iter()
                .map(|k| json!({ "key": k, "value": counter(k) }))
                .collect();
            json!({ "status": 200, "body": { "rows": rows, "around_state": host::kv_get("around_state") } })
        }
        _ => json!({ "status": 404, "body": null }),
    })
}

struct PeltyAddon;
impl pelty_addon_sdk::Guest for PeltyAddon {
    fn describe() -> String {
        json!({ "id": "lifecycle", "name": "Lifecycle demo" }).to_string()
    }
    fn call(op: String, input: String) -> Result<String, String> {
        let v: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        let out = match op.as_str() {
            "hook" => hook(v)?,
            "mixin" => mixin(v)?,
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
