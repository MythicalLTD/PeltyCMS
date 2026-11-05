//! Enom registrar.
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
#[allow(unused_imports)]
use md5::{Digest, Md5};
use serde_json::Value;
use std::sync::Arc;

pub const ID: &str = "enom";
pub const NAME: &str = "Enom";

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![f("uid", "text", true), f("password", "text", true)];
    v.push(f("test_mode", "bool", false));
    v.push(f("api_base", "text", false));
    v
}

pub struct Enom(Conn);

impl std::ops::Deref for Enom {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl Enom {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        Enom(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        Enom(Conn::new(NAME, transport, settings))
    }
}

// Enom
impl Enom {
    async fn enom(&self, command: &str, domain: &str, extra: &[(&str, String)]) -> RegistrarResult<String> {
        self.need(&["uid", "password"])?;
        let (sld, tld) = split(domain);
        let mut p: Vec<(&str, String)> = vec![("uid", self.s("uid")), ("pw", self.s("password")), ("command", command.into()), ("responsetype", "xml".into())];
        if !domain.is_empty() {
            p.push(("sld", sld));
            p.push(("tld", tld));
        }
        p.extend(extra.iter().cloned());
        let url = format!("{}?{}", self.base("https://reseller.enom.com/interface.asp", "https://resellertest.enom.com/interface.asp"), qs(&p));
        let (st, body) = self.send(RegRequest::get(url)).await?;
        let errs = xml_text(&body, "ErrCount").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
        if !ok2xx(st) || errs > 0 {
            return Err(remote(xml_text(&body, "Err1").unwrap_or_else(|| format!("HTTP {st}"))));
        }
        Ok(body)
    }
}

impl RegistrarModule for Enom {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(Enom::with_settings(self.transport.clone(), settings)))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            {
                    let b = self.enom("check", domain, &[]).await?;
                    Ok(xml_text(&b, "RRPCode").as_deref() == Some("210"))
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
                    let (first, last) = names(registrant);
                    let mut p: Vec<(&str, String)> = vec![("NumYears", years.to_string())];
                    let ns_keys = ["NS1", "NS2", "NS3", "NS4", "NS5"];
                    p.extend(nameservers.iter().zip(ns_keys).map(|(n, k)| (k, n.clone())));
                    if nameservers.is_empty() {
                        p.push(("UseDNS", "default".into()));
                    }
                    p.extend([
                        ("RegistrantFirstName", first),
                        ("RegistrantLastName", last),
                        ("RegistrantOrganizationName", registrant.organization.clone()),
                        ("RegistrantAddress1", registrant.address.clone()),
                        ("RegistrantCity", registrant.city.clone()),
                        ("RegistrantPostalCode", registrant.postal_code.clone()),
                        ("RegistrantCountry", registrant.country.clone()),
                        ("RegistrantPhone", registrant.phone.clone()),
                        ("RegistrantEmailAddress", registrant.email.clone()),
                    ]);
                    // Custom domain fields = eNom extended attributes (e.g. us_nexus, us_purpose).
                    let taken: Vec<String> = p.iter().map(|(k, _)| k.to_string()).collect();
                    let mut reserved: Vec<&str> = taken.iter().map(String::as_str).collect();
                    reserved.extend(["uid", "pw", "command", "responsetype", "sld", "tld"]);
                    let extra = pelty_provisioning::registrar::field_params(fields, &reserved);
                    p.extend(extra.iter().map(|(k, v)| (k.as_str(), v.clone())));
                    let b = self.enom("Purchase", domain, &p).await?;
                    match xml_text(&b, "RRPCode").as_deref() {
                        Some("200") => Ok(()),
                        _ => Err(remote(xml_text(&b, "RRPText").unwrap_or_else(|| "registration not confirmed".into()))),
                    }
                }
        })
    }

    fn renew<'a>(&'a self, domain: &'a str, years: u32, _expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.enom("Extend", domain, &[("NumYears", years.to_string())]).await.map(|_| ())
        })
    }

    fn transfer<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            {
                    let (sld, tld) = split(domain);
                    self.enom("TP_CreateOrder", "", &[("SLD1", sld), ("TLD1", tld), ("AuthInfo1", auth_code.into()), ("DomainCount", "1".into()), ("OrderType", "Autoverification".into())]).await.map(|_| ())
                }
        })
    }

    fn set_nameservers<'a>(&'a self, domain: &'a str, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                    let ks = ["NS1", "NS2", "NS3", "NS4", "NS5"];
                    let p: Vec<(&str, String)> = nameservers.iter().zip(ks).map(|(n, k)| (k, n.clone())).collect();
                    self.enom("ModifyNS", domain, &p).await.map(|_| ())
                }
        })
    }

    fn set_contact<'a>(&'a self, domain: &'a str, c: &'a Contact) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                    let (first, last) = names(c);
                    self.enom("Contacts", domain, &[("ContactType", "REGISTRANT".into()), ("RegistrantFirstName", first), ("RegistrantLastName", last), ("RegistrantEmailAddress", c.email.clone()), ("RegistrantPhone", c.phone.clone()), ("RegistrantAddress1", c.address.clone()), ("RegistrantCity", c.city.clone()), ("RegistrantPostalCode", c.postal_code.clone()), ("RegistrantCountry", c.country.clone())]).await.map(|_| ())
                }
        })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let bad = || RegistrarError::Remote("registrar returned no expiry date".into());
            {
                    let b = self.enom("GetDomainInfo", domain, &[]).await?;
                    let exp = xml_text(&b, "expiration").and_then(|s| us_date(&s)).ok_or_else(bad)?;
                    let st = xml_text(&b, "registrationstatus").unwrap_or_default();
                    let ns = xml_text(&b, "services").map(|s| xml_all(&s, "dns")).unwrap_or_default();
                    Ok(RemoteDomain { expires_on: exp, status: if st.eq_ignore_ascii_case("registered") { "active".into() } else { norm_status(&st) }, nameservers: ns })
                }
        })
    }

    fn epp_code<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            Ok(None)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_provisioning::registrars::MockRegTransport;
    #[allow(unused_imports)]
    use serde_json::json;

    fn reg(settings: Value) -> (Enom, MockRegTransport) {
        let t = MockRegTransport::default();
        (Enom::with_settings(Arc::new(t.clone()), &settings), t)
    }
    #[allow(dead_code)]
    fn contact() -> Contact {
        Contact { name: "Ada Lovelace".into(), email: "ada@example.com".into(), phone: "+40.700000000".into(), address: "Str 1".into(), city: "Cluj".into(), postal_code: "400000".into(), country: "RO".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn enom_flow() {
        let (r, t) = reg(json!({"uid": "u", "password": "p"}));
        t.push(200, "<interface-response><RRPCode>210</RRPCode><ErrCount>0</ErrCount></interface-response>");
        assert!(r.available("pelty.net").await.unwrap());
        let q = &t.requests()[0];
        assert_eq!((q.query("sld").as_deref(), q.query("tld").as_deref(), q.query("command").as_deref()), (Some("pelty"), Some("net"), Some("check")));
        t.push(200, "<interface-response><RRPCode>211</RRPCode><ErrCount>0</ErrCount></interface-response>");
        assert!(!r.available("google.net").await.unwrap());
        t.push(200, "<interface-response><RRPCode>200</RRPCode><ErrCount>0</ErrCount></interface-response>");
        r.register("pelty.net", 1, "", &contact(), &["ns1.x".into()]).await.unwrap();
        assert_eq!(t.requests()[2].query("NS1").as_deref(), Some("ns1.x"));
        t.push(200, "<interface-response><GetDomainInfo><status><expiration>1/5/2029 9:00:00 AM</expiration><registrationstatus>Registered</registrationstatus></status><services><entry name=\"dnsserver\"><configuration><dns>ns1.e.com</dns><dns>ns2.e.com</dns></configuration></entry></services></GetDomainInfo><ErrCount>0</ErrCount></interface-response>");
        let i = r.info("pelty.net").await.unwrap();
        assert_eq!(i.expires_on, "2029-01-05");
        assert_eq!(i.nameservers.len(), 2);
        t.push(200, "<interface-response><ErrCount>1</ErrCount><errors><Err1>Domain name not found</Err1></errors></interface-response>");
        assert!(matches!(r.renew("zz.net", 1, "").await, Err(RegistrarError::UnknownDomain)));
    }
}
