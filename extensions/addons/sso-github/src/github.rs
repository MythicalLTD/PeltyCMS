//! GitHub sign-in provider.
use pelty_addon_sso_common::{self as sso, Provider};
#[allow(unused_imports)]
use serde_json::{json, Value};

pub const PROVIDER: Provider = Provider {
    id: "github",
    name: "GitHub",
    authorize: "https://github.com/login/oauth/authorize",
    token: "https://github.com/login/oauth/access_token",
    userinfo: "https://api.github.com/user",
    scope: "read:user user:email",
    profile,
    enrich: Some(verified_email),
};

/// GitHub's /user email is the public one; verification comes from /user/emails.
fn profile(v: &Value) -> Option<Value> {
    sso::profile(
        sso::id(v)?,
        sso::s(v, "email").unwrap_or_default(),
        false,
        sso::name(v),
    )
}

/// Replace the public email with the primary verified one from `/user/emails`.
fn verified_email(prof: &mut Value, userinfo: &str, auth: Vec<(String, String)>, http: sso::Http) {
    let base = userinfo.trim_end_matches("/user");
    if let Ok((200..=299, text)) = http("GET", &format!("{base}/user/emails"), auth, String::new())
    {
        if let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&text) {
            if let Some(e) = list.iter().find(|e| {
                e["primary"].as_bool() == Some(true) && e["verified"].as_bool() == Some(true)
            }) {
                let email = e["email"].as_str().unwrap_or("").to_string();
                prof["email_verified"] = json!(!email.is_empty());
                prof["email"] = json!(email);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exchanges_code_and_reads_github_emails() {
        let http = |m: &str,
                    u: &str,
                    _h: Vec<(String, String)>,
                    b: String|
         -> Result<(u16, String), String> {
            match (m, u) {
                ("POST", "https://t/token") => {
                    assert!(
                        b.contains("code=c1")
                            && b.contains("code_verifier=v1")
                            && b.contains("client_secret=sec")
                    );
                    Ok((200, r#"{"access_token":"at"}"#.into()))
                }
                ("GET", "https://api/user") => Ok((200, r#"{"id":9,"login":"gh"}"#.into())),
                ("GET", "https://api/user/emails") => Ok((
                    200,
                    r#"[{"email":"v@x","primary":true,"verified":true}]"#.into(),
                )),
                _ => Err(format!("unexpected {m} {u}")),
            }
        };
        let input = json!({"code": "c1", "code_verifier": "v1", "redirect_uri": "https://r", "token_url": "https://t/token", "userinfo_url": "https://api/user"});
        let p = sso::exchange(
            &PROVIDER,
            &json!({"client_id": "id", "client_secret": "sec"}),
            &input,
            &http,
        )
        .unwrap();
        assert_eq!(
            p,
            json!({"subject": "9", "email": "v@x", "email_verified": true, "name": "gh"})
        );
    }

    #[test]
    fn maps_claims() {
        let gh = (PROVIDER.profile)(&json!({"id": 42, "email": "b@x", "login": "bee"})).unwrap();
        assert_eq!(
            (
                gh["subject"].as_str(),
                gh["email_verified"].as_bool(),
                gh["name"].as_str()
            ),
            (Some("42"), Some(false), Some("bee"))
        );
    }
}
