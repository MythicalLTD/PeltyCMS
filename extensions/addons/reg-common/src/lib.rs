//! Shared plumbing for the registrar addons: the configured connection
//! (settings, test mode, base URL, transport) and small parsing/encoding
//! helpers (XML, dates, query strings). No registrar logic lives here.
use pelty_provisioning::registrar::{Contact, RegistrarError, RegistrarResult};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use serde_json::{Map, Value};
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SettingField {
    pub key: &'static str,
    pub kind: &'static str,
    pub required: bool,
}

pub const fn f(key: &'static str, kind: &'static str, required: bool) -> SettingField {
    SettingField {
        key,
        kind,
        required,
    }
}

/// A registrar's configured connection. Unconfigured (empty settings)
/// connections refuse to run.
pub struct Conn {
    pub name: &'static str,
    pub transport: Arc<dyn RegTransport>,
    pub cfg: Map<String, Value>,
}

impl Conn {
    pub fn new(name: &'static str, transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        Conn {
            name,
            transport,
            cfg: settings.as_object().cloned().unwrap_or_default(),
        }
    }
    pub fn s(&self, k: &str) -> String {
        match self.cfg.get(k) {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        }
    }
    pub fn test(&self) -> bool {
        matches!(self.cfg.get("test_mode"), Some(Value::Bool(true)))
    }
    pub fn need(&self, keys: &[&str]) -> RegistrarResult<()> {
        for k in keys {
            if self.s(k).is_empty() {
                return Err(RegistrarError::Remote(format!(
                    "{} is not configured ({k})",
                    self.name
                )));
            }
        }
        Ok(())
    }
    pub fn base(&self, live: &str, test: &str) -> String {
        let o = self.s("api_base");
        if !o.is_empty() {
            return o.trim_end_matches('/').to_string();
        }
        if self.test() { test } else { live }.to_string()
    }
    pub async fn send(&self, r: RegRequest) -> RegistrarResult<(u16, String)> {
        self.transport.send(r).await.map_err(RegistrarError::Remote)
    }
}

pub fn split(domain: &str) -> (String, String) {
    let d = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    match d.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (d, String::new()),
    }
}

pub fn names(c: &Contact) -> (String, String) {
    let n = c.name.trim();
    match n.rsplit_once(' ') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (n.to_string(), n.to_string()),
    }
}

pub fn qs<K: AsRef<str>>(pairs: &[(K, String)]) -> String {
    let mut s = form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k.as_ref(), v);
    }
    s.finish()
}

/// Text inside the first `<tag ...>…</tag>` (case-insensitive tag name).
pub fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let t = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(&format!("<{t}")) {
        let start = from + i;
        let after = start + 1 + t.len();
        let next = lower.as_bytes().get(after).copied();
        if matches!(next, Some(b'>') | Some(b' ') | Some(b'/')) {
            let gt = start + lower[start..].find('>')?;
            if lower.as_bytes()[gt - 1] == b'/' {
                return Some(String::new());
            }
            let end = gt + lower[gt..].find(&format!("</{t}>"))?;
            return Some(xml_unescape(xml[gt + 1..end].trim()));
        }
        from = after;
    }
    None
}

/// All texts of `<tag>` elements.
pub fn xml_all(xml: &str, tag: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = xml;
    while let Some(v) = xml_text(rest, tag) {
        out.push(v);
        let lower = rest.to_ascii_lowercase();
        let Some(p) = lower.find(&format!("</{}>", tag.to_ascii_lowercase())) else {
            break;
        };
        rest = &rest[p + tag.len() + 3..];
    }
    out
}

/// Attribute value on the first `<tag ...>` element.
pub fn xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let start = xml
        .find(&format!("<{tag} "))
        .or_else(|| xml.find(&format!("<{tag}\n")))?;
    let end = start + xml[start..].find('>')?;
    let el = &xml[start..end];
    let key = format!("{attr}=\"");
    let i = el.find(&key)? + key.len();
    let j = i + el[i..].find('"')?;
    Some(xml_unescape(&el[i..j]))
}

pub fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `MM/DD/YYYY…` → `YYYY-MM-DD`.
pub fn us_date(s: &str) -> Option<String> {
    let d = s.split_whitespace().next()?;
    let p: Vec<&str> = d.split('/').collect();
    if p.len() != 3 {
        return None;
    }
    Some(format!("{}-{:0>2}-{:0>2}", p[2], p[0], p[1]))
}

/// First 10 chars when they look like an ISO date.
pub fn iso_date(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    let b = d.as_bytes();
    (b[4] == b'-' && b[7] == b'-' && d.chars().filter(|c| c.is_ascii_digit()).count() == 8)
        .then(|| d.to_string())
}

pub fn epoch_date(secs: i64) -> String {
    // Civil-from-days (Howard Hinnant), no external crate needed here.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn norm_status(s: &str) -> String {
    let l = s.to_ascii_lowercase();
    if l.contains("transfer") || l.contains("pending") {
        "transferring"
    } else if l.contains("expired") || l.contains("redemption") || l.contains("grace") {
        "expired"
    } else if l.contains("cancel") || l.contains("deleted") || l.contains("suspend") {
        "cancelled"
    } else {
        "active"
    }
    .into()
}

pub fn remote(msg: impl Into<String>) -> RegistrarError {
    let m: String = msg.into();
    let l = m.to_ascii_lowercase();
    if l.contains("auth")
        && (l.contains("code") || l.contains("info") || l.contains("epp"))
        && (l.contains("invalid") || l.contains("incorrect") || l.contains("wrong"))
    {
        RegistrarError::BadAuthCode
    } else if l.contains("not available")
        || l.contains("unavailable")
        || l.contains("already registered")
        || l.contains("is taken")
    {
        RegistrarError::Unavailable
    } else if l.contains("not found")
        || l.contains("does not exist")
        || l.contains("unknown domain")
        || l.contains("not in your account")
    {
        RegistrarError::UnknownDomain
    } else {
        RegistrarError::Remote(m.chars().take(300).collect())
    }
}

pub fn ok2xx(st: u16) -> bool {
    (200..300).contains(&st)
}

pub fn enc(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helpers() {
        assert_eq!(epoch_date(0), "1970-01-01");
        assert_eq!(epoch_date(1_830_297_600), "2028-01-01");
        assert_eq!(us_date("3/7/2028"), Some("2028-03-07".into()));
        assert_eq!(
            split("Sub.Example.co.uk"),
            ("sub".into(), "example.co.uk".into())
        );
        assert_eq!(
            xml_text("<a><B x=\"1\">hi &amp; yo</B></a>", "b").as_deref(),
            Some("hi & yo")
        );
    }
}
