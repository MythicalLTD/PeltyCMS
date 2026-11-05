//! Registrar host plumbing: the HTTP transports registrar addons send
//! through, and the RDAP lookup used by the domain search. The registrars
//! themselves (Namecheap, ResellerClub, Enom, OpenSRS, GoDaddy, CentralNic,
//! generic JSON) live in `extensions/addons/reg-*`.
//!
//! Every module builds plain HTTP requests and sends them through a
//! [`RegTransport`], so the request/response logic is fully testable with
//! [`MockRegTransport`] and never needs the network in tests. Credentials come
//! from the registrar addon's settings (see `make docs`); a configured
//! instance is produced with [`crate::registrar::RegistrarModule::configure`].
use crate::BoxFuture;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct RegRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RegRequest {
    pub fn get(url: String) -> Self {
        RegRequest {
            method: "GET",
            url,
            headers: vec![],
            body: String::new(),
        }
    }
    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }
    /// Decoded query parameter (first match).
    pub fn query(&self, k: &str) -> Option<String> {
        let q = self.url.split_once('?')?.1;
        form_urlencoded::parse(q.as_bytes())
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.into_owned())
    }
}

/// Returns `(status, body)`; `Err` = network error.
pub trait RegTransport: Send + Sync {
    fn send(&self, req: RegRequest) -> BoxFuture<'_, Result<(u16, String), String>>;
}

#[cfg(not(target_family = "wasm"))]
/// Production transport: reqwest + rustls, 30 s timeout, no redirects.
pub struct ReqwestRegTransport {
    client: Option<reqwest::Client>,
}

#[cfg(not(target_family = "wasm"))]
impl Default for ReqwestRegTransport {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("PeltyCMS/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok();
        ReqwestRegTransport { client }
    }
}

#[cfg(not(target_family = "wasm"))]
impl RegTransport for ReqwestRegTransport {
    fn send(&self, req: RegRequest) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            let c = self.client.as_ref().ok_or("http client unavailable")?;
            let m =
                reqwest::Method::from_bytes(req.method.as_bytes()).map_err(|e| e.to_string())?;
            let mut b = c.request(m, &req.url);
            for (k, v) in &req.headers {
                b = b.header(k, v);
            }
            if !req.body.is_empty() {
                b = b.body(req.body);
            }
            let r = b.send().await.map_err(|e| e.to_string())?;
            let st = r.status().as_u16();
            Ok((st, r.text().await.map_err(|e| e.to_string())?))
        })
    }
}

/// Default transport: reqwest natively, the addon host inside WASM.
pub fn default_transport() -> Arc<dyn RegTransport> {
    #[cfg(not(target_family = "wasm"))]
    return Arc::new(ReqwestRegTransport::default());
    #[cfg(target_family = "wasm")]
    return Arc::new(HostRegTransport);
}

/// Sends through the addon host's `http-fetch` (WASM addons only).
#[cfg(target_family = "wasm")]
pub struct HostRegTransport;

#[cfg(target_family = "wasm")]
impl RegTransport for HostRegTransport {
    fn send(&self, req: RegRequest) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            crate::http::host_send(crate::http::RawRequest {
                method: req.method.to_string(),
                url: req.url,
                headers: req.headers,
                body: req.body,
            })
        })
    }
}

/// Scripted transport for tests: records requests, pops queued responses.
#[derive(Default, Clone)]
pub struct MockRegTransport {
    sent: Arc<Mutex<Vec<RegRequest>>>,
    queue: Arc<Mutex<VecDeque<(u16, String)>>>,
}

impl MockRegTransport {
    pub fn push(&self, status: u16, body: impl Into<String>) -> &Self {
        if let Ok(mut q) = self.queue.lock() {
            q.push_back((status, body.into()));
        }
        self
    }
    pub fn requests(&self) -> Vec<RegRequest> {
        self.sent.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl RegTransport for MockRegTransport {
    fn send(&self, req: RegRequest) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async move {
            if let Ok(mut s) = self.sent.lock() {
                s.push(req);
            }
            self.queue
                .lock()
                .ok()
                .and_then(|mut q| q.pop_front())
                .ok_or_else(|| "mock: no scripted response".into())
        })
    }
}

// RDAP
/// Public registration data for a name (RDAP, RFC 9083).
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct RdapInfo {
    pub registered: bool,
    pub registrar: Option<String>,
    pub created: Option<String>,
    pub expires: Option<String>,
    pub status: Vec<String>,
    pub nameservers: Vec<String>,
}

/// WHOIS-style lookup over RDAP. `base` is an RDAP service or bootstrap
/// redirector such as `https://rdap.org/` (redirects are not followed by the
/// production transport, so point it at a server that answers directly or
/// accept the 3xx as "registered"). 404 = not registered.
pub async fn rdap_lookup(
    t: &dyn RegTransport,
    base: &str,
    domain: &str,
) -> Result<RdapInfo, String> {
    let url = format!(
        "{}/domain/{}",
        base.trim_end_matches('/'),
        enc(&domain.to_ascii_lowercase())
    );
    let req = RegRequest {
        method: "GET",
        url,
        headers: vec![("Accept".into(), "application/rdap+json".into())],
        body: String::new(),
    };
    let (st, body) = t.send(req).await?;
    if st == 404 {
        return Ok(RdapInfo {
            registered: false,
            registrar: None,
            created: None,
            expires: None,
            status: vec![],
            nameservers: vec![],
        });
    }
    if (300..400).contains(&st) {
        return Ok(RdapInfo {
            registered: true,
            registrar: None,
            created: None,
            expires: None,
            status: vec![],
            nameservers: vec![],
        });
    }
    if !ok2xx(st) {
        return Err(format!("RDAP HTTP {st}"));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "RDAP: bad json".to_string())?;
    let event = |action: &str| {
        v["events"]
            .as_array()
            .and_then(|a| a.iter().find(|e| e["eventAction"] == action))
            .and_then(|e| e["eventDate"].as_str())
            .and_then(iso_date)
    };
    let registrar = v["entities"].as_array().and_then(|a| {
        a.iter()
            .find(|e| {
                e["roles"]
                    .as_array()
                    .is_some_and(|r| r.iter().any(|x| x == "registrar"))
            })
            .and_then(|e| {
                e["vcardArray"][1].as_array().and_then(|props| {
                    props
                        .iter()
                        .find(|p| p[0] == "fn")
                        .and_then(|p| p[3].as_str().map(str::to_string))
                })
            })
    });
    Ok(RdapInfo {
        registered: true,
        registrar,
        created: event("registration"),
        expires: event("expiration"),
        status: v["status"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        nameservers: v["nameservers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|n| n["ldhName"].as_str().map(|x| x.to_ascii_lowercase()))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn iso_date(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    let b = d.as_bytes();
    (b[4] == b'-' && b[7] == b'-' && d.chars().filter(|c| c.is_ascii_digit()).count() == 8)
        .then(|| d.to_string())
}

fn ok2xx(st: u16) -> bool {
    (200..300).contains(&st)
}

fn enc(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rdap() {
        let t = MockRegTransport::default();
        t.push(404, "");
        assert!(
            !rdap_lookup(&t, "https://rdap.test/", "free-name.com")
                .await
                .unwrap()
                .registered
        );
        assert_eq!(
            t.requests()[0].url,
            "https://rdap.test/domain/free-name.com"
        );
        t.push(200, r#"{"objectClassName":"domain","ldhName":"EXAMPLE.COM","status":["client transfer prohibited"],
            "events":[{"eventAction":"registration","eventDate":"1995-08-14T04:00:00Z"},{"eventAction":"expiration","eventDate":"2027-08-13T04:00:00Z"}],
            "nameservers":[{"ldhName":"A.IANA-SERVERS.NET"}],
            "entities":[{"roles":["registrar"],"vcardArray":["vcard",[["version",{},"text","4.0"],["fn",{},"text","RESERVED-IANA"]]]}]}"#);
        let i = rdap_lookup(&t, "https://rdap.test", "example.com")
            .await
            .unwrap();
        assert!(i.registered);
        assert_eq!(i.expires.as_deref(), Some("2027-08-13"));
        assert_eq!(i.created.as_deref(), Some("1995-08-14"));
        assert_eq!(i.registrar.as_deref(), Some("RESERVED-IANA"));
        assert_eq!(i.nameservers, vec!["a.iana-servers.net".to_string()]);
        t.push(503, "");
        assert!(rdap_lookup(&t, "https://rdap.test", "x.com").await.is_err());
    }
}
