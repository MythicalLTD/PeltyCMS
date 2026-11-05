//! OpenSRS registrar.
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
#[allow(unused_imports)]
use md5::{Digest, Md5};
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub const ID: &str = "opensrs";
pub const NAME: &str = "OpenSRS";

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![f("username", "text", true), f("api_key", "text", true), f("reg_username", "text", false), f("reg_password", "text", false)];
    v.push(f("test_mode", "bool", false));
    v.push(f("api_base", "text", false));
    v
}

pub struct OpenSrs(Conn);

impl std::ops::Deref for OpenSrs {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl OpenSrs {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        OpenSrs(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        OpenSrs(Conn::new(NAME, transport, settings))
    }
}

// OpenSRS
/// OpenSRS XCP signature: md5(md5(xml + key) + key), lowercase hex.
pub fn opensrs_signature(xml: &str, key: &str) -> String {
    let h = |s: &str| Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    h(&format!("{}{key}", h(&format!("{xml}{key}"))))
}

fn xcp_value(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let items: String = m.iter().map(|(k, v)| format!("<item key=\"{}\">{}</item>", xml_escape(k), xcp_value(v))).collect();
            format!("<dt_assoc>{items}</dt_assoc>")
        }
        Value::Array(a) => {
            let items: String = a.iter().enumerate().map(|(i, v)| format!("<item key=\"{i}\">{}</item>", xcp_value(v))).collect();
            format!("<dt_array>{items}</dt_array>")
        }
        Value::String(s) => xml_escape(s),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Value of `<item key="k">` (first, scalar content).
fn xcp_item(xml: &str, key: &str) -> Option<String> {
    let k = format!("<item key=\"{key}\">");
    let i = xml.find(&k)? + k.len();
    let rest = &xml[i..];
    let j = rest.find('<')?;
    Some(xml_unescape(rest[..j].trim()))
}

impl OpenSrs {
    async fn osrs(&self, action: &str, object: &str, attrs: Value, top: &[(&str, &str)]) -> RegistrarResult<String> {
        self.need(&["username", "api_key"])?;
        let mut body = Map::new();
        body.insert("protocol".into(), json!("XCP"));
        body.insert("action".into(), json!(action));
        body.insert("object".into(), json!(object));
        for (k, v) in top {
            body.insert((*k).into(), json!(v));
        }
        body.insert("attributes".into(), attrs);
        let xml = format!(
            "<?xml version='1.0' encoding='UTF-8' standalone='no' ?><!DOCTYPE OPS_envelope SYSTEM 'ops.dtd'><OPS_envelope><header><version>0.9</version></header><body><data_block>{}</data_block></body></OPS_envelope>",
            xcp_value(&Value::Object(body))
        );
        let sig = opensrs_signature(&xml, &self.s("api_key"));
        let req = RegRequest {
            method: "POST",
            url: self.base("https://rr-n1-tor.opensrs.net:55443", "https://horizon.opensrs.net:55443"),
            headers: vec![("Content-Type".into(), "text/xml".into()), ("X-Username".into(), self.s("username")), ("X-Signature".into(), sig)],
            body: xml,
        };
        let (st, resp) = self.send(req).await?;
        if !ok2xx(st) || xcp_item(&resp, "is_success").as_deref() != Some("1") {
            return Err(remote(xcp_item(&resp, "response_text").unwrap_or_else(|| format!("HTTP {st}"))));
        }
        Ok(resp)
    }
    fn osrs_contact(c: &Contact) -> Value {
        let (first, last) = names(c);
        json!({"first_name": first, "last_name": last, "org_name": if c.organization.is_empty() { c.name.clone() } else { c.organization.clone() },
            "address1": c.address, "city": c.city, "state": c.city, "postal_code": c.postal_code, "country": c.country, "phone": c.phone, "email": c.email})
    }
    fn osrs_creds(&self) -> (String, String) {
        let u = self.s("reg_username");
        let p = self.s("reg_password");
        (if u.is_empty() { "pelty".into() } else { u }, if p.is_empty() { self.s("api_key").chars().take(16).collect() } else { p })
    }
}

impl RegistrarModule for OpenSrs {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(OpenSrs::with_settings(self.transport.clone(), settings)))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            {
                    let b = self.osrs("LOOKUP", "DOMAIN", json!({"domain": domain}), &[]).await?;
                    Ok(xcp_item(&b, "status").as_deref() == Some("available"))
                }
        })
    }

    fn register<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        self.register_with_fields(domain, years, expires_on, registrant, nameservers, &serde_json::Value::Null)
    }

    fn register_with_fields<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str, registrant: &'a Contact, nameservers: &'a [String], fields: &'a serde_json::Value) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            {
                    let (u, pw) = self.osrs_creds();
                    let c = Self::osrs_contact(registrant);
                    let ns: Vec<Value> = nameservers.iter().enumerate().map(|(i, n)| json!({"sortorder": (i + 1).to_string(), "name": n})).collect();
                    let attrs = json!({"domain": domain, "period": years.to_string(), "reg_type": "new", "reg_username": u, "reg_password": pw,
                        "handle": "process", "custom_tech_contact": "0", "custom_nameservers": if ns.is_empty() { "0" } else { "1" },
                        "contact_set": {"owner": c, "admin": c, "billing": c, "tech": c}, "nameserver_list": ns});
                    let mut attrs = attrs;
                    let tld = pelty_provisioning::registrar::field_params(fields, &[]);
                    if !tld.is_empty() {
                        // Custom domain fields go to OpenSRS `tld_data`.
                        attrs["tld_data"] = Value::Object(tld.into_iter().map(|(k, v)| (k, Value::String(v))).collect());
                    }
                    self.osrs("SW_REGISTER", "DOMAIN", attrs, &[]).await.map(|_| ())
                }
        })
    }

    fn renew<'a>(&'a self, domain: &'a str, years: u32, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                    // OpenSRS wants the *current* expiry year: new expiry minus the renewal term.
                    let new_year: i32 = expires_on.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0);
                    let cur = (new_year - years as i32).to_string();
                    self.osrs("RENEW", "DOMAIN", json!({"domain": domain, "period": years.to_string(), "currentexpirationyear": cur, "handle": "process", "auto_renew": "0"}), &[]).await.map(|_| ())
                }
        })
    }

    fn transfer<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            {
                    let (u, pw) = self.osrs_creds();
                    self.osrs("SW_REGISTER", "DOMAIN", json!({"domain": domain, "period": "1", "reg_type": "transfer", "auth_info": auth_code, "reg_username": u, "reg_password": pw, "handle": "process"}), &[]).await.map(|_| ())
                }
        })
    }

    fn set_nameservers<'a>(&'a self, domain: &'a str, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.osrs("ADVANCED_UPDATE_NAMESERVERS", "DOMAIN", json!({"domain": domain, "op_type": "assign", "assign_ns": nameservers}), &[]).await.map(|_| ())
        })
    }

    fn set_contact<'a>(&'a self, domain: &'a str, c: &'a Contact) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.osrs("UPDATE_CONTACTS", "DOMAIN", json!({"domain": domain, "types": ["owner"], "contact_set": {"owner": Self::osrs_contact(c)}}), &[]).await.map(|_| ())
        })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let bad = || RegistrarError::Remote("registrar returned no expiry date".into());
            {
                    let b = self.osrs("GET", "DOMAIN", json!({"type": "all_info"}), &[("domain", domain)]).await?;
                    let exp = xcp_item(&b, "registry_expiredate").and_then(|s| iso_date(&s)).ok_or_else(bad)?;
                    let ns: Vec<String> = b.match_indices("<item key=\"name\">").filter_map(|(i, _)| xcp_item(&b[i..], "name")).collect();
                    Ok(RemoteDomain { expires_on: exp, status: "active".into(), nameservers: ns })
                }
        })
    }

    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            {
                    let b = self.osrs("GET", "DOMAIN", json!({"type": "domain_auth_info"}), &[("domain", domain)]).await?;
                    Ok(xcp_item(&b, "domain_auth_info"))
                }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_provisioning::registrars::MockRegTransport;
    #[allow(unused_imports)]
    use serde_json::json;

    fn reg(settings: Value) -> (OpenSrs, MockRegTransport) {
        let t = MockRegTransport::default();
        (OpenSrs::with_settings(Arc::new(t.clone()), &settings), t)
    }
    #[allow(dead_code)]
    fn contact() -> Contact {
        Contact { name: "Ada Lovelace".into(), email: "ada@example.com".into(), phone: "+40.700000000".into(), address: "Str 1".into(), city: "Cluj".into(), postal_code: "400000".into(), country: "RO".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn opensrs_signs_and_parses() {
        assert_eq!(opensrs_signature("", ""), "74be16979710d4c4e7c6647856088456");
        let (r, t) = reg(json!({"username": "res", "api_key": "KEY", "test_mode": true}));
        t.push(200, r#"<OPS_envelope><body><data_block><dt_assoc><item key="is_success">1</item><item key="response_code">210</item><item key="attributes"><dt_assoc><item key="status">available</item></dt_assoc></item></dt_assoc></data_block></body></OPS_envelope>"#);
        assert!(r.available("pelty.org").await.unwrap());
        let q = &t.requests()[0];
        assert_eq!(q.url, "https://horizon.opensrs.net:55443");
        assert_eq!(q.header("X-Signature").map(str::to_string), Some(opensrs_signature(&q.body, "KEY")));
        assert!(q.body.contains("<item key=\"action\">LOOKUP</item>") && q.body.contains("<item key=\"domain\">pelty.org</item>"));
        t.push(200, r#"<dt_assoc><item key="is_success">1</item><item key="attributes"><dt_assoc><item key="registry_expiredate">2030-02-03 12:00:00</item><item key="nameserver_list"><dt_array><item key="0"><dt_assoc><item key="name">ns1.o.com</item></dt_assoc></item></dt_array></item></dt_assoc></item></dt_assoc>"#);
        let i = r.info("pelty.org").await.unwrap();
        assert_eq!((i.expires_on.as_str(), i.nameservers.len()), ("2030-02-03", 1));
        t.push(200, r#"<item key="is_success">1</item>"#);
        r.renew("pelty.org", 2, "2032-02-03").await.unwrap();
        assert!(t.requests()[2].body.contains("<item key=\"currentexpirationyear\">2030</item>"));
        t.push(200, r#"<item key="is_success">0</item><item key="response_text">Invalid domain authorization code</item>"#);
        assert!(matches!(r.transfer("x.org", "no", "").await, Err(RegistrarError::BadAuthCode)));
    }
}
