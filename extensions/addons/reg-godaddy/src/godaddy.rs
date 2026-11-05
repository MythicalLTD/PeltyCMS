//! GoDaddy registrar.
#[allow(unused_imports)]
use md5::{Digest, Md5};
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{
    Contact, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain,
};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
use serde_json::{json, Value};
use std::sync::Arc;

pub const ID: &str = "godaddy";
pub const NAME: &str = "GoDaddy";

static NO_FIELDS: Value = Value::Null;

/// Top-level members the module sets itself; a field can't replace them.
const GD_RESERVED: &[&str] = &[
    "domain",
    "period",
    "nameServers",
    "renewAuto",
    "privacy",
    "consent",
    "authCode",
];

/// Set `path` inside `v`, creating objects; never replaces a non-object on
/// the way, nor a whole object (e.g. a contact) at the end.
fn set_path(v: &mut Value, path: &[&str], val: Value) {
    let Some(o) = v.as_object_mut() else { return };
    match path {
        [last] => {
            if !o.get(*last).is_some_and(Value::is_object) {
                o.insert(last.to_string(), val);
            }
        }
        [head, rest @ ..] => set_path(
            o.entry(head.to_string()).or_insert_with(|| json!({})),
            rest,
            val,
        ),
        [] => {}
    }
}

/// Merge custom domain fields into a GoDaddy purchase/transfer body, keeping
/// the key's case (GoDaddy members are camelCase). `a.b` nests into an object
/// (`contactRegistrant.nexusCategory`); booleans and numbers stay JSON-typed.
pub fn gd_merge_fields(body: &mut Value, fields: &Value) {
    for (k, s) in pelty_provisioning::registrar::field_params(fields, &[]) {
        let path: Vec<&str> = k.split('.').collect();
        if path.iter().any(|p| p.is_empty())
            || GD_RESERVED.iter().any(|r| r.eq_ignore_ascii_case(path[0]))
        {
            continue;
        }
        let v = match &fields[&k] {
            Value::Bool(b) => Value::Bool(*b),
            Value::Number(n) => Value::Number(n.clone()),
            _ => Value::String(s),
        };
        set_path(body, &path, v);
    }
}

pub fn settings() -> Vec<SettingField> {
    #[allow(unused_mut)]
    let mut v = vec![
        f("api_key", "text", true),
        f("api_secret", "text", true),
        f("customer_ip", "text", false),
    ];
    v.push(f("test_mode", "bool", false));
    v.push(f("api_base", "text", false));
    v
}

pub struct GoDaddy(Conn);

impl std::ops::Deref for GoDaddy {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl GoDaddy {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        GoDaddy(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        GoDaddy(Conn::new(NAME, transport, settings))
    }
}

// GoDaddy
impl GoDaddy {
    async fn gd(
        &self,
        method: &'static str,
        path: &str,
        body: Option<Value>,
    ) -> RegistrarResult<Value> {
        self.need(&["api_key", "api_secret"])?;
        let mut headers = vec![
            (
                "Authorization".to_string(),
                format!("sso-key {}:{}", self.s("api_key"), self.s("api_secret")),
            ),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        let req = RegRequest {
            method,
            url: format!(
                "{}{path}",
                self.base("https://api.godaddy.com", "https://api.ote-godaddy.com")
            ),
            headers,
            body: body.map(|b| b.to_string()).unwrap_or_default(),
        };
        let (st, resp) = self.send(req).await?;
        let v: Value = if resp.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&resp).unwrap_or(Value::Null)
        };
        if st == 404 {
            return Err(RegistrarError::UnknownDomain);
        }
        if !ok2xx(st) {
            return Err(remote(
                v["message"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or(format!("HTTP {st}")),
            ));
        }
        Ok(v)
    }
    fn gd_contact(c: &Contact) -> Value {
        let (first, last) = names(c);
        json!({"nameFirst": first, "nameLast": last, "organization": c.organization, "email": c.email, "phone": c.phone,
            "addressMailing": {"address1": c.address, "city": c.city, "state": c.city, "postalCode": c.postal_code, "country": c.country}})
    }
    fn gd_consent(&self) -> Value {
        let ip = self.s("customer_ip");
        json!({"agreementKeys": ["DNRA"], "agreedBy": if ip.is_empty() { "127.0.0.1".into() } else { ip }, "agreedAt": "1970-01-01T00:00:00Z"})
    }
}

impl RegistrarModule for GoDaddy {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(GoDaddy::with_settings(
            self.transport.clone(),
            settings,
        )))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            {
                let v = self
                    .gd(
                        "GET",
                        &format!("/v1/domains/available?domain={}", enc(domain)),
                        None,
                    )
                    .await?;
                Ok(v["available"].as_bool().unwrap_or(false))
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
        registrant: &'a Contact,
        nameservers: &'a [String],
        fields: &'a Value,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let _ = expires_on;
            {
                let c = Self::gd_contact(registrant);
                let mut body = json!({"domain": domain, "period": years, "nameServers": nameservers, "renewAuto": false, "privacy": false, "consent": self.gd_consent(),
                        "contactRegistrant": c, "contactAdmin": c, "contactTech": c, "contactBilling": c});
                gd_merge_fields(&mut body, fields);
                self.gd("POST", "/v1/domains/purchase", Some(body))
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
            self.gd(
                "POST",
                &format!("/v1/domains/{}/renew", enc(domain)),
                Some(json!({"period": years})),
            )
            .await
            .map(|_| ())
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
            let mut body = json!({"authCode": auth_code, "period": 1, "privacy": false, "renewAuto": false, "consent": self.gd_consent()});
            gd_merge_fields(&mut body, fields);
            self.gd(
                "POST",
                &format!("/v1/domains/{}/transfer", enc(domain)),
                Some(body),
            )
            .await
            .map(|_| ())
        })
    }

    fn set_nameservers<'a>(
        &'a self,
        domain: &'a str,
        nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.gd(
                "PATCH",
                &format!("/v1/domains/{}", enc(domain)),
                Some(json!({"nameServers": nameservers})),
            )
            .await
            .map(|_| ())
        })
    }

    fn set_contact<'a>(
        &'a self,
        domain: &'a str,
        c: &'a Contact,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            self.gd(
                "PATCH",
                &format!("/v1/domains/{}/contacts", enc(domain)),
                Some(json!({"contactRegistrant": Self::gd_contact(c)})),
            )
            .await
            .map(|_| ())
        })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let bad = || RegistrarError::Remote("registrar returned no expiry date".into());
            {
                let v = self
                    .gd("GET", &format!("/v1/domains/{}", enc(domain)), None)
                    .await?;
                let exp = v["expires"].as_str().and_then(iso_date).ok_or_else(bad)?;
                let ns = v["nameServers"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(RemoteDomain {
                    expires_on: exp,
                    status: norm_status(v["status"].as_str().unwrap_or("ACTIVE")),
                    nameservers: ns,
                })
            }
        })
    }

    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            {
                let v = self
                    .gd("GET", &format!("/v1/domains/{}", enc(domain)), None)
                    .await?;
                Ok(v["authCode"].as_str().map(str::to_string))
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

    fn reg(settings: Value) -> (GoDaddy, MockRegTransport) {
        let t = MockRegTransport::default();
        (GoDaddy::with_settings(Arc::new(t.clone()), &settings), t)
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
    async fn godaddy_flow() {
        let (r, t) = reg(json!({"api_key": "K", "api_secret": "S"}));
        t.push(200, r#"{"available":true,"domain":"pelty.io"}"#);
        assert!(r.available("pelty.io").await.unwrap());
        let q = &t.requests()[0];
        assert_eq!(q.header("Authorization"), Some("sso-key K:S"));
        assert_eq!(
            q.url,
            "https://api.godaddy.com/v1/domains/available?domain=pelty.io"
        );
        t.push(200, r#"{"orderId":1}"#);
        r.register("pelty.io", 1, "", &contact(), &["ns1.g".into()])
            .await
            .unwrap();
        let b: Value = serde_json::from_str(&t.requests()[1].body).unwrap();
        assert_eq!(b["contactRegistrant"]["nameLast"], "Lovelace");
        assert_eq!(b["consent"]["agreementKeys"][0], "DNRA");
        t.push(200, r#"{"expires":"2027-06-01T00:00:00.000Z","status":"ACTIVE","nameServers":["a","b"],"authCode":"GD-1"}"#);
        assert_eq!(r.info("pelty.io").await.unwrap().expires_on, "2027-06-01");
        t.push(200, r#"{"authCode":"GD-1"}"#);
        assert_eq!(
            r.epp_code("pelty.io").await.unwrap().as_deref(),
            Some("GD-1")
        );
        t.push(404, r#"{"code":"NOT_FOUND","message":"x"}"#);
        assert!(matches!(
            r.info("nope.io").await,
            Err(RegistrarError::UnknownDomain)
        ));
        t.push(204, "");
        r.set_nameservers("pelty.io", &["n1".into()]).await.unwrap();
        assert_eq!(t.requests()[5].method, "PATCH");
    }
}
