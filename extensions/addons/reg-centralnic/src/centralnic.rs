//! CentralNic Reseller registrar.
#[allow(unused_imports)]
use md5::{Digest, Md5};
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{
    Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain,
};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub const ID: &str = "centralnic";
pub const NAME: &str = "CentralNic Reseller";

static NO_FIELDS: Value = Value::Null;

/// Custom domain fields as RRPproxy extension parameters: `X-CA-LEGALTYPE`
/// passes through as typed; any other key becomes `X-<KEY>` upper-cased with
/// `_`/`.` turned into `-` (RRPproxy's convention). Core command parameters
/// and credentials can't be set this way.
pub fn crn_ext_params(fields: &Value) -> Vec<(String, String)> {
    pelty_provisioning::registrar::field_params(fields, &[])
        .into_iter()
        .map(|(k, v)| {
            let k = if k.len() > 2 && k[..2].eq_ignore_ascii_case("x-") {
                k
            } else {
                format!("X-{}", k.to_ascii_uppercase().replace(['_', '.'], "-"))
            };
            (k, v)
        })
        .collect()
}

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![
        f("login", "text", true),
        f("password", "text", true),
        f("contact_handle", "text", true),
    ];
    v.push(f("test_mode", "bool", false));
    v.push(f("api_base", "text", false));
    v
}

pub struct CentralNic(Conn);

impl std::ops::Deref for CentralNic {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl CentralNic {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        CentralNic(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        CentralNic(Conn::new(NAME, transport, settings))
    }
}

// CentralNic
/// Parse an RRPproxy/CentralNic Reseller plain-text response.
pub fn crn_parse(body: &str) -> (u32, String, Map<String, Value>) {
    let mut code = 0;
    let mut desc = String::new();
    let mut props: Map<String, Value> = Map::new();
    for line in body.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim().to_string());
        if k.eq_ignore_ascii_case("code") {
            code = v.parse().unwrap_or(0);
        } else if k.eq_ignore_ascii_case("description") {
            desc = v;
        } else if let Some(rest) = k.strip_prefix("property[") {
            let name = rest.split(']').next().unwrap_or("").to_ascii_lowercase();
            if let Value::Array(a) = props.entry(name).or_insert_with(|| json!([])) {
                a.push(Value::String(v));
            }
        }
    }
    (code, desc, props)
}

impl CentralNic {
    async fn crn(
        &self,
        command: &str,
        extra: &[(&str, String)],
    ) -> RegistrarResult<(u32, Map<String, Value>)> {
        self.need(&["login", "password"])?;
        let mut p: Vec<(&str, String)> = vec![
            ("s_login", self.s("login")),
            ("s_pw", self.s("password")),
            ("command", command.into()),
        ];
        p.extend(extra.iter().cloned());
        let url = format!(
            "{}?{}",
            self.base(
                "https://api.rrpproxy.net/api/call.cgi",
                "https://api-ote.rrpproxy.net/api/call.cgi"
            ),
            qs(&p)
        );
        let (st, body) = self.send(RegRequest::get(url)).await?;
        let (code, desc, props) = crn_parse(&body);
        if !ok2xx(st) || !(200..300).contains(&code) {
            return Err(remote(if desc.is_empty() {
                format!("HTTP {st}")
            } else {
                format!("{code} {desc}")
            }));
        }
        Ok((code, props))
    }
}

fn first(props: &Map<String, Value>, k: &str) -> Option<String> {
    props
        .get(k)?
        .as_array()?
        .first()?
        .as_str()
        .map(str::to_string)
}

impl RegistrarModule for CentralNic {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(CentralNic::with_settings(
            self.transport.clone(),
            settings,
        )))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            match self.crn("CheckDomain", &[("domain", domain.into())]).await {
                Ok((code, _)) => Ok(code == 210),
                Err(RegistrarError::Remote(m)) if m.starts_with("211") => Ok(false),
                Err(RegistrarError::Unavailable) => Ok(false),
                Err(e) => Err(e),
            }
        })
    }

    fn register<'a>(
        &'a self,
        domain: &'a str,
        years: u32,
        expires_on: &'a str,
        registrant: &'a Contact,
        nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.register_with_fields(
            domain,
            years,
            expires_on,
            registrant,
            nameservers,
            &NO_FIELDS,
        )
    }

    fn register_with_fields<'a>(
        &'a self,
        domain: &'a str,
        years: u32,
        expires_on: &'a str,
        _registrant: &'a Contact,
        nameservers: &'a [String],
        fields: &'a Value,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            {
                self.need(&["contact_handle"])?;
                let h = self.s("contact_handle");
                let mut p: Vec<(&str, String)> = vec![
                    ("domain", domain.into()),
                    ("period", years.to_string()),
                    ("ownercontact0", h.clone()),
                    ("admincontact0", h.clone()),
                    ("techcontact0", h.clone()),
                    ("billingcontact0", h),
                ];
                let ns_keys = [
                    "nameserver0",
                    "nameserver1",
                    "nameserver2",
                    "nameserver3",
                    "nameserver4",
                ];
                p.extend(nameservers.iter().zip(ns_keys).map(|(n, k)| (k, n.clone())));
                let ext = crn_ext_params(fields);
                p.extend(ext.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.crn("AddDomain", &p).await.map(|_| ())
            }
        })
    }

    fn renew<'a>(
        &'a self,
        domain: &'a str,
        years: u32,
        expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                let new_year: i32 = expires_on
                    .get(..4)
                    .and_then(|y| y.parse().ok())
                    .unwrap_or(0);
                self.crn(
                    "RenewDomain",
                    &[
                        ("domain", domain.into()),
                        ("period", years.to_string()),
                        ("expiration", (new_year - years as i32).to_string()),
                    ],
                )
                .await
                .map(|_| ())
            }
        })
    }

    fn transfer<'a>(
        &'a self,
        domain: &'a str,
        auth_code: &'a str,
        expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.transfer_with_fields(domain, auth_code, expires_on, &NO_FIELDS)
    }

    fn transfer_with_fields<'a>(
        &'a self,
        domain: &'a str,
        auth_code: &'a str,
        expires_on: &'a str,
        fields: &'a Value,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            let mut p: Vec<(&str, String)> = vec![
                ("domain", domain.into()),
                ("auth", auth_code.into()),
                ("action", "request".into()),
            ];
            let ext = crn_ext_params(fields);
            p.extend(ext.iter().map(|(k, v)| (k.as_str(), v.clone())));
            self.crn("TransferDomain", &p).await.map(|_| ())
        })
    }

    fn set_nameservers<'a>(
        &'a self,
        domain: &'a str,
        nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                let ks = [
                    "nameserver0",
                    "nameserver1",
                    "nameserver2",
                    "nameserver3",
                    "nameserver4",
                ];
                let mut p: Vec<(&str, String)> = vec![("domain", domain.into())];
                p.extend(nameservers.iter().zip(ks).map(|(n, k)| (k, n.clone())));
                self.crn("ModifyDomain", &p).await.map(|_| ())
            }
        })
    }

    fn set_contact<'a>(
        &'a self,
        _domain: &'a str,
        _c: &'a Contact,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            Err(RegistrarError::Remote(format!(
                "{} manages contacts by handle; edit them in the registrar panel",
                NAME
            )))
        })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let bad = || RegistrarError::Remote("registrar returned no expiry date".into());
            {
                let (_, p) = self
                    .crn("StatusDomain", &[("domain", domain.into())])
                    .await?;
                let exp = first(&p, "registrationexpirationdate")
                    .and_then(|s| iso_date(&s))
                    .ok_or_else(bad)?;
                let ns = p
                    .get("nameserver")
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(RemoteDomain {
                    expires_on: exp,
                    status: norm_status(&first(&p, "status").unwrap_or_default()),
                    nameservers: ns,
                })
            }
        })
    }

    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            {
                let (_, p) = self
                    .crn("StatusDomain", &[("domain", domain.into())])
                    .await?;
                Ok(first(&p, "auth"))
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

    fn reg(settings: Value) -> (CentralNic, MockRegTransport) {
        let t = MockRegTransport::default();
        (CentralNic::with_settings(Arc::new(t.clone()), &settings), t)
    }
    #[allow(dead_code)]
    fn contact() -> Contact {
        Contact {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: "+40.700000000".into(),
            address: "Str 1".into(),
            city: "Cluj".into(),
            postal_code: "400000".into(),
            country: "RO".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn centralnic_flow() {
        let (r, t) = reg(json!({"login": "l", "password": "p", "contact_handle": "P-ABC1"}));
        t.push(
            200,
            "[RESPONSE]\ncode = 210\ndescription = Domain name available\nEOF\n",
        );
        assert!(r.available("pelty.xyz").await.unwrap());
        t.push(
            200,
            "[RESPONSE]\ncode = 211\ndescription = Domain name not available\nEOF\n",
        );
        assert!(!r.available("google.xyz").await.unwrap());
        t.push(
            200,
            "[RESPONSE]\ncode = 200\ndescription = Command completed successfully\nEOF\n",
        );
        r.register(
            "pelty.xyz",
            1,
            "",
            &contact(),
            &["ns1.c".into(), "ns2.c".into()],
        )
        .await
        .unwrap();
        let q = &t.requests()[2];
        assert_eq!(
            (
                q.query("command").as_deref(),
                q.query("ownercontact0").as_deref(),
                q.query("nameserver1").as_deref()
            ),
            (Some("AddDomain"), Some("P-ABC1"), Some("ns2.c"))
        );
        t.push(200, "[RESPONSE]\ncode = 200\ndescription = OK\nproperty[registrationexpirationdate][0] = 2028-09-09 10:00:00\nproperty[nameserver][0] = ns1.c\nproperty[status][0] = ACTIVE\nproperty[auth][0] = Xy-9\nEOF\n");
        let i = r.info("pelty.xyz").await.unwrap();
        assert_eq!(
            (
                i.expires_on.as_str(),
                i.nameservers.len(),
                i.status.as_str()
            ),
            ("2028-09-09", 1, "active")
        );
        t.push(
            200,
            "[RESPONSE]\ncode = 200\nproperty[auth][0] = Xy-9\nEOF\n",
        );
        assert_eq!(
            r.epp_code("pelty.xyz").await.unwrap().as_deref(),
            Some("Xy-9")
        );
    }
}
