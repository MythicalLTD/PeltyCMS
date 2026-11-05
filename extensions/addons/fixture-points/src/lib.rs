//! Test fixture (not bundled): subscribes to another addon's filter point
//! (`integration-hooks:greeting`) and, while filtering it, applies its own
//! point `integration-points:loop`, which it also subscribes to: the host
//! skips the applying addon, so this never recurses.
use pelty_addon_sdk::{op, reply, serde_json::json, serde_json::Value};

struct Points;
impl pelty_addon_sdk::Addon for Points {
    fn describe() -> Value {
        json!({ "id": "points", "name": "Filter points" })
    }
    fn filter(f: &op::Filter) -> Result<Value, String> {
        let mut v = f.value.clone();
        match f.name.as_str() {
            "integration-hooks:greeting" => {
                let looped = pelty_addon_sdk::apply_filter(
                    "integration-points:loop",
                    &json!({ "n": 0 }),
                    &json!({}),
                );
                let text = v["text"].as_str().unwrap_or("").to_string();
                v["text"] = json!(format!("{text} world (loop n={})", looped["n"]));
            }
            // Would recurse forever if the host called the applying addon.
            "integration-points:loop" => {
                let n = v["n"].as_i64().unwrap_or(0) + 1;
                v = pelty_addon_sdk::apply_filter(
                    "integration-points:loop",
                    &json!({ "n": n }),
                    &json!({}),
                );
            }
            _ => {}
        }
        Ok(reply::filter(v))
    }
}
pelty_addon_sdk::integration_addon!(Points);
