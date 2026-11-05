//! Generic JSON registrar registrar.
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
#[allow(unused_imports)]
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::sync::Arc;

pub const ID: &str = "generic";
pub const NAME: &str = "Generic JSON registrar";

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![f("api_base", "text", true), f("auth_header", "text", false), f("auth_value", "text", false), f("hmac_secret", "text", false)];
    v
}

pub struct Generic(Conn);

impl std::ops::Deref for Generic {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl Generic {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        Generic(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        Generic(Conn::new(NAME, transport, settings))
    }
}

// Generic
impl Generic {
    async fn gen(&self, method: &'static str, path: &str, body: Option<Value>) -> RegistrarResult<Value> {
        self.need(&["api_base"])?;
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if !body.is_empty() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        let (h, v) = (self.s("auth_header"), self.s("auth_value"));
        if !h.is_empty() {
            headers.push((h, v));
        }
        let secret = self.s("hmac_secret");
        if !secret.is_empty() {
            use hmac::{Hmac, Mac};
            let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).map_err(|e| RegistrarError::Remote(e.to_string()))?;
            mac.update(body.as_bytes());
            headers.push(("X-Pelty-Signature".into(), hex::encode(mac.finalize().into_bytes())));
        }
        let req = RegRequest { method, url: format!("{}{path}", self.s("api_base").trim_end_matches('/')), headers, body };
        let (st, resp) = self.send(req).await?;
        let v: Value = if resp.trim().is_empty() { Value::Null } else { serde_json::from_str(&resp).unwrap_or(Value::Null) };
        if st == 404 {
            return Err(RegistrarError::UnknownDomain);
        }
        if !ok2xx(st) {
            return Err(remote(v["error"].as_str().or(v["message"].as_str()).map(str::to_string).unwrap_or(format!("HTTP {st}"))));
        }
        Ok(v)
    }
}

impl RegistrarModule for Generic {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(Generic::with_settings(self.transport.clone(), settings)))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            {
                    let v = self.gen("GET", &format!("/domains/{}/availability", enc(domain)), None).await?;
                    Ok(v["available"].as_bool().unwrap_or(false))
                }
        })
    }

    fn register<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        self.register_with_fields(domain, years, expires_on, registrant, nameservers, &serde_json::Value::Null)
    }

    fn register_with_fields<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String], fields: &'a serde_json::Value) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            self.gen("POST", "/domains", Some(json!({"domain": domain, "years": years, "nameservers": nameservers, "registrant": registrant, "fields": if fields.is_object() { fields.clone() } else { json!({}) }}))).await.map(|_| ())
        })
    }

    fn renew<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.gen("POST", &format!("/domains/{}/renew", enc(domain)), Some(json!({"years": years, "expires_on": expires_on}))).await.map(|_| ())
        })
    }

    fn transfer<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        self.transfer_with_fields(domain, auth_code, expires_on, &serde_json::Value::Null)
    }

    fn transfer_with_fields<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str, fields: &'a serde_json::Value) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            self.gen("POST", "/domains/transfers", Some(json!({"domain": domain, "auth_code": auth_code, "fields": if fields.is_object() { fields.clone() } else { json!({}) }}))).await.map(|_| ())
        })
    }

    fn set_nameservers<'a>(&'a self, domain: &'a str, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.gen("PUT", &format!("/domains/{}/nameservers", enc(domain)), Some(json!({"nameservers": nameservers}))).await.map(|_| ())
        })
    }

    fn set_contact<'a>(&'a self, domain: &'a str, c: &'a Contact) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.gen("PUT", &format!("/domains/{}/contact", enc(domain)), Some(json!(c))).await.map(|_| ())
        })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let bad = || RegistrarError::Remote("registrar returned no expiry date".into());
            {
                    let v = self.gen("GET", &format!("/domains/{}", enc(domain)), None).await?;
                    let exp = v["expires_on"].as_str().and_then(iso_date).ok_or_else(bad)?;
                    let ns = v["nameservers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    Ok(RemoteDomain { expires_on: exp, status: norm_status(v["status"].as_str().unwrap_or("active")), nameservers: ns })
                }
        })
    }

    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            {
                    let v = self.gen("GET", &format!("/domains/{}/epp", enc(domain)), None).await?;
                    Ok(v["auth_code"].as_str().map(str::to_string))
                }
                // Namecheap and Enom e-mail the EPP code to the registrant instead.
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_provisioning::registrars::MockRegTransport;
    #[allow(unused_imports)]
    use serde_json::json;

    fn reg(settings: Value) -> (Generic, MockRegTransport) {
        let t = MockRegTransport::default();
        (Generic::with_settings(Arc::new(t.clone()), &settings), t)
    }
    #[allow(dead_code)]
    fn contact() -> Contact {
        Contact { name: "Ada Lovelace".into(), email: "ada@example.com".into(), phone: "+40.700000000".into(), address: "Str 1".into(), city: "Cluj".into(), postal_code: "400000".into(), country: "RO".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn generic_signs_body() {
        let (r, t) = reg(json!({"api_base": "https://reg.example/api/", "auth_header": "X-Key", "auth_value": "abc", "hmac_secret": "s"}));
        t.push(200, r#"{"available":false}"#);
        assert!(!r.available("a.dev").await.unwrap());
        assert_eq!(t.requests()[0].url, "https://reg.example/api/domains/a.dev/availability");
        assert_eq!(t.requests()[0].header("X-Key"), Some("abc"));
        t.push(201, r#"{"ok":true}"#);
        r.register("a.dev", 1, "", &contact(), &[]).await.unwrap();
        let q = &t.requests()[1];
        use hmac::{Hmac, Mac};
        let mut m = Hmac::<sha2::Sha256>::new_from_slice(b"s").unwrap();
        m.update(q.body.as_bytes());
        assert_eq!(q.header("X-Pelty-Signature").map(str::to_string), Some(hex::encode(m.finalize().into_bytes())));
        t.push(200, r#"{"expires_on":"2027-01-02","status":"active","nameservers":["x"]}"#);
        assert_eq!(r.info("a.dev").await.unwrap().expires_on, "2027-01-02");
    }
}
