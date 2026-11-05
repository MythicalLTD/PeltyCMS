//! OpenID Connect sign-in provider.
use pelty_addon_sso_common::{self as sso, Provider};
#[allow(unused_imports)]
use serde_json::{json, Value};

pub const PROVIDER: Provider = Provider {
    id: "oidc",
    name: "OpenID Connect",
    authorize: "",
    token: "",
    userinfo: "",
    scope: "openid email profile",
    profile: sso::oidc_profile,
    enrich: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_claims() {
        assert!((PROVIDER.profile)(&json!({"email": "x"})).is_none());
        assert_eq!(
            (PROVIDER.profile)(&json!({"sub": "s"})).unwrap()["subject"],
            "s"
        );
    }
}
