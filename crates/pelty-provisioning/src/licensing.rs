//! Software licensing primitives (the `licensing` addon is the module) (WHMCS Licensing Addon style). No remote
//! panel: `create` mints a license key, lifecycle actions are reflected by
//! the service status, and the public verify endpoint
//! (`POST /api/v1/licensing/verify`) answers with a response signed with
//! HMAC-SHA256 under the assigned server's API key (the "secret").
//! The first check binds the license to the caller's domain / IP / directory
//! (if the product allows binding); `reissue` clears the binding.
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// `PREFIX-XXXXX-XXXXX-XXXXX-XXXXX` (100 bits of randomness, no 0/O/1/I).
pub fn generate_key(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    let groups: Vec<String> = (0..4)
        .map(|_| {
            (0..5)
                .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
                .collect()
        })
        .collect();
    let p: String = prefix
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(10)
        .collect::<String>()
        .to_ascii_uppercase();
    if p.is_empty() {
        groups.join("-")
    } else {
        format!("{p}-{}", groups.join("-"))
    }
}

pub fn valid_key_format(k: &str) -> bool {
    k.len() <= 40
        && k.split('-').count() >= 4
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Check {
    pub domain: Option<String>,
    pub ip: Option<String>,
    pub directory: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Verdict {
    /// active | suspended | expired | invalid | reissued
    pub status: String,
    pub message: String,
    /// Binding to persist into `module_data` when the license was unbound.
    #[serde(skip)]
    pub bind: Option<Check>,
}

fn norm(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(|v| {
            v.trim()
                .to_ascii_lowercase()
                .trim_start_matches("www.")
                .to_string()
        })
        .filter(|v| !v.is_empty())
}

/// Decide a license check. `service_status` is the PeltyCMS service status;
/// `product` the product module config (`bind_domain`, `bind_ip`,
/// `bind_directory` booleans); `data` the service `module_data`.
pub fn decide(service_status: &str, product: &Value, data: &Value, check: &Check) -> Verdict {
    let v = |status: &str, message: &str| Verdict {
        status: status.into(),
        message: message.into(),
        bind: None,
    };
    match service_status {
        "active" => {}
        "suspended" => return v("suspended", "license suspended"),
        "terminated" | "cancelled" | "fraud" => return v("expired", "license is no longer valid"),
        _ => return v("invalid", "license is not active"),
    }
    let flag = |k: &str| {
        product
            .get(k)
            .map(|x| x.as_bool().unwrap_or(x == "1" || x == "true"))
            .unwrap_or(k != "bind_directory")
    };
    let bound = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_string);
    let req = Check {
        domain: norm(&check.domain),
        ip: norm(&check.ip),
        directory: norm(&check.directory),
    };
    let pairs = [
        ("bind_domain", "bound_domain", &req.domain),
        ("bind_ip", "bound_ip", &req.ip),
        ("bind_directory", "bound_directory", &req.directory),
    ];
    let mut needs_bind = false;
    for (flag_key, bound_key, value) in pairs {
        if !flag(flag_key) {
            continue;
        }
        match (bound(bound_key), value) {
            (Some(b), Some(v)) if b == *v => {}
            (Some(_), _) => {
                return v(
                    "invalid",
                    "license is bound to another installation; reissue it in the client area",
                )
            }
            (None, Some(_)) => needs_bind = true,
            (None, None) => return v("invalid", "missing installation details"),
        }
    }
    Verdict {
        status: "active".into(),
        message: "ok".into(),
        bind: needs_bind.then_some(req),
    }
}

/// Signature over the canonical response JSON (keys sorted by serde_json's
/// BTreeMap order), clients recompute it with the shared secret.
pub fn sign(secret: &str, payload: &Value) -> String {
    let mut m =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap_or_else(|_| unreachable!());
    m.update(payload.to_string().as_bytes());
    hex::encode(m.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn keys_unique_and_formatted() {
        let a = generate_key("pelty");
        let b = generate_key("pelty");
        assert_ne!(a, b);
        assert!(a.starts_with("PELTY-") && a.len() == 6 + 23, "{a}");
        assert!(valid_key_format(&a));
        assert!(!valid_key_format("x' OR 1=1"));
    }
    #[test]
    fn binds_then_enforces() {
        let p = json!({});
        let c = Check {
            domain: Some("www.Example.com".into()),
            ip: Some("1.2.3.4".into()),
            directory: None,
        };
        let v = decide("active", &p, &json!({}), &c);
        assert_eq!(v.status, "active");
        let b = v.bind.unwrap();
        assert_eq!(b.domain.as_deref(), Some("example.com"));
        let data = json!({ "bound_domain": "example.com", "bound_ip": "1.2.3.4" });
        let v2 = decide("active", &p, &data, &c);
        assert_eq!(v2.status, "active");
        assert!(v2.bind.is_none());
        let other = Check {
            domain: Some("evil.com".into()),
            ..c.clone()
        };
        assert_eq!(decide("active", &p, &data, &other).status, "invalid");
        assert_eq!(decide("suspended", &p, &data, &c).status, "suspended");
        assert_eq!(decide("terminated", &p, &data, &c).status, "expired");
        // binding disabled: anything goes
        let free = json!({ "bind_domain": false, "bind_ip": "false" });
        assert_eq!(decide("active", &free, &data, &other).status, "active");
    }
    #[test]
    fn signature_stable() {
        let p = json!({ "status": "active", "key": "K" });
        assert_eq!(sign("secret-secret-123", &p), sign("secret-secret-123", &p));
        assert_ne!(sign("secret-secret-123", &p), sign("other-secret-1234", &p));
    }
}
