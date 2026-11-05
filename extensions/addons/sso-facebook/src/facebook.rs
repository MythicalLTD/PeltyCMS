//! Facebook sign-in provider.
use pelty_addon_sso_common::{self as sso, Provider};
#[allow(unused_imports)]
use serde_json::{json, Value};

pub const PROVIDER: Provider = Provider {
    id: "facebook",
    name: "Facebook",
    authorize: "https://www.facebook.com/v19.0/dialog/oauth",
    token: "https://graph.facebook.com/v19.0/oauth/access_token",
    userinfo: "https://graph.facebook.com/me?fields=id,name,email",
    scope: "email",
    profile,
    enrich: None,
};

/// Facebook only returns confirmed emails.
fn profile(v: &Value) -> Option<Value> {
    let email = sso::s(v, "email").unwrap_or_default();
    let verified = !email.is_empty();
    sso::profile(sso::id(v)?, email, verified, sso::name(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_claims() {
        // Facebook only returns confirmed emails.
        assert_eq!(
            (PROVIDER.profile)(&json!({"id": "5", "email": "f@x", "name": "F"})).unwrap()
                ["email_verified"],
            true
        );
        assert_eq!(
            (PROVIDER.profile)(&json!({"id": "5"})).unwrap()["email_verified"],
            false
        );
    }
}
