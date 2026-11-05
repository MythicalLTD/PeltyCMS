//! ResellerClub / LogicBoxes registrar.
#[allow(unused_imports)]
use md5::{Digest, Md5};
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{
    Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain,
};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

pub const ID: &str = "resellerclub";
pub const NAME: &str = "ResellerClub / LogicBoxes";

static NO_FIELDS: Value = Value::Null;

/// Custom domain fields as LogicBoxes TLD attributes: `attr-name1=<key>`,
/// `attr-value1=<value>`, … with the key's case kept (e.g. `cedcontactid`,
/// `purpose`, `category`).
pub fn rc_attr_params(fields: &Value) -> Vec<(String, String)> {
    let reserved = [
        "auth-userid",
        "api-key",
        "domain-name",
        "years",
        "ns",
        "customer-id",
        "reg-contact-id",
        "admin-contact-id",
        "tech-contact-id",
        "billing-contact-id",
        "invoice-option",
        "auth-code",
    ];
    pelty_provisioning::registrar::field_params(fields, &reserved)
        .into_iter()
        .enumerate()
        .flat_map(|(i, (k, v))| {
            [
                (format!("attr-name{}", i + 1), k),
                (format!("attr-value{}", i + 1), v),
            ]
        })
        .collect()
}

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![
        f("reseller_id", "text", true),
        f("api_key", "text", true),
        f("customer_id", "text", true),
        f("contact_id", "text", true),
    ];
    v.push(f("test_mode", "bool", false));
    v.push(f("api_base", "text", false));
    v
}

pub struct ResellerClub(Conn);

impl std::ops::Deref for ResellerClub {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl ResellerClub {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        ResellerClub(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        ResellerClub(Conn::new(NAME, transport, settings))
    }
}

// ResellerClub
impl ResellerClub {
    fn rc_base(&self) -> String {
        self.base("https://httpapi.com/api", "https://test.httpapi.com/api")
    }
    fn rc_auth(&self) -> Vec<(&'static str, String)> {
        vec![
            ("auth-userid", self.s("reseller_id")),
            ("api-key", self.s("api_key")),
        ]
    }
    async fn rc(
        &self,
        method: &'static str,
        path: &str,
        extra: &[(&str, String)],
    ) -> RegistrarResult<Value> {
        self.need(&["reseller_id", "api_key"])?;
        let mut p: Vec<(&str, String)> = self.rc_auth();
        p.extend(extra.iter().cloned());
        let q = qs(&p);
        let req = if method == "GET" {
            RegRequest::get(format!("{}{path}?{q}", self.rc_base()))
        } else {
            RegRequest {
                method,
                url: format!("{}{path}", self.rc_base()),
                headers: vec![(
                    "Content-Type".into(),
                    "application/x-www-form-urlencoded".into(),
                )],
                body: q,
            }
        };
        let (st, body) = self.send(req).await?;
        let v: Value =
            serde_json::from_str(body.trim()).unwrap_or(Value::String(body.trim().to_string()));
        let err = v["status"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("error"))
            || v["actionstatus"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("failed"));
        if !ok2xx(st) || err {
            let m = v["message"]
                .as_str()
                .or(v["error"].as_str())
                .or(v["actionstatusdesc"].as_str())
                .map(str::to_string)
                .unwrap_or(format!("HTTP {st}"));
            return Err(remote(m));
        }
        Ok(v)
    }
    async fn rc_order_id(&self, domain: &str) -> RegistrarResult<String> {
        let v = self
            .rc(
                "GET",
                "/domains/orderid.json",
                &[("domain-name", domain.to_string())],
            )
            .await?;
        match v {
            Value::Number(n) => Ok(n.to_string()),
            Value::String(s) if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() => Ok(s),
            _ => Err(RegistrarError::UnknownDomain),
        }
    }
    fn rc_contacts(&self) -> Vec<(&'static str, String)> {
        let c = self.s("contact_id");
        vec![
            ("customer-id", self.s("customer_id")),
            ("reg-contact-id", c.clone()),
            ("admin-contact-id", c.clone()),
            ("tech-contact-id", c.clone()),
            ("billing-contact-id", c),
            ("invoice-option", "NoInvoice".into()),
        ]
    }
}

impl RegistrarModule for ResellerClub {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(ResellerClub::with_settings(
            self.transport.clone(),
            settings,
        )))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            {
                let (sld, tld) = split(domain);
                let v = self
                    .rc(
                        "GET",
                        "/domains/available.json",
                        &[("domain-name", sld), ("tlds", tld)],
                    )
                    .await?;
                Ok(v[domain.to_ascii_lowercase()]["status"].as_str() == Some("available"))
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
                self.need(&["customer_id", "contact_id"])?;
                let mut p: Vec<(&str, String)> =
                    vec![("domain-name", domain.into()), ("years", years.to_string())];
                p.extend(nameservers.iter().map(|n| ("ns", n.clone())));
                p.extend(self.rc_contacts());
                let attrs = rc_attr_params(fields);
                p.extend(attrs.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.rc("POST", "/domains/register.json", &p)
                    .await
                    .map(|_| ())
            }
        })
    }

    fn renew<'a>(
        &'a self,
        domain: &'a str,
        years: u32,
        _expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                let id = self.rc_order_id(domain).await?;
                let det = self
                    .rc(
                        "GET",
                        "/domains/details.json",
                        &[("order-id", id.clone()), ("options", "OrderDetails".into())],
                    )
                    .await?;
                let exp = det["endtime"]
                    .as_str()
                    .map(str::to_string)
                    .or(det["endtime"].as_i64().map(|n| n.to_string()))
                    .unwrap_or_default();
                self.rc(
                    "POST",
                    "/domains/renew.json",
                    &[
                        ("order-id", id),
                        ("years", years.to_string()),
                        ("exp-date", exp),
                        ("invoice-option", "NoInvoice".into()),
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
            {
                self.need(&["customer_id", "contact_id"])?;
                let mut p: Vec<(&str, String)> = vec![
                    ("domain-name", domain.into()),
                    ("auth-code", auth_code.into()),
                ];
                p.extend(self.rc_contacts());
                let attrs = rc_attr_params(fields);
                p.extend(attrs.iter().map(|(k, v)| (k.as_str(), v.clone())));
                self.rc("POST", "/domains/transfer.json", &p)
                    .await
                    .map(|_| ())
            }
        })
    }

    fn set_nameservers<'a>(
        &'a self,
        domain: &'a str,
        nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            {
                let id = self.rc_order_id(domain).await?;
                let mut p: Vec<(&str, String)> = vec![("order-id", id)];
                p.extend(nameservers.iter().map(|n| ("ns", n.clone())));
                self.rc("POST", "/domains/modify-ns.json", &p)
                    .await
                    .map(|_| ())
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
                let v = self
                    .rc(
                        "GET",
                        "/domains/details-by-name.json",
                        &[("domain-name", domain.into()), ("options", "All".into())],
                    )
                    .await?;
                let secs = v["endtime"]
                    .as_str()
                    .and_then(|s| s.parse::<i64>().ok())
                    .or(v["endtime"].as_i64())
                    .ok_or_else(bad)?;
                let ns = (1..=13)
                    .filter_map(|i| v[format!("ns{i}")].as_str().map(str::to_string))
                    .collect();
                Ok(RemoteDomain {
                    expires_on: epoch_date(secs),
                    status: norm_status(v["currentstatus"].as_str().unwrap_or("Active")),
                    nameservers: ns,
                })
            }
        })
    }

    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            {
                let v = self
                    .rc(
                        "GET",
                        "/domains/details-by-name.json",
                        &[
                            ("domain-name", domain.into()),
                            ("options", "OrderDetails".into()),
                        ],
                    )
                    .await?;
                Ok(v["domsecret"].as_str().map(str::to_string))
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

    fn reg(settings: Value) -> (ResellerClub, MockRegTransport) {
        let t = MockRegTransport::default();
        (
            ResellerClub::with_settings(Arc::new(t.clone()), &settings),
            t,
        )
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
    async fn resellerclub_flow() {
        let (r, t) =
            reg(json!({"reseller_id": "1", "api_key": "k", "customer_id": "7", "contact_id": "9"}));
        t.push(
            200,
            r#"{"pelty.ro":{"classkey":"dotro","status":"available"}}"#,
        );
        assert!(r.available("pelty.ro").await.unwrap());
        assert_eq!(t.requests()[0].query("tlds").as_deref(), Some("ro"));
        assert!(t.requests()[0]
            .url
            .starts_with("https://httpapi.com/api/domains/available.json?"));
        t.push(
            200,
            r#"{"actiontypedesc":"Registration","actionstatus":"Success","entityid":"55"}"#,
        );
        r.register(
            "pelty.ro",
            1,
            "",
            &contact(),
            &["ns1.x".into(), "ns2.x".into()],
        )
        .await
        .unwrap();
        let b = &t.requests()[1].body;
        assert!(
            b.contains("ns=ns1.x&ns=ns2.x")
                && b.contains("reg-contact-id=9")
                && b.contains("customer-id=7")
        );
        // renew: order id lookup, details, renew
        t.push(200, "55")
            .push(200, r#"{"endtime":"1830297600"}"#)
            .push(200, r#"{"actionstatus":"Success"}"#);
        r.renew("pelty.ro", 1, "2028-01-01").await.unwrap();
        assert!(t.requests()[4].body.contains("exp-date=1830297600"));
        t.push(200, r#"{"endtime":"1830297600","currentstatus":"Active","ns1":"a.ns","ns2":"b.ns","domsecret":"S3cr3t"}"#);
        let i = r.info("pelty.ro").await.unwrap();
        assert_eq!(i.expires_on, "2028-01-01");
        assert_eq!(i.nameservers, vec!["a.ns".to_string(), "b.ns".into()]);
        t.push(200, r#"{"domsecret":"S3cr3t"}"#);
        assert_eq!(
            r.epp_code("pelty.ro").await.unwrap().as_deref(),
            Some("S3cr3t")
        );
        t.push(500, r#"{"status":"ERROR","message":"Invalid auth code"}"#);
        assert!(matches!(
            r.transfer("x.ro", "bad", "").await,
            Err(RegistrarError::BadAuthCode)
        ));
    }
}
