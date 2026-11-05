//! Test fixture (not bundled): exercises every opt-in power so the host
//! tests can check the granted and the refused path. Op `try` returns
//! `{"ok": <result>}` or `{"err": "<host error>"}`.
use pelty_addon_sdk::{power, serde_json, serde_json::json, serde_json::Value};

fn wrap<T: Into<Value>>(r: Result<T, String>) -> Value {
    match r {
        Ok(v) => json!({ "ok": v.into() }),
        Err(e) => json!({ "err": e }),
    }
}

fn parsed(r: Result<String, String>) -> Value {
    wrap(r.and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())))
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn fs_roundtrip(path: &str) -> Result<String, String> {
    std::fs::write(path, b"hello").map_err(|e| e.to_string())?;
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

fn try_power(v: &Value) -> Value {
    match v["power"].as_str().unwrap_or("") {
        "db_read" => parsed(power::db_read(&s(v, "entity"), &v["query"].to_string())),
        "db_write" => parsed(power::db_write(&s(v, "op"), &v["input"].to_string())),
        "sql" => parsed(power::sql(&s(v, "statement"), &v["params"].to_string())),
        "exec" => {
            let args: Vec<String> = v["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)).collect();
            wrap(power::exec(&s(v, "program"), &args, "").map(|o| json!({ "status": o.status, "stdout": o.stdout })))
        }
        "fs" => wrap(fs_roundtrip(&s(v, "path"))),
        "job" => wrap(power::job_enqueue(&s(v, "op"), "{}", 0).map(|_| true)),
        "fetch" => wrap(pelty_addon_sdk::fetch("GET", &s(v, "url"), vec![], String::new()).map(|(st, _)| st)),
        p => json!({ "err": format!("unknown power {p}") }),
    }
}

fn hook(v: &Value) -> Value {
    let mut p = v["payload"].clone();
    // Only requests marked with `?pd=1` are touched, so the fixture never
    // disturbs other tests sharing the database.
    if !p["query"].as_str().unwrap_or("").contains("pd=1") {
        return json!({ "action": "continue" });
    }
    if p["query"].as_str().unwrap_or("").contains("cancel=1") {
        return json!({ "action": "cancel", "reason": "fixture" });
    }
    let saw = p["body"].as_object().into_iter().flatten().any(|(k, x)| k.contains("password") && x != "[redacted]");
    if let Some(items) = p["body"]["items"].as_array_mut() {
        for i in items {
            if let Some(n) = i["unit_price"].as_str().and_then(|a| a.parse::<f64>().ok()) {
                i["unit_price"] = json!(format!("{:.2}", n * 2.0));
            }
            if let Some(d) = i["description"].as_str() {
                i["description"] = json!(format!("{d} [seen pw={saw}]"));
            }
        }
    }
    if p["query"].as_str().unwrap_or("").contains("bad=1") {
        p["body"]["items"][0]["unit_price"] = json!(0.1);
    }
    // Try to overwrite a redacted secret: the host must restore the original.
    if let Some(m) = p["body"].as_object_mut() {
        for (k, x) in m.iter_mut() {
            if k.contains("password") {
                *x = json!("Hijacked-Password-1");
            }
        }
    }
    json!({ "action": "modify", "payload": p })
}

struct PeltyAddon;
impl pelty_addon_sdk::Guest for PeltyAddon {
    fn describe() -> String {
        json!({ "id": "powerdemo", "name": "Power demo" }).to_string()
    }
    fn call(op: String, input: String) -> Result<String, String> {
        let v: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        let out = match op.as_str() {
            "try" => try_power(&v),
            "hook" => hook(&v),
            "tick" => json!({ "ticked": true }),
            // money.modify: 2.50 off every 10.00 line (the test's product;
            // suites run one at a time).
            "filter" if v["filter"] == "checkout.discounts" => {
                let mut val = v["value"].clone();
                for l in val["lines"].as_array_mut().into_iter().flatten() {
                    if l["amount"].as_str().and_then(|a| a.parse::<f64>().ok()) == Some(10.0) {
                        (l["discount"], l["label"]) = (json!("2.50"), json!("Fixture loyalty"));
                    }
                }
                json!({ "value": val })
            }
            "filter" => json!({ "value": v["value"] }),
            "auth.verify" => {
                if v["email"] == "alice@ext.test" && v["password"] == "open-sesame" {
                    json!({ "ok": true, "email": "alice@ext.test", "display_name": "Alice External", "external_id": "ext-1" })
                } else {
                    json!({ "ok": false })
                }
            }
            "http" if v["path"] == "/try" => json!({ "status": 200, "body": try_power(&v["body"]) }),
            "http" => json!({ "status": 404, "body": null }),
            _ => return Err(format!("unknown op {op}")),
        };
        Ok(out.to_string())
    }
    fn on_event(_name: String, _payload: String) -> Result<(), String> {
        Ok(())
    }
}
pelty_addon_sdk::export!(PeltyAddon with_types_in pelty_addon_sdk);
