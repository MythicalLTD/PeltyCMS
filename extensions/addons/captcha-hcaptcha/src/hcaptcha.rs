//! hCaptcha engine.
use pelty_addon_captcha_common::Engine;

pub const ENGINE: Engine = Engine {
    id: "hcaptcha",
    name: "hCaptcha",
    verify_url: "https://api.hcaptcha.com/siteverify",
    has_score: true,
    script: "https://js.hcaptcha.com/1/api.js?render=explicit",
    global: "hcaptcha",
    mode: "widget",
    risk_score: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_widget() {
        let d = pelty_addon_captcha_common::describe(&ENGINE);
        assert_eq!(d["engine"], "hcaptcha");
        assert!(d["widget"]["script"]
            .as_str()
            .unwrap()
            .starts_with("https://"));
    }
}
