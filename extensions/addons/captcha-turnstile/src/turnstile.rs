//! Cloudflare Turnstile engine.
use pelty_addon_captcha_common::Engine;

pub const ENGINE: Engine = Engine {
    id: "turnstile",
    name: "Cloudflare Turnstile",
    verify_url: "https://challenges.cloudflare.com/turnstile/v0/siteverify",
    has_score: false,
    script: "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit",
    global: "turnstile",
    mode: "widget",
    risk_score: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_widget() {
        let d = pelty_addon_captcha_common::describe(&ENGINE);
        assert_eq!(d["engine"], "turnstile");
        assert!(d["widget"]["script"]
            .as_str()
            .unwrap()
            .starts_with("https://"));
    }
}
