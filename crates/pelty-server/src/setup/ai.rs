//! Optional provider configuration; secrets are collected without echoing.
use super::*;

pub(super) fn ask<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    p.say("AI ticket drafts are optional. Public ticket text and published KB context leave this server; provider charges may apply. Staff must review and send every reply.");
    let current = |key: &str, default: &str| {
        a.env
            .get(key)
            .cloned()
            .unwrap_or_else(|| ctx.c(key, default))
    };
    if !p.confirm(
        "Enable OpenAI-compatible ticket suggested replies?",
        !current("PELTY_AI_BASE_URL", "").is_empty(),
    )? {
        for key in ["PELTY_AI_BASE_URL", "PELTY_AI_API_KEY", "PELTY_AI_MODEL"] {
            a.env.insert(key.into(), String::new());
        }
        return Ok(());
    }
    let base = super::ask(
        p,
        "AI API base URL (include /v1; HTTPS, or HTTP on loopback)",
        &current("PELTY_AI_BASE_URL", "https://api.openai.com/v1"),
        |v| {
            let valid = url::Url::parse(v).is_ok_and(|u| {
                (u.scheme() == "https"
                    || (u.scheme() == "http"
                        && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
                    && u.query().is_none()
                    && u.fragment().is_none()
            });
            if valid {
                Ok(v.to_string())
            } else {
                Err("Use an HTTPS API base URL, or a loopback HTTP URL, without credentials, query or fragment.".into())
            }
        },
    )?;
    let model = super::ask(
        p,
        "AI model",
        &current("PELTY_AI_MODEL", "gpt-4o-mini"),
        |v| {
            if v.trim().is_empty() || v.len() > 200 {
                Err("Use a model name of 1-200 bytes.".into())
            } else {
                Ok(v.trim().to_string())
            }
        },
    )?;
    let mut key = current("PELTY_AI_API_KEY", "");
    loop {
        let value = p.secret("AI API key (Enter keeps existing key)", false)?;
        if !value.is_empty() {
            key = value;
        }
        if !key.trim().is_empty() && !key.chars().any(char::is_control) {
            break;
        }
        p.say("An API key is required and must not contain control characters.");
    }
    for (k, v) in [
        ("PELTY_AI_BASE_URL", base),
        ("PELTY_AI_API_KEY", key),
        ("PELTY_AI_MODEL", model),
    ] {
        a.env.insert(k.into(), v);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Script(std::collections::VecDeque<String>);
    impl Script {
        fn new(values: &[&str]) -> Self {
            Self(values.iter().map(|v| v.to_string()).collect())
        }
        fn next(&mut self) -> io::Result<String> {
            self.0
                .pop_front()
                .ok_or_else(|| io::Error::other("missing answer"))
        }
    }
    impl Prompt for Script {
        fn say(&mut self, _: &str) {}
        fn input(&mut self, _: &str, default: &str) -> io::Result<String> {
            let v = self.next()?;
            Ok(if v.is_empty() { default.into() } else { v })
        }
        fn secret(&mut self, _: &str, _: bool) -> io::Result<String> {
            self.next()
        }
        fn confirm(&mut self, _: &str, _: bool) -> io::Result<bool> {
            Ok(self.next()? == "y")
        }
        fn select(&mut self, _: &str, _: &[&str], _: usize) -> io::Result<usize> {
            unreachable!()
        }
        fn multi_select(&mut self, _: &str, _: &[&str], _: &[bool]) -> io::Result<Vec<usize>> {
            unreachable!()
        }
    }
    fn none(_: &str) -> Option<String> {
        None
    }
    #[test]
    fn optional_ai_answers_and_disable() {
        let ctx = Ctx {
            cur: &none,
            eula_accepted: true,
            languages: vec![],
            currencies: vec![],
            captcha: vec![],
            gateways: vec![],
        };
        let mut a = Answers::default();
        let mut p = Script::new(&["y", "https://example.test/v1", "test-model", "test-secret"]);
        ask(&mut p, &ctx, &mut a).unwrap();
        assert_eq!(a.env["PELTY_AI_MODEL"], "test-model");
        assert_eq!(a.env["PELTY_AI_API_KEY"], "test-secret");
        assert!(!summary(&a).contains("test-secret"));
        let mut p = Script::new(&["n"]);
        ask(&mut p, &ctx, &mut a).unwrap();
        assert!(a.env["PELTY_AI_API_KEY"].is_empty());
    }
}
