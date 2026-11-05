//! Namecheap registrar.
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
#[allow(unused_imports)]
use md5::{Digest, Md5};
use serde_json::Value;
use std::sync::Arc;

pub const ID: &str = "namecheap";
pub const NAME: &str = "Namecheap";

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![f("api_user", "text", true), f("api_key", "text", true), f("username", "text", true), f("client_ip", "text", true)];
    v.push(f("test_mode", "bool", false));
    v.push(f("api_base", "text", false));
    v
}

pub struct Namecheap(Conn);

impl std::ops::Deref for Namecheap {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl Namecheap {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        Namecheap(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        Namecheap(Conn::new(NAME, transport, settings))
    }
}

// Namecheap
impl Namecheap {
    fn nc_url(&self, command: &str, extra: &[(String, String)]) -> String {
        let mut p: Vec<(String, String)> = vec![
            ("ApiUser".into(), self.s("api_user")),
            ("ApiKey".into(), self.s("api_key")),
            ("UserName".into(), self.s("username")),
            ("ClientIp".into(), self.s("client_ip")),
            ("Command".into(), command.to_string()),
        ];
        p.extend(extra.iter().cloned());
        format!("{}?{}", self.base("https://api.namecheap.com/xml.response", "https://api.sandbox.namecheap.com/xml.response"), qs(&p))
    }
    async fn nc(&self, command: &str, extra: &[(&str, String)]) -> RegistrarResult<String> {
        let owned: Vec<(String, String)> = extra.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        self.nc_owned(command, &owned).await
    }
    async fn nc_owned(&self, command: &str, extra: &[(String, String)]) -> RegistrarResult<String> {
        self.need(&["api_user", "api_key", "username", "client_ip"])?;
        let (st, body) = self.send(RegRequest::get(self.nc_url(command, extra))).await?;
        if !ok2xx(st) || xml_attr(&body, "ApiResponse", "Status").as_deref() != Some("OK") {
            return Err(remote(xml_text(&body, "Error").unwrap_or_else(|| format!("HTTP {st}"))));
        }
        Ok(body)
    }
    fn nc_contacts(c: &Contact) -> Vec<(String, String)> {
        let (first, last) = names(c);
        let mut v = vec![];
        for role in ["Registrant", "Tech", "Admin", "AuxBilling"] {
            let k = |s: &str| format!("{role}{s}");
            v.push((k("FirstName"), first.clone()));
            v.push((k("LastName"), last.clone()));
            v.push((k("OrganizationName"), c.organization.clone()));
            v.push((k("Address1"), c.address.clone()));
            v.push((k("City"), c.city.clone()));
            v.push((k("StateProvince"), c.city.clone()));
            v.push((k("PostalCode"), c.postal_code.clone()));
            v.push((k("Country"), c.country.clone()));
            v.push((k("Phone"), c.phone.clone()));
            v.push((k("EmailAddress"), c.email.clone()));
        }
        v
    }
}

impl RegistrarModule for Namecheap {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(Namecheap::with_settings(self.transport.clone(), settings)))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            {
                    let b = self.nc("namecheap.domains.check", &[("DomainList", domain.into())]).await?;
                    Ok(xml_attr(&b, "DomainCheckResult", "Available").is_some_and(|v| v.eq_ignore_ascii_case("true")))
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
                    let mut p: Vec<(String, String)> = vec![("DomainName".into(), domain.into()), ("Years".into(), years.to_string())];
                    if !nameservers.is_empty() {
                        p.push(("Nameservers".into(), nameservers.join(",")));
                    }
                    p.extend(Self::nc_contacts(registrant));
                    // Custom domain fields = Namecheap extended attributes (e.g. RegistrantNexus, CIRALegalType).
                    let taken: Vec<String> = p.iter().map(|(k, _)| k.clone()).collect();
                    let mut reserved: Vec<&str> = taken.iter().map(String::as_str).collect();
                    reserved.extend(["ApiUser", "ApiKey", "UserName", "ClientIp", "Command"]);
                    p.extend(pelty_provisioning::registrar::field_params(fields, &reserved));
                    let b = self.nc_owned("namecheap.domains.create", &p).await?;
                    if xml_attr(&b, "DomainCreateResult", "Registered").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
                        Ok(())
                    } else {
                        Err(RegistrarError::Remote("registration not confirmed".into()))
                    }
                }
        })
    }

    fn renew<'a>(&'a self, domain: &'a str, years: u32, _expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.nc("namecheap.domains.renew", &[("DomainName", domain.into()), ("Years", years.to_string())]).await.map(|_| ())
        })
    }

    fn transfer<'a>(&'a self, domain: &'a str, auth_code: &'a str, expires_on: &'a str) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            self.nc("namecheap.domains.transfer.create", &[("DomainName", domain.into()), ("Years", "1".into()), ("EPPCode", auth_code.into())]).await.map(|_| ())
        })
    }

    fn set_nameservers<'a>(&'a self, domain: &'a str, nameservers: &'a [String]) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                    let (sld, tld) = split(domain);
                    self.nc("namecheap.domains.dns.setCustom", &[("SLD", sld), ("TLD", tld), ("Nameservers", nameservers.join(","))]).await.map(|_| ())
                }
        })
    }

    fn set_contact<'a>(&'a self, domain: &'a str, c: &'a Contact) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                    let mut p: Vec<(String, String)> = vec![("DomainName".into(), domain.into())];
                    p.extend(Self::nc_contacts(c));
                    self.nc_owned("namecheap.domains.setContacts", &p).await.map(|_| ())
                }
        })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let bad = || RegistrarError::Remote("registrar returned no expiry date".into());
            {
                    let b = self.nc("namecheap.domains.getInfo", &[("DomainName", domain.into())]).await?;
                    let exp = xml_text(&b, "ExpiredDate").and_then(|s| us_date(&s)).ok_or_else(bad)?;
                    let st = xml_attr(&b, "DomainGetInfoResult", "Status").unwrap_or_default();
                    Ok(RemoteDomain { expires_on: exp, status: norm_status(&st), nameservers: xml_all(&b, "Nameserver") })
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

    fn reg(settings: Value) -> (Namecheap, MockRegTransport) {
        let t = MockRegTransport::default();
        (Namecheap::with_settings(Arc::new(t.clone()), &settings), t)
    }
    #[allow(dead_code)]
    fn contact() -> Contact {
        Contact { name: "Ada Lovelace".into(), email: "ada@example.com".into(), phone: "+40.700000000".into(), address: "Str 1".into(), city: "Cluj".into(), postal_code: "400000".into(), country: "RO".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn unconfigured_refuses_without_network() {
        let (r, t) = reg(json!({}));
        assert!(matches!(r.available("a.com").await, Err(RegistrarError::Remote(m)) if m.contains("not configured")));
        assert!(t.requests().is_empty());
    }
    #[tokio::test]
    async fn namecheap_flow() {
        let (r, t) = reg(json!({"api_user": "u", "api_key": "k", "username": "u", "client_ip": "1.2.3.4", "test_mode": true}));
        t.push(200, r#"<ApiResponse Status="OK"><CommandResponse><DomainCheckResult Domain="pelty.com" Available="true" /></CommandResponse></ApiResponse>"#);
        assert!(r.available("pelty.com").await.unwrap());
        let q = &t.requests()[0];
        assert!(q.url.starts_with("https://api.sandbox.namecheap.com/xml.response?"));
        assert_eq!(q.query("Command").as_deref(), Some("namecheap.domains.check"));
        assert_eq!(q.query("DomainList").as_deref(), Some("pelty.com"));
        t.push(200, r#"<ApiResponse Status="OK"><DomainCreateResult Domain="pelty.com" Registered="true" ChargedAmount="9.0"/></ApiResponse>"#);
        r.register("pelty.com", 2, "2028-01-01", &contact(), &["ns1.a.io".into(), "ns2.a.io".into()]).await.unwrap();
        let q = &t.requests()[1];
        assert_eq!(q.query("Years").as_deref(), Some("2"));
        assert_eq!(q.query("Nameservers").as_deref(), Some("ns1.a.io,ns2.a.io"));
        assert_eq!(q.query("RegistrantFirstName").as_deref(), Some("Ada"));
        assert_eq!(q.query("AuxBillingEmailAddress").as_deref(), Some("ada@example.com"));
        t.push(200, r#"<ApiResponse Status="OK"><DomainGetInfoResult Status="Ok"><DomainDetails><ExpiredDate>03/07/2028</ExpiredDate></DomainDetails><DnsDetails><Nameserver>dns1.x</Nameserver><Nameserver>dns2.x</Nameserver></DnsDetails></DomainGetInfoResult></ApiResponse>"#);
        let i = r.info("pelty.com").await.unwrap();
        assert_eq!(i, RemoteDomain { expires_on: "2028-03-07".into(), status: "active".into(), nameservers: vec!["dns1.x".into(), "dns2.x".into()] });
        t.push(200, r#"<ApiResponse Status="ERROR"><Errors><Error Number="2030280">Domain is not available</Error></Errors></ApiResponse>"#);
        assert!(matches!(r.register("taken.com", 1, "", &contact(), &[]).await, Err(RegistrarError::Unavailable)));
        assert_eq!(r.epp_code("pelty.com").await.unwrap(), None);
    }
}
