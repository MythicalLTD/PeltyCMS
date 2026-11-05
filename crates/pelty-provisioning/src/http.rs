//! Small JSON HTTP helper shared by modules: bearer auth, timeouts, error
//! classification (4xx = rejected, 5xx/network = transient). Never logs keys.
use crate::{ProvisionError, ProvisionResult, ServerConn};
use serde_json::Value;
use std::net::{IpAddr, Ipv6Addr};
#[cfg(not(target_family = "wasm"))]
use reqwest::Client;
#[cfg(not(target_family = "wasm"))]
use std::net::SocketAddr;
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

/// HTTP method (target-independent; maps to reqwest natively and to the
/// addon host's `http-fetch` inside a WASM addon).
/// Not an upper-case ASCII HTTP method token (1-16 chars).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidMethod;

impl std::fmt::Display for InvalidMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid HTTP method")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method(std::borrow::Cow<'static, str>);

impl Method {
    pub const GET: Method = Method(std::borrow::Cow::Borrowed("GET"));
    pub const POST: Method = Method(std::borrow::Cow::Borrowed("POST"));
    pub const PUT: Method = Method(std::borrow::Cow::Borrowed("PUT"));
    pub const PATCH: Method = Method(std::borrow::Cow::Borrowed("PATCH"));
    pub const DELETE: Method = Method(std::borrow::Cow::Borrowed("DELETE"));
    pub fn from_bytes(b: &[u8]) -> Result<Method, InvalidMethod> {
        let s = std::str::from_utf8(b).map_err(|_| InvalidMethod)?;
        if s.is_empty() || s.len() > 16 || !s.bytes().all(|c| c.is_ascii_uppercase()) {
            return Err(InvalidMethod);
        }
        Ok(Method(std::borrow::Cow::Owned(s.to_string())))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A fully built outgoing request (used by the WASM backend and the addon host).
#[derive(Debug, Clone)]
pub struct RawRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Backend that performs requests inside a WASM addon (set once by the addon SDK).
#[cfg(target_family = "wasm")]
pub type Backend = fn(RawRequest) -> Result<(u16, String), String>;
#[cfg(target_family = "wasm")]
static BACKEND: std::sync::OnceLock<Backend> = std::sync::OnceLock::new();
/// Install the request backend (WASM addons only).
#[cfg(target_family = "wasm")]
pub fn set_backend(b: Backend) {
    let _ = BACKEND.set(b);
}

#[cfg(not(target_family = "wasm"))]
fn builder() -> reqwest::ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("PeltyCMS/", env!("CARGO_PKG_VERSION")))
}

/// True for addresses a panel URL must never reach: link-local (incl. the
/// 169.254.169.254 cloud metadata endpoint), well-known metadata IPs,
/// unspecified, multicast and broadcast. Loopback/private ranges stay allowed
/// because self-hosted panels commonly live on a LAN.
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_link_local() || v4.is_unspecified() || v4.is_multicast() || v4.is_broadcast() || v4.octets() == [100, 100, 100, 200] // Alibaba metadata
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_forbidden_ip(IpAddr::V4(v4));
            }
            let seg = v6.segments();
            v6.is_unspecified() || v6.is_multicast() || (seg[0] & 0xffc0) == 0xfe80 || v6 == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254) // AWS IMDS v6
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn allow_link_local() -> bool {
    // `1`/`true` lets provisioning modules reach link-local addresses (off by default).
    std::env::var("PELTY_PROVISION_ALLOW_LINK_LOCAL").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
}

/// True for loopback, RFC 1918, CGNAT, ULA, link-local and other
/// non-public addresses (refused for public-only requests).
pub fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast() || (v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64) || v.octets()[0] == 0,
        IpAddr::V6(v) => {
            if let Some(m) = v.to_ipv4_mapped() {
                return is_non_public_ip(IpAddr::V4(m));
            }
            v.is_loopback() || v.is_unspecified() || v.is_multicast() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(not(target_family = "wasm"))]
tokio::task_local! {
    /// Set by the addon runtime for `http: ["*"]` calls without `http_private`:
    /// the pinned addresses must all be public (closes the DNS-rebinding gap
    /// between the runtime's pre-check and the actual connect).
    pub static PUBLIC_ONLY: bool;
}

#[cfg(not(target_family = "wasm"))]
/// Run `f` with the public-only guard on.
pub async fn public_only<F: std::future::Future>(f: F) -> F::Output {
    PUBLIC_ONLY.scope(true, f).await
}

#[cfg(not(target_family = "wasm"))]
/// Resolve the host once, reject forbidden targets, and pin the connection to
/// the vetted addresses so DNS rebinding cannot swap them afterwards.
async fn guarded_client(url: &reqwest::Url) -> ProvisionResult<Client> {
    let host = url.host_str().ok_or_else(|| ProvisionError::Config("base_url has no host".into()))?.to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let lookup_host = host.trim_start_matches('[').trim_end_matches(']').to_string();
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((lookup_host.as_str(), port)).await.map_err(|e| ProvisionError::Transient(format!("dns: {e}")))?.collect();
    if addrs.is_empty() {
        return Err(ProvisionError::Transient("dns: no addresses".into()));
    }
    if !allow_link_local() && addrs.iter().any(|a| is_forbidden_ip(a.ip())) {
        return Err(ProvisionError::Config("base_url resolves to a forbidden address (link-local/metadata)".into()));
    }
    if PUBLIC_ONLY.try_with(|v| *v).unwrap_or(false) && addrs.iter().any(|a| is_non_public_ip(a.ip())) {
        return Err(ProvisionError::Config("private address refused (needs http_private)".into()));
    }
    builder().resolve_to_addrs(&host, &addrs).build().map_err(|e| ProvisionError::Config(e.to_string()))
}

pub async fn call(server: &ServerConn, method: Method, path: &str, accept: &str, body: Option<&Value>) -> ProvisionResult<Value> {
    let auth = format!("Bearer {}", server.api_key);
    let raw = body.map(|b| ("application/json", b.to_string()));
    call_raw(server, method, path, &[("accept", accept), ("authorization", &auth)], raw).await
}

/// Lower-level request: caller-supplied headers and a raw body
/// (content type, bytes). Same URL guard and error classification as `call`.
pub async fn call_raw(server: &ServerConn, method: Method, path: &str, headers: &[(&str, &str)], body: Option<(&str, String)>) -> ProvisionResult<Value> {
    let base = server.base_url.trim_end_matches('/');
    if !(base.starts_with("https://") || base.starts_with("http://")) {
        return Err(ProvisionError::Config("base_url must start with http:// or https://".into()));
    }
    let url = url::Url::parse(&format!("{base}{path}")).map_err(|_| ProvisionError::Config("invalid base_url".into()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProvisionError::Config("base_url must not contain credentials".into()));
    }
    let mut hs: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut b = String::new();
    if let Some((ct, body)) = body {
        hs.push(("content-type".into(), ct.to_string()));
        b = body;
    }
    let (status, text) = send(RawRequest { method: method.as_str().to_string(), url: url.to_string(), headers: hs, body: b }).await?;
    classify(status, &text)
}

/// Send a vetted request natively (SSRF-guarded reqwest).
#[cfg(not(target_family = "wasm"))]
async fn send(r: RawRequest) -> ProvisionResult<(u16, String)> {
    let url = reqwest::Url::parse(&r.url).map_err(|_| ProvisionError::Config("invalid base_url".into()))?;
    let m = reqwest::Method::from_bytes(r.method.as_bytes()).map_err(|_| ProvisionError::Config("invalid method".into()))?;
    let mut req = guarded_client(&url).await?.request(m, url);
    for (k, v) in &r.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if !r.body.is_empty() {
        req = req.body(r.body);
    }
    let res = req.send().await.map_err(|e| ProvisionError::Transient(e.without_url().to_string()))?;
    let status = res.status().as_u16();
    let text = res.text().await.map_err(|e| ProvisionError::Transient(e.without_url().to_string()))?;
    Ok((status, text))
}

/// Inside a WASM addon every request goes through the host (`http-fetch`).
#[cfg(target_family = "wasm")]
async fn send(r: RawRequest) -> ProvisionResult<(u16, String)> {
    host_send(r).map_err(ProvisionError::Transient)
}

/// Raw send through the installed host backend (WASM addons only).
#[cfg(target_family = "wasm")]
pub fn host_send(r: RawRequest) -> Result<(u16, String), String> {
    let b = BACKEND.get().ok_or_else(|| "http backend not installed".to_string())?;
    b(r)
}

/// Same 2xx / 4xx / 5xx classification for every transport.
pub fn classify(status: u16, text: &str) -> ProvisionResult<Value> {
    let json: Value = if text.trim().is_empty() { Value::Null } else { serde_json::from_str(text).unwrap_or(Value::String(text.chars().take(500).collect())) };
    if (200..300).contains(&status) {
        return Ok(json);
    }
    let message = extract_message(&json);
    if status >= 500 || status == 429 {
        Err(ProvisionError::Transient(format!("{status}: {message}")))
    } else {
        Err(ProvisionError::Rejected { status, message })
    }
}

/// Generic SSRF-guarded fetch used by the WASM addon host: any method, any
/// http(s) URL without credentials, link-local/metadata refused, body capped.
#[cfg(not(target_family = "wasm"))]
pub async fn fetch(r: RawRequest, max: usize) -> Result<(u16, String), String> {
    if !(r.url.starts_with("https://") || r.url.starts_with("http://")) {
        return Err("url must be http(s)".into());
    }
    let u = reqwest::Url::parse(&r.url).map_err(|_| "invalid url".to_string())?;
    if !u.username().is_empty() || u.password().is_some() {
        return Err("url must not contain credentials".into());
    }
    let m = reqwest::Method::from_bytes(r.method.as_bytes()).map_err(|_| "invalid method".to_string())?;
    let mut req = guarded_client(&u).await.map_err(|e| e.to_string())?.request(m, u);
    for (k, v) in &r.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if !r.body.is_empty() {
        req = req.body(r.body);
    }
    let mut res = req.send().await.map_err(|e| e.without_url().to_string())?;
    let status = res.status().as_u16();
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.without_url().to_string())? {
        if out.len() + chunk.len() > max {
            return Err("response too large".into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok((status, String::from_utf8_lossy(&out).into_owned()))
}

#[cfg(not(target_family = "wasm"))]
/// POST a body to an absolute URL with the same SSRF guard as panel calls.
/// Returns the HTTP status (any status; callers decide what counts as
/// delivered) or a network error. Used for plugin webhook deliveries.
pub async fn post_to(url: &str, headers: &[(&str, &str)], body: String) -> Result<u16, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("url must be http(s)".into());
    }
    let u = reqwest::Url::parse(url).map_err(|_| "invalid url".to_string())?;
    if !u.username().is_empty() || u.password().is_some() {
        return Err("url must not contain credentials".into());
    }
    let mut req = guarded_client(&u).await.map_err(|e| e.to_string())?.post(u);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = req.header("content-type", "application/json").body(body).send().await.map_err(|e| e.without_url().to_string())?;
    Ok(res.status().as_u16())
}

#[cfg(not(target_family = "wasm"))]
/// GET an absolute URL with the SSRF guard, refusing bodies over `max` bytes.
/// Used for the addon registry index and package downloads.
pub async fn get_bytes(url: &str, max: usize) -> Result<Vec<u8>, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("url must be http(s)".into());
    }
    let u = reqwest::Url::parse(url).map_err(|_| "invalid url".to_string())?;
    if !u.username().is_empty() || u.password().is_some() {
        return Err("url must not contain credentials".into());
    }
    let mut res = guarded_client(&u).await.map_err(|e| e.to_string())?.get(u).send().await.map_err(|e| e.without_url().to_string())?;
    if !res.status().is_success() {
        return Err(format!("http {}", res.status().as_u16()));
    }
    if res.content_length().is_some_and(|l| l > max as u64) {
        return Err("response too large".into());
    }
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.without_url().to_string())? {
        if out.len() + chunk.len() > max {
            return Err("response too large".into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn extract_message(v: &Value) -> String {
    // Pterodactyl: {"errors":[{"detail":..}]}; FeatherPanel: {"error_message":..}
    let m = v.pointer("/errors/0/detail").or_else(|| v.get("error_message")).or_else(|| v.get("message")).and_then(Value::as_str);
    m.map(str::to_string).unwrap_or_else(|| v.to_string().chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn non_public_ranges() {
        for ip in ["127.0.0.1", "10.1.2.3", "192.168.0.1", "172.16.0.1", "100.64.0.1", "169.254.169.254", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(is_non_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "93.184.216.34", "2606:4700::1111"] {
            assert!(!is_non_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    /// DNS-rebinding guard: under `public_only`, a name resolving to loopback
    /// is refused at connect time even though no pre-check ran.
    #[tokio::test]
    async fn public_only_refuses_private_at_connect() {
        let r = RawRequest { method: "GET".into(), url: "http://localhost:9/".into(), headers: vec![], body: String::new() };
        let e = public_only(fetch(r, 1024)).await.unwrap_err();
        assert!(e.contains("private address refused"), "{e}");
    }

    #[test]
    fn blocks_metadata_and_link_local() {
        assert!(is_forbidden_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
        assert!(is_forbidden_ip(IpAddr::V4(Ipv4Addr::new(100, 100, 100, 200))));
        assert!(is_forbidden_ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(is_forbidden_ip("fe80::1".parse().unwrap()));
        assert!(is_forbidden_ip("fd00:ec2::254".parse().unwrap()));
        assert!(is_forbidden_ip("::ffff:169.254.169.254".parse().unwrap()));
        assert!(!is_forbidden_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(!is_forbidden_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(!is_forbidden_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    #[tokio::test]
    #[cfg(not(target_family = "wasm"))]
    async fn call_refuses_metadata_endpoint() {
        let s = ServerConn { base_url: "http://169.254.169.254".into(), api_key: "k".into(), config: Value::Null };
        let e = call(&s, Method::GET, "/latest/meta-data", "application/json", None).await.unwrap_err();
        assert!(matches!(e, ProvisionError::Config(_)), "{e:?}");
    }
}
