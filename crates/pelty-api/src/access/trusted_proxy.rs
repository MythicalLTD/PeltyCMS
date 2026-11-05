//! Trusted proxies (WHMCS System > Trusted Proxies, plus `PELTY_TRUSTED_PROXIES`):
//! when the TCP peer is a trusted proxy, the client address is taken from
//! `X-Forwarded-For` (read right to left, skipping other trusted hops), else
//! from `X-Real-IP` / `CF-Connecting-IP`. Every later layer (IP rules, rate
//! limits, sessions, audit) then sees the real client in `ConnectInfo`, and
//! `X-Forwarded-Proto` becomes a [`ClientScheme`] extension. From any other
//! peer these headers are removed, so nothing downstream can be fooled.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::security::parse_cidr;
use crate::AppState;
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use ipnet::IpNet;
use pelty_core::AppError;
use pelty_database::audit;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const KEY: &str = "security.trusted_proxies";
const TTL: Duration = Duration::from_secs(30);
static CACHE: Mutex<Option<(Instant, Vec<IpNet>)>> = Mutex::new(None);

#[derive(Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TrustedProxies {
    /// IP addresses or CIDR ranges of reverse proxies / load balancers.
    pub proxies: Vec<String>,
}

/// Headers only a trusted proxy may set.
pub const FORWARD_HEADERS: [&str; 5] = ["x-forwarded-for", "x-real-ip", "cf-connecting-ip", "x-forwarded-proto", "x-forwarded-host"];

/// Scheme the client used (`https` / `http`), from a trusted proxy's
/// `X-Forwarded-Proto`; absent when the request came in directly.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientScheme(pub String);

/// Parse a list of IPs / CIDRs; `0.0.0.0/0`-style "trust everyone" entries and
/// garbage are dropped (returned in the second list).
pub fn parse_list(items: &[String]) -> (Vec<IpNet>, Vec<String>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for i in items {
        match parse_cidr(i) {
            Some(n) if n.prefix_len() > 0 => ok.push(n),
            _ => bad.push(i.clone()),
        }
    }
    (ok, bad)
}

/// Client address from all forwarding headers (XFF wins, then X-Real-IP,
/// then CF-Connecting-IP).
pub fn resolve_headers(trusted: &[IpNet], peer: IpAddr, xff: Option<&str>, real_ip: Option<&str>) -> IpAddr {
    if xff.is_some() || !trusted.iter().any(|n| n.contains(&peer)) {
        return resolve(trusted, peer, xff);
    }
    real_ip.and_then(|v| v.trim().parse::<IpAddr>().ok()).unwrap_or(peer)
}

/// `https` / `http` from `X-Forwarded-Proto` (first value), anything else ignored.
pub fn scheme(proto: Option<&str>) -> Option<String> {
    let p = proto?.split(',').next()?.trim().to_ascii_lowercase();
    matches!(p.as_str(), "https" | "http").then_some(p)
}

/// Client address for a request from `peer` with the given XFF header value.
pub fn resolve(trusted: &[IpNet], peer: IpAddr, xff: Option<&str>) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|n| n.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let Some(xff) = xff else { return peer };
    let hops: Vec<IpAddr> = xff.split(',').map_while(|h| h.trim().parse::<IpAddr>().ok()).collect();
    // Garbage anywhere in the chain: fall back to the peer rather than guess.
    if hops.len() != xff.split(',').count() {
        return peer;
    }
    hops.iter().rev().find(|ip| !is_trusted(ip)).or(hops.first()).copied().unwrap_or(peer)
}

async fn load(s: &AppState) -> Vec<IpNet> {
    if let Ok(g) = CACHE.lock() {
        if let Some((at, v)) = g.as_ref() {
            if at.elapsed() < TTL {
                return v.clone();
            }
        }
    }
    let cfg: TrustedProxies = match pelty_database::settings::get(&s.pool, KEY).await {
        Ok(Some(v)) => serde_json::from_value(v).unwrap_or_default(),
        Ok(None) => TrustedProxies::default(),
        Err(e) => {
            tracing::error!(error = %e, "trusted proxies unavailable");
            return vec![];
        }
    };
    let mut nets: Vec<IpNet> = cfg.proxies.iter().filter_map(|p| parse_cidr(p)).collect();
    nets.extend(env_nets(&s.cfg.trusted_proxies));
    if let Ok(mut g) = CACHE.lock() {
        *g = Some((Instant::now(), nets.clone()));
    }
    nets
}

fn env_nets(items: &[String]) -> Vec<IpNet> {
    static ENV: std::sync::OnceLock<Vec<IpNet>> = std::sync::OnceLock::new();
    ENV.get_or_init(|| {
        let (ok, bad) = parse_list(items);
        if !bad.is_empty() {
            tracing::warn!(ignored = ?bad, "PELTY_TRUSTED_PROXIES: entries ignored (invalid or trust-everyone)");
        }
        ok
    })
    .clone()
}

pub fn invalidate() {
    if let Ok(mut g) = CACHE.lock() {
        *g = None;
    }
}

/// Per-request risk signals for fraud screening, set by [`rewrite`]. The
/// geo country is only taken from a trusted proxy (Cloudflare, CloudFront or
/// an nginx GeoIP module); proxy headers are client-side proxy evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RequestSignals {
    pub geo_country: Option<String>,
    pub proxy_headers: Vec<String>,
}

/// Geo-country headers set by edge proxies, in priority order.
pub const COUNTRY_HEADERS: [&str; 4] = ["cf-ipcountry", "cloudfront-viewer-country", "x-geoip-country", "x-country-code"];
/// Headers forward proxies and anonymisers add on the client's side.
const CLIENT_PROXY_HEADERS: [&str; 5] = ["proxy-connection", "x-proxy-id", "x-bluecoat-via", "client-ip", "x-client-ip"];

/// A two-letter geo code; `XX` (unknown) is dropped, `T1` (Tor) kept.
pub fn geo_country(v: Option<&str>) -> Option<String> {
    let c = v?.trim().to_ascii_uppercase();
    (c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphanumeric()) && c != "XX").then_some(c)
}

/// Signals for a request from `peer`. A trusted peer's geo header is used and
/// an X-Forwarded-For with more than one untrusted hop means the client went
/// through another proxy; an untrusted peer that sends forwarding headers at
/// all is itself a proxy (or is forging them).
pub fn signals(trusted: &[IpNet], peer: IpAddr, h: impl Fn(&str) -> Option<String>) -> RequestSignals {
    let mut out = RequestSignals::default();
    let is_trusted = trusted.iter().any(|n| n.contains(&peer));
    if is_trusted {
        out.geo_country = COUNTRY_HEADERS.iter().find_map(|k| geo_country(h(k).as_deref()));
        if let Some(x) = h("x-forwarded-for") {
            let hops = x.split(',').filter_map(|p| p.trim().parse::<IpAddr>().ok()).filter(|ip| !trusted.iter().any(|n| n.contains(ip))).count();
            if hops > 1 {
                out.proxy_headers.push("x-forwarded-for".into());
            }
        }
    } else if !local(peer) {
        // A loopback/private peer is most likely an unconfigured local
        // reverse proxy, not the customer's; don't penalise every order.
        for k in ["via", "forwarded", "x-forwarded-for", "x-real-ip"] {
            if h(k).is_some() {
                out.proxy_headers.push(k.into());
            }
        }
    }
    for k in CLIENT_PROXY_HEADERS {
        if h(k).is_some() {
            out.proxy_headers.push(k.into());
        }
    }
    out
}

fn local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00 || v.to_ipv4_mapped().is_some_and(|m| m.is_loopback() || m.is_private()),
    }
}

pub async fn rewrite(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    if let Some(peer) = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0) {
        let nets = load(&s).await;
        let sig = signals(&nets, peer.ip(), |k| req.headers().get(k).and_then(|v| v.to_str().ok()).map(str::to_string));
        req.extensions_mut().insert(sig);
        if nets.iter().any(|n| n.contains(&peer.ip())) {
            let h = |k: &str| req.headers().get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
            let real = h("x-real-ip").or_else(|| h("cf-connecting-ip"));
            let ip = resolve_headers(&nets, peer.ip(), h("x-forwarded-for").as_deref(), real.as_deref());
            let proto = scheme(h("x-forwarded-proto").as_deref());
            if ip != peer.ip() {
                req.extensions_mut().insert(ConnectInfo(SocketAddr::new(ip, peer.port())));
            }
            if let Some(p) = proto {
                req.extensions_mut().insert(ClientScheme(p));
            }
        } else {
            for k in FORWARD_HEADERS.iter().chain(COUNTRY_HEADERS.iter()) {
                req.headers_mut().remove(*k);
            }
        }
    }
    next.run(req).await
}

#[utoipa::path(get, path = "/api/v1/admin/security/trusted-proxies", responses((status = 200), (status = 403)))]
pub async fn get_cfg(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    let v = pelty_database::settings::get(&s.pool, KEY).await?;
    let cfg: TrustedProxies = v.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    Ok(Json(json!(cfg)))
}

#[utoipa::path(put, path = "/api/v1/admin/security/trusted-proxies", request_body = TrustedProxies, responses((status = 200), (status = 422)))]
pub async fn put_cfg(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(mut b): Json<TrustedProxies>) -> ApiResult<Json<Value>> {
    if b.proxies.len() > 100 {
        return Err(AppError::Validation("error.validation").into());
    }
    let mut out = Vec::new();
    for p in &b.proxies {
        let n = parse_cidr(p).ok_or(AppError::Validation("error.invalid_cidr"))?;
        // Trusting everyone would let any client spoof its address.
        if n.prefix_len() == 0 {
            return Err(AppError::Validation("error.invalid_cidr").into());
        }
        let s = n.to_string();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    b.proxies = out;
    pelty_database::settings::set(&s.pool, KEY, json!(b)).await?;
    invalidate();
    audit::record(&s.pool, Some(u.user_id), "settings.trusted_proxies_updated", None, u.ip.as_deref(), json!(b)).await?;
    Ok(Json(json!(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_signals() {
        let t: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let hs = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert_eq!(geo_country(Some(" de ")), Some("DE".into()));
        assert_eq!(geo_country(Some("T1")), Some("T1".into()));
        assert_eq!(geo_country(Some("XX")), None);
        assert_eq!(geo_country(Some("DEU")), None);
        // Trusted: geo header honoured; one client hop is normal, two mean an extra proxy.
        let s = signals(&t, ip("10.0.0.5"), hs(&[("cf-ipcountry", "ro"), ("x-forwarded-for", "198.51.100.7")]));
        assert_eq!(s, RequestSignals { geo_country: Some("RO".into()), proxy_headers: vec![] });
        let s = signals(&t, ip("10.0.0.5"), hs(&[("x-forwarded-for", "6.6.6.6, 198.51.100.7, 10.1.1.1")]));
        assert_eq!(s.proxy_headers, vec!["x-forwarded-for"]);
        // Untrusted public peer: geo ignored, forwarding headers are evidence.
        let s = signals(&t, ip("203.0.113.9"), hs(&[("cf-ipcountry", "RO"), ("via", "1.1 squid"), ("proxy-connection", "keep-alive")]));
        assert_eq!(s, RequestSignals { geo_country: None, proxy_headers: vec!["via".into(), "proxy-connection".into()] });
        // Untrusted local peer (unconfigured nginx): not counted.
        assert!(signals(&t, ip("127.0.0.1"), hs(&[("x-forwarded-for", "1.2.3.4")])).proxy_headers.is_empty());
        assert_eq!(signals(&t, ip("203.0.113.9"), hs(&[])), RequestSignals::default());
    }

    #[test]
    fn resolves_client_behind_trusted_hops() {
        let t: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap(), "192.0.2.1/32".parse().unwrap()];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        // Untrusted peer: header ignored.
        assert_eq!(resolve(&t, ip("203.0.113.9"), Some("1.1.1.1")), ip("203.0.113.9"));
        // Trusted peer, no header.
        assert_eq!(resolve(&t, ip("10.0.0.5"), None), ip("10.0.0.5"));
        // Rightmost untrusted hop wins; a spoofed leftmost value is ignored.
        assert_eq!(resolve(&t, ip("10.0.0.5"), Some("6.6.6.6, 198.51.100.7, 192.0.2.1")), ip("198.51.100.7"));
        // All hops trusted: leftmost.
        assert_eq!(resolve(&t, ip("10.0.0.5"), Some("10.1.1.1, 10.2.2.2")), ip("10.1.1.1"));
        // Garbage: keep the peer.
        assert_eq!(resolve(&t, ip("10.0.0.5"), Some("evil, 1.2.3.4")), ip("10.0.0.5"));
        assert_eq!(resolve(&t, ip("10.0.0.5"), Some("1.2.3.4, ")), ip("10.0.0.5"));
    }

    #[test]
    fn real_ip_and_proto_only_from_trusted_peers() {
        let t: Vec<IpNet> = vec!["127.0.0.1/32".parse().unwrap()];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert_eq!(resolve_headers(&t, ip("127.0.0.1"), None, Some("198.51.100.7")), ip("198.51.100.7"));
        assert_eq!(resolve_headers(&t, ip("203.0.113.9"), None, Some("198.51.100.7")), ip("203.0.113.9"));
        // XFF wins over X-Real-IP; garbage Real-IP keeps the peer.
        assert_eq!(resolve_headers(&t, ip("127.0.0.1"), Some("198.51.100.8"), Some("198.51.100.7")), ip("198.51.100.8"));
        assert_eq!(resolve_headers(&t, ip("127.0.0.1"), None, Some("nope")), ip("127.0.0.1"));
        assert_eq!(scheme(Some("HTTPS, http")), Some("https".into()));
        assert_eq!(scheme(Some("ftp")), None);
        assert_eq!(scheme(None), None);
    }

    #[test]
    fn env_list_rejects_trust_everyone() {
        let (ok, bad) = parse_list(&["10.0.0.0/8".into(), "::1".into(), "0.0.0.0/0".into(), "x".into()]);
        assert_eq!(ok.len(), 2);
        assert_eq!(bad, vec!["0.0.0.0/0".to_string(), "x".to_string()]);
    }
}
