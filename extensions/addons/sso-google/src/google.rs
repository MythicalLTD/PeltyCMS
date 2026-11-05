//! Google sign-in provider.
use pelty_addon_sso_common::{self as sso, Provider};
#[allow(unused_imports)]
use serde_json::{json, Value};

pub const PROVIDER: Provider = Provider {
    id: "google",
    name: "Google",
    authorize: "https://accounts.google.com/o/oauth2/v2/auth",
    token: "https://oauth2.googleapis.com/token",
    userinfo: "https://openidconnect.googleapis.com/v1/userinfo",
    scope: "openid email profile",
    profile: sso::oidc_profile,
    enrich: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_claims() {
        let g = (PROVIDER.profile)(&json!({"sub": "1", "email": "a@x", "email_verified": true, "name": "A"})).unwrap();
        assert_eq!(g, json!({"subject": "1", "email": "a@x", "email_verified": true, "name": "A"}));
    }
}
