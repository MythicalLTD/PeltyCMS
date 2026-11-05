//! Addon settings of kind `secret` (API secrets, OAuth client secrets) are
//! sealed at rest with the app encryption key and never returned to the
//! browser: reads get [`MASK`], and saving [`MASK`] back keeps the old value.
use crate::addons::SettingSpec;
use crate::AppState;
use base64::Engine as _;
use serde_json::Value;

const PREFIX: &str = "sealed:v1:";
/// Placeholder shown instead of a stored secret.
pub const MASK: &str = "••••••••";

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn is_secret(spec: &[SettingSpec], key: &str) -> bool {
    spec.iter().any(|s| s.key == key && s.kind == "secret")
}

/// Seal secret fields of validated settings. `previous` (opened) supplies
/// the value when the client sent [`MASK`] back unchanged.
pub fn seal(s: &AppState, spec: &[SettingSpec], v: &Value, previous: &Value) -> Value {
    let mut out = v.clone();
    if let Some(o) = out.as_object_mut() {
        for (k, val) in o.iter_mut() {
            if !is_secret(spec, k) {
                continue;
            }
            if val.as_str() == Some(MASK) {
                *val = previous
                    .get(k)
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
            }
            if let (Some(b), Some(plain)) = (&s.secret_box, val.as_str()) {
                if !plain.is_empty() && !plain.starts_with(PREFIX) {
                    *val = Value::String(format!(
                        "{PREFIX}{}",
                        b64().encode(b.seal(plain.as_bytes()))
                    ));
                }
            }
        }
    }
    out
}

/// Open every sealed string (unreadable ones become empty).
pub fn open(s: &AppState, v: &Value) -> Value {
    let mut out = v.clone();
    if let Some(o) = out.as_object_mut() {
        for val in o.values_mut() {
            if let Some(rest) = val.as_str().and_then(|x| x.strip_prefix(PREFIX)) {
                let plain = s
                    .secret_box
                    .as_ref()
                    .and_then(|b| b.open(&b64().decode(rest).ok()?))
                    .and_then(|p| String::from_utf8(p).ok());
                *val = Value::String(plain.unwrap_or_default());
            }
        }
    }
    out
}

/// Settings for the admin UI: secret fields replaced by [`MASK`] when set.
pub fn mask(spec: &[SettingSpec], v: &Value) -> Value {
    let mut out = v.clone();
    if let Some(o) = out.as_object_mut() {
        for (k, val) in o.iter_mut() {
            if is_secret(spec, k) && val.as_str().is_some_and(|x| !x.is_empty()) {
                *val = Value::String(MASK.into());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Vec<SettingSpec> {
        vec![SettingSpec {
            key: "secret_key".into(),
            label: String::new(),
            kind: "secret".into(),
            required: false,
            choices: vec![],
            default: None,
        }]
    }

    #[test]
    fn mask_hides_only_set_secrets() {
        let v = serde_json::json!({"secret_key": "abc", "site_key": "pub"});
        assert_eq!(
            mask(&spec(), &v),
            serde_json::json!({"secret_key": MASK, "site_key": "pub"})
        );
        assert_eq!(
            mask(&spec(), &serde_json::json!({"secret_key": ""}))["secret_key"],
            ""
        );
    }
}
