//! Discord sign-in provider.
use pelty_addon_sso_common::{self as sso, Provider};
#[allow(unused_imports)]
use serde_json::{json, Value};

pub const PROVIDER: Provider = Provider {
    id: "discord",
    name: "Discord",
    authorize: "https://discord.com/oauth2/authorize",
    token: "https://discord.com/api/oauth2/token",
    userinfo: "https://discord.com/api/users/@me",
    scope: "identify email",
    profile,
    enrich: None,
};

fn profile(v: &Value) -> Option<Value> {
    sso::profile(
        sso::id(v)?,
        sso::s(v, "email").unwrap_or_default(),
        v["verified"].as_bool().unwrap_or(false),
        sso::name(v),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_claims() {
        let d = (PROVIDER.profile)(
            &json!({"id": "7", "verified": true, "email": "c@x", "global_name": "C"}),
        )
        .unwrap();
        assert_eq!(
            d,
            json!({"subject": "7", "email": "c@x", "email_verified": true, "name": "C"})
        );
    }
}
