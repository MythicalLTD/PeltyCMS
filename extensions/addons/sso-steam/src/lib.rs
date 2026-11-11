//! Steam sign-in with the official Steam OpenID 2.0 provider
//! (https://steamcommunity.com/openid), plus the player's persona name from
//! the Steam Web API (`GetPlayerSummaries`) when an API key is set.
//!
//! The host owns the account-security part: one-time state, browser
//! binding, and checking that `openid.return_to` is exactly the callback it
//! issued. This addon checks the assertion's shape and asks Steam to verify
//! the signature (`check_authentication`); nothing is trusted otherwise.
use serde_json::{json, Map, Value};

pub const OP: &str = "https://steamcommunity.com/openid/login";
pub const CLAIMED_PREFIX: &str = "https://steamcommunity.com/openid/id/";
pub const SUMMARIES: &str = "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/";
const NS: &str = "http://specs.openid.net/auth/2.0";

/// `(method, url, headers, body) -> (status, body)`.
pub type Http<'a> =
    &'a dyn Fn(&str, &str, Vec<(String, String)>, String) -> Result<(u16, String), String>;

pub fn describe() -> Value {
    json!({
        "engine": "steam",
        "name": "Steam",
        "protocol": "openid2",
        "authorize_url": OP,
        "userinfo_url": SUMMARIES,
        "scope": "",
    })
}

/// SteamID64 of a claimed id `https://steamcommunity.com/openid/id/<17 digits>`.
pub fn steam_id(claimed: &str) -> Option<String> {
    let id = claimed.strip_prefix(CLAIMED_PREFIX)?;
    (id.len() == 17 && id.starts_with("7656119") && id.bytes().all(|b| b.is_ascii_digit()))
        .then(|| id.to_string())
}

/// Shape checks of a positive assertion before asking Steam about it.
pub fn check_assertion(p: &Map<String, Value>) -> Result<String, String> {
    let g = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    let fail = |w: &str| Err(format!("failed:{w}"));
    if g("openid.ns") != NS {
        return fail("namespace");
    }
    if g("openid.mode") != "id_res" {
        return fail("mode");
    }
    if g("openid.op_endpoint") != OP {
        return fail("endpoint");
    }
    let claimed = g("openid.claimed_id");
    if claimed != g("openid.identity") {
        return fail("identity");
    }
    let Some(id) = steam_id(claimed) else {
        return fail("claimed id");
    };
    let signed: Vec<&str> = g("openid.signed").split(',').collect();
    for need in [
        "op_endpoint",
        "claimed_id",
        "identity",
        "return_to",
        "response_nonce",
    ] {
        if !signed.contains(&need) {
            return fail("unsigned field");
        }
    }
    if g("openid.sig").is_empty() || g("openid.assoc_handle").is_empty() {
        return fail("signature");
    }
    Ok(id)
}

/// `application/x-www-form-urlencoded`.
pub fn form(pairs: &[(String, String)]) -> String {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => {
                    out.push(b as char)
                }
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

/// `is_valid:true` in Steam's key-value answer.
pub fn is_valid(body: &str) -> bool {
    body.lines()
        .filter_map(|l| l.split_once(':'))
        .any(|(k, v)| k.trim() == "is_valid" && v.trim() == "true")
}

/// Op `verify`: `input` = `{params: {"openid.*": …}, settings: {api_key}}`.
/// Returns `{subject, email, email_verified, name, username}`.
pub fn verify(input: &Value, http: Http) -> Result<Value, String> {
    let fail = |w: &str| format!("failed:{w}");
    let params = input["params"]
        .as_object()
        .ok_or_else(|| fail("no params"))?;
    let id = check_assertion(params)?;
    // Echo every openid.* field back with mode=check_authentication.
    let mut pairs: Vec<(String, String)> = params
        .iter()
        .filter(|(k, _)| k.starts_with("openid.") && k.as_str() != "openid.mode")
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect();
    pairs.sort();
    pairs.push(("openid.mode".into(), "check_authentication".into()));
    let (st, text) = http(
        "POST",
        OP,
        vec![(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        form(&pairs),
    )
    .map_err(|e| fail(&e))?;
    if !(200..300).contains(&st) || !is_valid(&text) {
        return Err(fail("not valid"));
    }
    let key = input["settings"]["api_key"].as_str().unwrap_or("").trim();
    let name = if key.is_empty() {
        String::new()
    } else {
        persona(key, &id, http).unwrap_or_default()
    };
    Ok(json!({
        "subject": id,
        "email": "",
        "email_verified": false,
        "name": name,
        "username": name,
    }))
}

/// Persona name from `GetPlayerSummaries`; `None` on any error.
fn persona(key: &str, id: &str, http: Http) -> Option<String> {
    let url = format!(
        "{SUMMARIES}?{}",
        form(&[("key".into(), key.into()), ("steamids".into(), id.into())])
    );
    let (st, text) = http(
        "GET",
        &url,
        vec![("accept".into(), "application/json".into())],
        String::new(),
    )
    .ok()?;
    if !(200..300).contains(&st) {
        return None;
    }
    let v: Value = serde_json::from_str(&text).ok()?;
    v["response"]["players"]
        .as_array()?
        .iter()
        .find(|p| p["steamid"].as_str() == Some(id))?["personaname"]
        .as_str()
        .map(|s| s.chars().take(100).collect())
}

pub fn call(op: &str, input: &str, http: Http) -> Result<String, String> {
    let v: Value = serde_json::from_str(input).map_err(|e| format!("malformed_payload:{e}"))?;
    match op {
        "verify" => Ok(verify(&v, http)?.to_string()),
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

    const ID: &str = "76561197960287930";

    fn assertion() -> Value {
        json!({
            "openid.ns": NS,
            "openid.mode": "id_res",
            "openid.op_endpoint": OP,
            "openid.claimed_id": format!("{CLAIMED_PREFIX}{ID}"),
            "openid.identity": format!("{CLAIMED_PREFIX}{ID}"),
            "openid.return_to": "https://shop.test/api/v1/auth/oauth/sso-steam/callback?state=s",
            "openid.response_nonce": "2026-11-11T10:00:00Zabc",
            "openid.assoc_handle": "1234567890",
            "openid.signed": "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle",
            "openid.sig": "c2ln",
        })
    }

    #[test]
    fn steam_ids() {
        assert_eq!(
            steam_id(&format!("{CLAIMED_PREFIX}{ID}")).as_deref(),
            Some(ID)
        );
        assert!(steam_id("https://steamcommunity.com/openid/id/123").is_none());
        assert!(steam_id(&format!("https://evil.test/openid/id/{ID}")).is_none());
        assert!(steam_id(&format!("{CLAIMED_PREFIX}{ID}/x")).is_none());
    }

    #[test]
    fn assertion_shape() {
        let ok = assertion();
        assert_eq!(check_assertion(ok.as_object().unwrap()).unwrap(), ID);
        for (k, v) in [
            ("openid.mode", "cancel"),
            ("openid.op_endpoint", "https://evil.test/openid/login"),
            (
                "openid.identity",
                "https://steamcommunity.com/openid/id/76561197960287931",
            ),
            ("openid.signed", "signed,op_endpoint,claimed_id,identity"),
            ("openid.sig", ""),
        ] {
            let mut bad = ok.clone();
            bad[k] = json!(v);
            assert!(check_assertion(bad.as_object().unwrap()).is_err(), "{k}");
        }
    }

    #[test]
    fn verifies_with_steam_and_reads_persona() {
        let seen = RefCell::new(vec![]);
        let http = |m: &str, u: &str, _h: Vec<(String, String)>, b: String| {
            seen.borrow_mut().push((m.to_string(), u.to_string(), b));
            Ok(if u == OP {
                (
                    200,
                    "ns:http://specs.openid.net/auth/2.0\nis_valid:true\n".to_string(),
                )
            } else {
                (
                    200,
                    json!({"response": {"players": [{"steamid": ID, "personaname": "Gabe"}]}})
                        .to_string(),
                )
            })
        };
        let p = verify(
            &json!({"params": assertion(), "settings": {"api_key": "K"}}),
            &http,
        )
        .unwrap();
        assert_eq!(
            (p["subject"].as_str(), p["name"].as_str()),
            (Some(ID), Some("Gabe"))
        );
        let s = seen.borrow();
        assert!(s[0].2.contains("openid.mode=check_authentication"));
        assert!(!s[0].2.contains("openid.mode=id_res"));
        assert!(s[1].1.starts_with(SUMMARIES) && s[1].1.contains("steamids=76561197960287930"));
    }

    #[test]
    fn rejects_unverified() {
        let no = |_m: &str, _u: &str, _h: Vec<(String, String)>, _b: String| {
            Ok((
                200,
                "ns:http://specs.openid.net/auth/2.0\nis_valid:false\n".to_string(),
            ))
        };
        assert!(verify(&json!({"params": assertion(), "settings": {}}), &no).is_err());
        // No API key: no Web API call, empty name.
        let yes = |_m: &str, u: &str, _h: Vec<(String, String)>, _b: String| {
            assert_eq!(u, OP);
            Ok((200, "is_valid:true".to_string()))
        };
        let p = verify(&json!({"params": assertion(), "settings": {}}), &yes).unwrap();
        assert_eq!(p["name"], "");
    }
}
