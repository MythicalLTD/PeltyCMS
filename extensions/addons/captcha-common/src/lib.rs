//! Shared siteverify plumbing for the captcha addons (Turnstile, hCaptcha,
//! reCAPTCHA v2/v3). Each addon crate only declares its [`Engine`]; the
//! request/response handling below is identical for all of them. Pure code
//! over an injected HTTP function, so it is unit-tested natively.
use serde_json::{json, Value};

pub use pelty_addon_sdk as sdk;

/// `(method, url, headers, body) -> (status, body)`.
pub type Http<'a> = &'a dyn Fn(&str, &str, Vec<(String, String)>, String) -> Result<(u16, String), String>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Engine {
    pub id: &'static str,
    pub name: &'static str,
    pub verify_url: &'static str,
    pub has_score: bool,
    /// Browser widget script; `{site_key}` is replaced by the site key.
    pub script: &'static str,
    /// Global the script defines (`render` / `reset`, or `execute` for v3).
    pub global: &'static str,
    /// `widget` (visible, render into a box) or `score` (invisible, execute).
    pub mode: &'static str,
    /// The provider returns a risk score (higher = riskier, e.g. hCaptcha)
    /// instead of a human score; it is inverted before `min_score`.
    pub risk_score: bool,
}

pub fn describe(e: &Engine) -> Value {
    json!({
        "engine": e.id, "name": e.name, "verify_url": e.verify_url, "has_score": e.has_score,
        "widget": { "script": e.script, "global": e.global, "mode": e.mode },
    })
}

/// `cfg`: addon settings (`secret_key`, optional `verify_url`, `min_score`).
/// Ok(false) = negative answer; Err(`misconfigured`|`unavailable:<why>`).
pub fn verify(e: &Engine, cfg: &Value, token: &str, ip: Option<&str>, http: Http) -> Result<bool, String> {
    let secret = cfg["secret_key"].as_str().unwrap_or("");
    if secret.is_empty() {
        return Err("misconfigured".into());
    }
    let url = match cfg["verify_url"].as_str() {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => e.verify_url.to_string(),
    };
    let mut pairs = vec![("secret", secret), ("response", token)];
    if let Some(ip) = ip {
        pairs.push(("remoteip", ip));
    }
    let (status, text) = http("POST", &url, vec![("content-type".into(), "application/x-www-form-urlencoded".into())], form(&pairs)).map_err(|e| format!("unavailable:{e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("unavailable:status {status}"));
    }
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if v["success"].as_bool() != Some(true) {
        return Ok(false);
    }
    if e.has_score {
        let min = cfg["min_score"].as_f64().or_else(|| cfg["min_score"].as_str().and_then(|x| x.parse().ok())).unwrap_or(0.5);
        // A missing score (e.g. hCaptcha's free tier) means "passed".
        if let Some(score) = v["score"].as_f64() {
            let score = if e.risk_score { 1.0 - score } else { score };
            if score < min {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Op `verify`: `{token, ip, settings}` → `{ok}`.
pub fn call(e: &Engine, op: &str, input: &str, http: Http) -> Result<String, String> {
    let v: Value = serde_json::from_str(input).map_err(|e| format!("malformed_payload:{e}"))?;
    match op {
        "verify" => {
            let ok = verify(e, &v["settings"], v["token"].as_str().unwrap_or(""), v["ip"].as_str(), http)?;
            Ok(json!({ "ok": ok }).to_string())
        }
        _ => Err(format!("unsupported_op:{op}")),
    }
}

/// Host `http-fetch` as an [`Http`].
pub fn host_http(m: &str, u: &str, h: Vec<(String, String)>, b: String) -> Result<(u16, String), String> {
    pelty_addon_sdk::fetch(m, u, h, b)
}

/// `application/x-www-form-urlencoded` encoding of `pairs`.
pub fn form(pairs: &[(&str, &str)]) -> String {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => out.push(b as char),
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        enc(k, &mut out);
        out.push('=');
        enc(v, &mut out);
    }
    out
}

/// Export a captcha addon for an [`Engine`] constant.
#[macro_export]
macro_rules! captcha_addon {
    ($engine:expr) => {
        struct PeltyAddon;
        impl $crate::sdk::Guest for PeltyAddon {
            fn describe() -> String {
                $crate::describe(&$engine).to_string()
            }
            fn call(op: String, input: String) -> Result<String, String> {
                $crate::call(&$engine, &op, &input, &$crate::host_http)
            }
            fn on_event(_name: String, _payload: String) -> Result<(), String> {
                Ok(())
            }
        }
        $crate::sdk::export!(PeltyAddon with_types_in $crate::sdk);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const SCORE: Engine = Engine { id: "s", name: "S", verify_url: "https://v/siteverify", has_score: true, script: "", global: "", mode: "score", risk_score: false };
    const RISK: Engine = Engine { risk_score: true, ..SCORE };
    const PLAIN: Engine = Engine { has_score: false, ..SCORE };

    #[test]
    fn verifies_and_applies_scores() {
        let seen = RefCell::new(String::new());
        let reply = RefCell::new(r#"{"success":true,"score":0.9}"#.to_string());
        let http = |_m: &str, u: &str, _h: Vec<(String, String)>, b: String| {
            *seen.borrow_mut() = format!("{u} {b}");
            Ok((200, reply.borrow().clone()))
        };
        let cfg = json!({"secret_key": "s3", "min_score": "0.5"});
        assert_eq!(verify(&SCORE, &cfg, "tok", Some("1.2.3.4"), &http), Ok(true));
        assert_eq!(*seen.borrow(), "https://v/siteverify secret=s3&response=tok&remoteip=1.2.3.4");
        // risk score: 0.9 risk → 0.1 < 0.5
        assert_eq!(verify(&RISK, &cfg, "tok", None, &http), Ok(false));
        *reply.borrow_mut() = r#"{"success":false}"#.into();
        assert_eq!(verify(&PLAIN, &cfg, "tok", None, &http), Ok(false));
        assert_eq!(verify(&PLAIN, &json!({}), "tok", None, &http), Err("misconfigured".into()));
        let down = |_m: &str, _u: &str, _h: Vec<(String, String)>, _b: String| Ok((503, String::new()));
        assert!(verify(&PLAIN, &cfg, "tok", None, &down).unwrap_err().starts_with("unavailable:"));
        *reply.borrow_mut() = r#"{"success":true}"#.into();
        assert_eq!(call(&PLAIN, "verify", r#"{"token":"t","settings":{"secret_key":"k"}}"#, &http), Ok(r#"{"ok":true}"#.into()));
        assert!(call(&PLAIN, "nope", "{}", &http).unwrap_err().starts_with("unsupported_op"));
    }

    #[test]
    fn form_encodes() {
        assert_eq!(super::form(&[("a b", "x&y=z"), ("k", "ü")]), "a+b=x%26y%3Dz&k=%C3%BC");
    }
}
