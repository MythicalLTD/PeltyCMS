//! Test fixture (not bundled): ABI 0.5 cron jobs. `nightly` succeeds with a
//! small result, `broken` always fails. `POST /cron` registers or removes
//! runtime jobs through `power.cron-register` / `cron-unregister`.
use pelty_addon_sdk::{op, power, reply, serde_json::json, serde_json::Value};

struct Cron;
impl pelty_addon_sdk::Addon for Cron {
    fn describe() -> Value {
        json!({ "id": "cron", "name": "Cron jobs" })
    }
    fn http(r: &op::Http) -> Result<Value, String> {
        let b = &r.body;
        let name = b["name"].as_str().unwrap_or_default().to_string();
        let res = match b["action"].as_str() {
            Some("register") => power::cron_register(
                &name,
                b["schedule"].as_str().unwrap_or_default(),
                "runtime job",
            ),
            Some("unregister") => power::cron_unregister(&name),
            _ => Err("unknown action".into()),
        };
        Ok(reply::http(
            200,
            match res {
                Ok(()) => json!({ "ok": true }),
                Err(e) => json!({ "err": e }),
            },
        ))
    }
    fn cron(job: &str) -> Result<Value, String> {
        match job {
            "nightly" => Ok(json!({ "job": job, "cleaned": 3 })),
            "dyn" => Ok(json!({ "job": job, "runtime": true })),
            "broken" => Err("broken on purpose".into()),
            other => Err(format!("unknown cron job {other}")),
        }
    }
}
pelty_addon_sdk::integration_addon!(Cron);
