//! Tiny `{{name}}` template renderer used by the webhook and agent modules.
//! No logic, no loops: unknown placeholders render empty, so a template can
//! never reach data it was not given. Values are escaped for their target
//! (JSON string content, URL path segment or a shell-free plain string).
use crate::ServiceCtx;
use hmac::{digest::KeyInit, Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    /// Inside a JSON string literal: the template supplies the quotes.
    Json,
    /// Percent-encoded (path or query component).
    Url,
    Plain,
}

/// Variables available to templates for one action on one service.
pub fn vars(action: &str, svc: &ServiceCtx) -> BTreeMap<String, String> {
    let mut v = BTreeMap::new();
    v.insert("action".into(), action.into());
    v.insert("service_id".into(), svc.service_id.clone());
    v.insert("email".into(), svc.customer_email.clone());
    v.insert("first_name".into(), svc.customer_first_name.clone());
    v.insert("last_name".into(), svc.customer_last_name.clone());
    v.insert(
        "remote_id".into(),
        svc.remote_id.clone().unwrap_or_default(),
    );
    flatten("config", &svc.product_config, &mut v);
    flatten("custom", &svc.custom_fields, &mut v);
    flatten("data", &svc.module_data, &mut v);
    v
}

fn flatten(prefix: &str, val: &Value, out: &mut BTreeMap<String, String>) {
    if let Value::Object(m) = val {
        for (k, x) in m {
            let s = match x {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            out.insert(format!("{prefix}.{k}"), s);
        }
    }
}

fn escape(s: &str, e: Escape) -> String {
    match e {
        Escape::Plain => s.to_string(),
        Escape::Json => {
            let q = serde_json::to_string(s).unwrap_or_default();
            q[1..q.len() - 1].to_string()
        }
        Escape::Url => s
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect(),
    }
}

pub fn render(tpl: &str, vars: &BTreeMap<String, String>, e: Escape) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("}}") {
            Some(j) => {
                let name = after[..j].trim();
                out.push_str(&escape(vars.get(name).map(String::as_str).unwrap_or(""), e));
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Hex HMAC-SHA256 of `msg` keyed with `secret`.
pub fn sign(secret: &str, msg: &[u8]) -> String {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    m.update(msg);
    hex::encode(m.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_and_escapes() {
        let mut v = BTreeMap::new();
        v.insert("email".to_string(), "a\"b@x.com".to_string());
        v.insert("config.plan".to_string(), "gold plan".to_string());
        assert_eq!(
            render(r#"{"e":"{{email}}","x":"{{nope}}"}"#, &v, Escape::Json),
            r#"{"e":"a\"b@x.com","x":""}"#
        );
        assert_eq!(
            render("/p/{{ config.plan }}", &v, Escape::Url),
            "/p/gold%20plan"
        );
        assert_eq!(render("open {{", &v, Escape::Plain), "open {{");
    }

    #[test]
    fn signature_is_stable() {
        // RFC 4231 test case 2.
        assert_eq!(
            sign("Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
