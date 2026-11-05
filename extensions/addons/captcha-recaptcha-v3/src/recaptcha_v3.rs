//! Google reCAPTCHA v3 engine.
use pelty_addon_captcha_common::Engine;

pub const ENGINE: Engine = Engine {
    id: "recaptcha_v3",
    name: "Google reCAPTCHA v3",
    verify_url: "https://www.google.com/recaptcha/api/siteverify",
    has_score: true,
    script: "https://www.google.com/recaptcha/api.js?render={site_key}",
    global: "grecaptcha",
    mode: "score",
    risk_score: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_widget() {
        let d = pelty_addon_captcha_common::describe(&ENGINE);
        assert_eq!(d["engine"], "recaptcha_v3");
        assert!(d["widget"]["script"]
            .as_str()
            .unwrap()
            .starts_with("https://"));
    }
}
