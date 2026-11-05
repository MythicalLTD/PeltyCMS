//! Microsoft sign-in provider.
use pelty_addon_sso_common::{self as sso, Provider};
#[allow(unused_imports)]
use serde_json::{json, Value};

pub const PROVIDER: Provider = Provider {
    id: "microsoft",
    name: "Microsoft",
    authorize: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    token: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    userinfo: "https://graph.microsoft.com/oidc/userinfo",
    scope: "openid email profile",
    profile: sso::oidc_profile,
    enrich: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_claims() {
        assert_eq!(
            (PROVIDER.profile)(&json!({"sub": "m", "email": "a@x"})).unwrap()["email_verified"],
            false
        );
    }
}
