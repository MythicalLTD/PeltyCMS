//! Shared helpers for the panel / cloud modules.
use crate::{ProvisionError, ProvisionResult, ServiceCtx};
use rand::Rng;
use serde_json::Value;

/// `FieldSpec` with the conventional `provisioning.field.<key>` label.
#[macro_export]
macro_rules! fld {
    ($key:literal, $kind:literal, $req:expr) => {
        $crate::FieldSpec {
            key: $key,
            label: concat!("provisioning.field.", $key),
            kind: $kind,
            required: $req,
            default: None,
        }
    };
    ($key:literal, $kind:literal, $req:expr, $def:expr) => {
        $crate::FieldSpec {
            key: $key,
            label: concat!("provisioning.field.", $key),
            kind: $kind,
            required: $req,
            default: Some(serde_json::json!($def)),
        }
    };
}

/// Percent-encode a query/form component (RFC 3986 unreserved kept).
pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `application/x-www-form-urlencoded` body / query string.
pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Strong random password (letters, digits, symbols) for new panel accounts.
/// Returned to the caller once; never stored by PeltyCMS in plain text.
pub fn gen_password(len: usize) -> String {
    const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const DIGIT: &[u8] = b"23456789";
    const SYM: &[u8] = b"!@#%^*-_=+";
    let mut rng = rand::thread_rng();
    let all: Vec<u8> = [LOWER, UPPER, DIGIT, SYM].concat();
    let mut out: Vec<u8> = vec![
        LOWER[rng.gen_range(0..LOWER.len())],
        UPPER[rng.gen_range(0..UPPER.len())],
        DIGIT[rng.gen_range(0..DIGIT.len())],
        SYM[rng.gen_range(0..SYM.len())],
    ];
    while out.len() < len.max(8) {
        out.push(all[rng.gen_range(0..all.len())]);
    }
    for i in (1..out.len()).rev() {
        out.swap(i, rng.gen_range(0..=i));
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Service domain / hostname: custom field `domain` (or `hostname`), else
/// a deterministic `svc-<id>.<default_domain>`.
pub fn domain(svc: &ServiceCtx) -> ProvisionResult<String> {
    for k in ["domain", "hostname"] {
        if let Some(d) = svc
            .custom_fields
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            if !d
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                || d.len() > 253
            {
                return Err(ProvisionError::Config("invalid domain".into()));
            }
            return Ok(d.to_ascii_lowercase());
        }
    }
    Err(ProvisionError::Config(
        "service has no domain (custom field `domain`)".into(),
    ))
}

/// Hostname for VPS-style services: domain/hostname field or `svc-<id8>`.
pub fn hostname(svc: &ServiceCtx) -> String {
    domain(svc).unwrap_or_else(|_| format!("svc-{}", short(svc)))
}

pub fn short(svc: &ServiceCtx) -> String {
    svc.service_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect()
}

/// Panel usernames limited to 8 chars (cPanel/DirectAdmin/Hestia rules):
/// letters from the email + digits from the service id.
pub fn short_username(email: &str, service_id: &str) -> String {
    let mut u: String = email
        .split('@')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(4)
        .collect::<String>()
        .to_ascii_lowercase();
    if u.is_empty() {
        u.push('u');
    }
    let tail: String = service_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8 - u.len())
        .collect::<String>()
        .to_ascii_lowercase();
    u + &tail
}

pub fn str_or<'a>(v: &'a Value, key: &str, default: &'a str) -> &'a str {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default)
}

/// Numeric config as a string (accepts numbers or numeric strings).
pub fn num_str(v: &Value, key: &str) -> ProvisionResult<String> {
    crate::cfg_u64(v, key).map(|n| n.to_string())
}

/// 404 on delete means already gone (idempotent retries).
pub fn gone_ok<T>(r: ProvisionResult<T>) -> ProvisionResult<()> {
    match r {
        Err(ProvisionError::Rejected { status: 404, .. }) => Ok(()),
        r => r.map(|_| ()),
    }
}

/// Password supplied by the user for `change_password`, validated.
pub fn new_password(params: &Value) -> ProvisionResult<String> {
    match params.get("password").and_then(Value::as_str) {
        Some(p) if p.len() >= 8 && p.len() <= 128 => Ok(p.to_string()),
        Some(_) => Err(ProvisionError::Config(
            "password must be 8-128 characters".into(),
        )),
        None => Ok(gen_password(16)),
    }
}

/// Module data string field (set at create time).
pub fn md<'a>(svc: &'a ServiceCtx, key: &str) -> ProvisionResult<&'a str> {
    svc.module_data
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProvisionError::Config(format!(
                "module data has no {key}; create the service first"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_has_classes() {
        for _ in 0..50 {
            let p = gen_password(16);
            assert_eq!(p.len(), 16);
            assert!(
                p.chars().any(|c| c.is_ascii_lowercase())
                    && p.chars().any(|c| c.is_ascii_uppercase())
                    && p.chars().any(|c| c.is_ascii_digit())
            );
        }
    }
    #[test]
    fn short_usernames() {
        assert_eq!(short_username("alice.doe@x.io", "0b7a5f0e-11"), "alic0b7a");
        assert_eq!(short_username("123@x.io", "abcdef123"), "uabcdef1");
        assert_eq!(form(&[("a b", "c&d")]), "a%20b=c%26d");
    }
}
