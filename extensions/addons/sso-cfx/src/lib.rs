//! FiveM / RedM sign-in through the Cfx.re forum (https://forum.cfx.re),
//! using Discourse User API keys, the path Cfx.re staff point third-party
//! sites to. This is a Cfx.re forum account, not Rockstar SSO, and not the
//! private Cfx.re IDMS used by Tebex and txAdmin.
//!
//! The host does the account-security part (one-time state, browser
//! binding, the RSA key pair, decrypting the payload, checking its nonce).
//! This addon only talks to the forum: op `profile` reads
//! `/session/current.json` with the decrypted key and then revokes the key,
//! so nothing long-lived is kept.
use serde_json::{json, Value};

pub const FORUM: &str = "https://forum.cfx.re";

/// `(method, url, headers, body) -> (status, body)`.
pub type Http<'a> =
    &'a dyn Fn(&str, &str, Vec<(String, String)>, String) -> Result<(u16, String), String>;

pub fn describe() -> Value {
    json!({
        "engine": "cfx",
        "name": "FiveM",
        "protocol": "discourse_user_api",
        "authorize_url": format!("{FORUM}/user-api-key/new"),
        "userinfo_url": format!("{FORUM}/session/current.json"),
        "revoke_url": format!("{FORUM}/user-api-key/revoke"),
        "scope": "session_info",
    })
}

/// `session/current.json` → `{subject, email, email_verified, name, username}`.
/// The forum does not share an e-mail with `session_info`; when one is
/// present it is passed on but never treated as verified. There is no
/// Rockstar licence in this document.
pub fn profile_of(v: &Value) -> Option<Value> {
    let u = &v["current_user"];
    let id = u["id"]
        .as_i64()
        .filter(|i| *i > 0)
        .map(|i| i.to_string())
        .or_else(|| {
            u["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .map(str::to_string)
        })?;
    let username = u["username"].as_str().filter(|s| !s.is_empty())?;
    let name = u["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(username);
    Some(json!({
        "subject": id,
        "email": u["email"].as_str().unwrap_or(""),
        "email_verified": false,
        "name": name,
        "username": username,
    }))
}

fn key_headers(key: &str, client_id: &str) -> Vec<(String, String)> {
    let mut h = vec![
        ("user-api-key".to_string(), key.to_string()),
        ("accept".to_string(), "application/json".to_string()),
    ];
    if !client_id.is_empty() {
        h.push(("user-api-client-id".to_string(), client_id.to_string()));
    }
    h
}

/// Op `profile`: `input` = `{key, client_id}`. Errors are `failed:<why>`.
pub fn profile(input: &Value, http: Http) -> Result<Value, String> {
    let fail = |w: &str| format!("failed:{w}");
    let key = input["key"].as_str().unwrap_or("");
    let client_id = input["client_id"].as_str().unwrap_or("");
    if key.is_empty() {
        return Err(fail("no key"));
    }
    let (st, text) = http(
        "GET",
        &format!("{FORUM}/session/current.json"),
        key_headers(key, client_id),
        String::new(),
    )
    .map_err(|e| fail(&e))?;
    // Revoke whatever happened: the key was only needed for this one read.
    let _ = http(
        "POST",
        &format!("{FORUM}/user-api-key/revoke"),
        key_headers(key, client_id),
        String::new(),
    );
    if !(200..300).contains(&st) {
        return Err(fail("session status"));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| fail("session json"))?;
    profile_of(&v).ok_or_else(|| fail("no user"))
}

pub fn call(op: &str, input: &str, http: Http) -> Result<String, String> {
    let v: Value = serde_json::from_str(input).map_err(|e| format!("malformed_payload:{e}"))?;
    match op {
        "profile" => Ok(profile(&v, http)?.to_string()),
        _ => Err(format!("unsupported_op:{op}")),
    }
}

#[cfg(all(target_family = "wasm", feature = "export"))]
mod export {
    struct PeltyAddon;
    impl pelty_addon_sdk::Guest for PeltyAddon {
        fn describe() -> String {
            super::describe().to_string()
        }
        fn call(op: String, input: String) -> Result<String, String> {
            super::call(&op, &input, &pelty_addon_sdk::fetch)
        }
        fn on_event(_name: String, _payload: String) -> Result<(), String> {
            Ok(())
        }
    }
    pelty_addon_sdk::export!(PeltyAddon with_types_in pelty_addon_sdk);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn maps_session() {
        let p = profile_of(&json!({"current_user": {"id": 42, "username": "pelty", "name": ""}}))
            .unwrap();
        assert_eq!(
            p,
            json!({"subject": "42", "email": "", "email_verified": false, "name": "pelty", "username": "pelty"})
        );
        assert!(profile_of(&json!({})).is_none());
        assert!(profile_of(&json!({"current_user": {"id": 0, "username": "x"}})).is_none());
        assert!(profile_of(&json!({"current_user": {"id": 5}})).is_none());
        // An e-mail, if the forum ever sends one, is never "verified".
        let p = profile_of(&json!({"current_user": {"id": "7", "username": "a", "email": "a@x"}}))
            .unwrap();
        assert_eq!(p["email_verified"], false);
    }

    #[test]
    fn reads_session_then_revokes() {
        let seen = RefCell::new(vec![]);
        let http = |m: &str, u: &str, h: Vec<(String, String)>, _b: String| {
            seen.borrow_mut().push((m.to_string(), u.to_string(), h));
            Ok(if u.ends_with("current.json") {
                (
                    200,
                    r#"{"current_user":{"id":9,"username":"z"}}"#.to_string(),
                )
            } else {
                (200, "{}".to_string())
            })
        };
        let p = profile(&json!({"key": "k1", "client_id": "c1"}), &http).unwrap();
        assert_eq!(p["subject"], "9");
        let s = seen.borrow();
        assert_eq!(s[0].1, "https://forum.cfx.re/session/current.json");
        assert!(s[0].2.contains(&("user-api-key".into(), "k1".into())));
        assert_eq!(
            (s[1].0.as_str(), s[1].1.as_str()),
            ("POST", "https://forum.cfx.re/user-api-key/revoke")
        );
    }

    #[test]
    fn rejects_bad_answers() {
        let denied = |_m: &str, _u: &str, _h: Vec<(String, String)>, _b: String| {
            Ok((403, r#"{"errors":["x"]}"#.to_string()))
        };
        assert!(profile(&json!({"key": "k"}), &denied)
            .unwrap_err()
            .starts_with("failed:"));
        assert!(profile(&json!({}), &denied).is_err());
        assert!(call("nope", "{}", &denied)
            .unwrap_err()
            .starts_with("unsupported_op"));
    }
}
