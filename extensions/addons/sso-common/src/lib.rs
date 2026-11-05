//! Shared OAuth 2.0 / OpenID Connect plumbing for the SSO addons: the code
//! exchange and the `exchange` op. Each addon crate declares its
//! [`Provider`] (endpoints + its userinfo → profile mapping). State, PKCE,
//! sessions and account linking stay in the host (they are account
//! security, not provider specifics).
use serde_json::{json, Value};

pub use pelty_addon_sdk as sdk;

/// `(method, url, headers, body) -> (status, body)`.
pub type Http<'a> =
    &'a dyn Fn(&str, &str, Vec<(String, String)>, String) -> Result<(u16, String), String>;

/// Extra provider step after userinfo (e.g. GitHub's verified e-mail):
/// `(profile, userinfo_url, auth_headers, http)`.
pub type Enrich = fn(&mut Value, &str, Vec<(String, String)>, Http);

#[derive(Clone, Copy)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub authorize: &'static str,
    pub token: &'static str,
    pub userinfo: &'static str,
    pub scope: &'static str,
    /// userinfo document → `{subject, email, email_verified, name}`.
    pub profile: fn(&Value) -> Option<Value>,
    pub enrich: Option<Enrich>,
}

pub fn describe(p: &Provider) -> Value {
    json!({ "engine": p.id, "name": p.name, "authorize_url": p.authorize, "token_url": p.token, "userinfo_url": p.userinfo, "scope": p.scope })
}

/// Build a profile; `None` when the subject is empty.
pub fn profile(
    subject: String,
    email: String,
    email_verified: bool,
    name: String,
) -> Option<Value> {
    if subject.is_empty() {
        return None;
    }
    Some(
        json!({ "subject": subject, "email": email, "email_verified": email_verified, "name": name }),
    )
}

/// String field `k` of `v`.
pub fn s(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(str::to_string)
}

/// Numeric-or-string `id` (GitHub, Discord, Facebook).
pub fn id(v: &Value) -> Option<String> {
    v["id"]
        .as_i64()
        .map(|i| i.to_string())
        .or_else(|| s(v, "id"))
}

/// First present display name among the usual keys.
pub fn name(v: &Value) -> String {
    s(v, "name")
        .or_else(|| s(v, "global_name"))
        .or_else(|| s(v, "login"))
        .or_else(|| s(v, "username"))
        .unwrap_or_default()
}

/// Standard OIDC claims (`sub`, `email`, `email_verified`).
pub fn oidc_profile(v: &Value) -> Option<Value> {
    let verified = v["email_verified"]
        .as_bool()
        .or_else(|| v["email_verified"].as_str().map(|x| x == "true"))
        .unwrap_or(false);
    profile(
        s(v, "sub")?,
        s(v, "email").unwrap_or_default(),
        verified,
        name(v),
    )
}

/// Op `exchange`: `input` = `{code, redirect_uri, code_verifier, token_url,
/// userinfo_url}`; `cfg` = addon settings (`client_id`, `client_secret`).
/// Returns the normalized profile. Errors are `failed:<why>`.
pub fn exchange(p: &Provider, cfg: &Value, input: &Value, http: Http) -> Result<Value, String> {
    let g = |k: &str| input[k].as_str().unwrap_or("").to_string();
    let fail = |w: &str| format!("failed:{w}");
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("code", &g("code")),
        ("redirect_uri", &g("redirect_uri")),
        ("client_id", cfg["client_id"].as_str().unwrap_or("")),
        ("client_secret", cfg["client_secret"].as_str().unwrap_or("")),
        ("code_verifier", &g("code_verifier")),
    ]);
    let (st, text) = http(
        "POST",
        &g("token_url"),
        vec![
            (
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            ),
            ("accept".into(), "application/json".into()),
        ],
        body,
    )
    .map_err(|e| fail(&e))?;
    let tok: Value = serde_json::from_str(&text).map_err(|_| fail("token response"))?;
    let access = tok["access_token"]
        .as_str()
        .filter(|_| (200..300).contains(&st))
        .ok_or_else(|| fail("no access token"))?
        .to_string();
    let auth: Vec<(String, String)> = vec![
        ("authorization".to_string(), format!("Bearer {access}")),
        ("accept".to_string(), "application/json".to_string()),
    ];
    let userinfo = g("userinfo_url");
    let (st, text) = http("GET", &userinfo, auth.clone(), String::new()).map_err(|e| fail(&e))?;
    if !(200..300).contains(&st) {
        return Err(fail("userinfo status"));
    }
    let info: Value = serde_json::from_str(&text).map_err(|_| fail("userinfo json"))?;
    let mut prof = (p.profile)(&info).ok_or_else(|| fail("no subject"))?;
    if let Some(f) = p.enrich {
        f(&mut prof, &userinfo, auth, http);
    }
    Ok(prof)
}

/// Op `exchange`: see [`exchange`]; settings in `input.settings`.
pub fn call(p: &Provider, op: &str, input: &str, http: Http) -> Result<String, String> {
    let v: Value = serde_json::from_str(input).map_err(|e| format!("malformed_payload:{e}"))?;
    match op {
        "exchange" => Ok(exchange(p, &v["settings"], &v, http)?.to_string()),
        _ => Err(format!("unsupported_op:{op}")),
    }
}

/// Host `http-fetch` as an [`Http`].
pub fn host_http(
    m: &str,
    u: &str,
    h: Vec<(String, String)>,
    b: String,
) -> Result<(u16, String), String> {
    pelty_addon_sdk::fetch(m, u, h, b)
}

/// `application/x-www-form-urlencoded` encoding of `pairs`.
pub fn form(pairs: &[(&str, &str)]) -> String {
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

/// Export an SSO addon for a [`Provider`] constant.
#[macro_export]
macro_rules! sso_addon {
    ($provider:expr) => {
        struct PeltyAddon;
        impl $crate::sdk::Guest for PeltyAddon {
            fn describe() -> String {
                $crate::describe(&$provider).to_string()
            }
            fn call(op: String, input: String) -> Result<String, String> {
                $crate::call(&$provider, &op, &input, &$crate::host_http)
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

    const P: Provider = Provider {
        id: "oidc",
        name: "OIDC",
        authorize: "",
        token: "",
        userinfo: "",
        scope: "openid",
        profile: oidc_profile,
        enrich: None,
    };

    #[test]
    fn oidc_claims() {
        let g =
            oidc_profile(&json!({"sub": "1", "email": "a@x", "email_verified": true, "name": "A"}))
                .unwrap();
        assert_eq!(
            g,
            json!({"subject": "1", "email": "a@x", "email_verified": true, "name": "A"})
        );
        assert_eq!(
            oidc_profile(&json!({"sub": "1", "email_verified": "true"})).unwrap()["email_verified"],
            true
        );
        assert!(oidc_profile(&json!({"email": "x"})).is_none());
    }

    #[test]
    fn exchange_errors() {
        let input = json!({"code": "c1", "token_url": "https://t/token", "userinfo_url": "https://api/user"});
        let bad = |_m: &str, _u: &str, _h: Vec<(String, String)>, _b: String| {
            Ok((400, r#"{"error":"x"}"#.to_string()))
        };
        assert!(exchange(&P, &json!({}), &input, &bad)
            .unwrap_err()
            .starts_with("failed:"));
        assert!(call(&P, "nope", "{}", &bad)
            .unwrap_err()
            .starts_with("unsupported_op"));
    }

    #[test]
    fn form_encodes() {
        assert_eq!(
            super::form(&[("a b", "x&y=z"), ("k", "ü")]),
            "a+b=x%26y%3Dz&k=%C3%BC"
        );
    }
}
