//! HTTP transport used by every gateway. Gateways build [`HttpRequestSpec`]s
//! and send them through an [`HttpTransport`], so request logic is testable
//! without network (in-memory [`MockTransport`]) or against a local mock HTTP
//! server through [`ReqwestTransport`] plus the gateway's `api_base` override.
use crate::{BoxFuture, GatewayError};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// An HTTP request ready to send to a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequestSpec {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpRequestSpec {
    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }
}

/// Pluggable HTTP transport. Returns `(status, body)`; `Err` = network error.
pub trait HttpTransport: Send + Sync {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>>;
}

/// Refuses every request (offline default for the bare registry).
pub struct NoTransport;
impl HttpTransport for NoTransport {
    fn send(&self, _: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async { Err("HTTP transport not configured".into()) })
    }
}

#[cfg(not(target_family = "wasm"))]
/// Production transport (reqwest, rustls, 20 s timeout, no redirects).
pub struct ReqwestTransport {
    client: reqwest::Client,
}

#[cfg(not(target_family = "wasm"))]
impl ReqwestTransport {
    pub fn new() -> Result<Self, GatewayError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("PeltyCMS/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| GatewayError::Config(e.to_string()))?;
        Ok(ReqwestTransport { client })
    }
}

#[cfg(not(target_family = "wasm"))]
impl HttpTransport for ReqwestTransport {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            let m =
                reqwest::Method::from_bytes(req.method.as_bytes()).map_err(|e| e.to_string())?;
            let mut b = self.client.request(m, &req.url);
            for (k, v) in &req.headers {
                b = b.header(k, v);
            }
            if !req.body.is_empty() {
                b = b.body(req.body);
            }
            let r = b.send().await.map_err(|e| e.to_string())?;
            let status = r.status().as_u16();
            let text = r.text().await.map_err(|e| e.to_string())?;
            Ok((status, text))
        })
    }
}

/// In-memory scripted transport for tests: records requests, answers with
/// queued responses (or `default` when the queue is empty).
#[derive(Default, Clone)]
pub struct MockTransport {
    pub sent: Arc<Mutex<Vec<HttpRequestSpec>>>,
    queue: Arc<Mutex<VecDeque<(u16, String)>>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&self, status: u16, body: impl Into<String>) -> &Self {
        if let Ok(mut q) = self.queue.lock() {
            q.push_back((status, body.into()));
        }
        self
    }
    pub fn requests(&self) -> Vec<HttpRequestSpec> {
        self.sent.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl HttpTransport for MockTransport {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            if let Ok(mut s) = self.sent.lock() {
                s.push(req);
            }
            self.queue
                .lock()
                .ok()
                .and_then(|mut q| q.pop_front())
                .ok_or_else(|| "mock: no scripted response".to_string())
        })
    }
}

/// Send and decode JSON; non-2xx becomes `Provider(<message>)`.
pub async fn send_json(
    t: &dyn HttpTransport,
    spec: HttpRequestSpec,
) -> Result<serde_json::Value, GatewayError> {
    let (status, body) = t.send(spec).await.map_err(GatewayError::Provider)?;
    let v: serde_json::Value = if body.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&body)
            .map_err(|_| GatewayError::Provider(format!("bad json (HTTP {status})")))?
    };
    if !(200..300).contains(&status) {
        let msg = [
            &v["error"]["message"],
            &v["error"]["description"],
            &v["error"]["detail"],
            &v["message"],
            &v["detail"],
            &v["error"],
        ]
        .iter()
        .find_map(|x| x.as_str())
        .unwrap_or("error")
        .to_string();
        return Err(GatewayError::Provider(format!("HTTP {status}: {msg}")));
    }
    Ok(v)
}

/// Resolve the API base: config `api_base` overrides the default, but only
/// https URLs or plain http on loopback (local mock servers) are accepted.
pub fn api_base(cfg: &serde_json::Value, default: &str) -> Result<String, GatewayError> {
    match cfg.get("api_base").and_then(|v| v.as_str()).map(str::trim) {
        None | Some("") => Ok(default.to_string()),
        Some(b) => {
            let ok = b.starts_with("https://")
                || b.starts_with("http://127.0.0.1")
                || b.starts_with("http://localhost")
                || b.starts_with("http://[::1]");
            if ok {
                Ok(b.trim_end_matches('/').to_string())
            } else {
                Err(GatewayError::Config(
                    "api_base must be https (or http on loopback)".into(),
                ))
            }
        }
    }
}

/// application/x-www-form-urlencoded encoding.
pub fn form_encode(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn form_decode(body: &[u8]) -> Vec<(String, String)> {
    fn dec(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < b.len() => {
                    match std::str::from_utf8(&b[i + 1..i + 3])
                        .ok()
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                    {
                        Some(v) => {
                            out.push(v);
                            i += 2;
                        }
                        None => out.push(b'%'),
                    }
                }
                c => out.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    String::from_utf8_lossy(body)
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (dec(k), dec(v)),
            None => (dec(p), String::new()),
        })
        .collect()
}

/// Constant-time byte comparison.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let mut m = match Hmac::<sha2::Sha256>::new_from_slice(key) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_roundtrip() {
        let e = form_encode(&[("a b", "x&y=z%".into())]);
        assert_eq!(e, "a+b=x%26y%3Dz%25");
        assert_eq!(
            form_decode(e.as_bytes()),
            vec![("a b".into(), "x&y=z%".into())]
        );
        assert_eq!(form_decode(b"id=tr_1"), vec![("id".into(), "tr_1".into())]);
    }
    #[test]
    fn api_base_guard() {
        let d = "https://api.example";
        assert_eq!(api_base(&serde_json::json!({}), d).unwrap(), d);
        assert_eq!(
            api_base(&serde_json::json!({"api_base":"http://127.0.0.1:9/"}), d).unwrap(),
            "http://127.0.0.1:9"
        );
        assert!(api_base(&serde_json::json!({"api_base":"http://evil.example"}), d).is_err());
    }
    #[test]
    fn hmac_rfc4231_case2() {
        // RFC 4231 test case 2.
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
