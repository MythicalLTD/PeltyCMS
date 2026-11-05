//! Cloudflare as a DNS host: create a zone on "register", push/pull DNS on
//! `replace_dns` / `get_dns`, toggle DNSSEC. Domain commerce (transfer, lock,
//! WHOIS privacy) is not supported — use a real registrar for those.
use pelty_addon_registrar_common::*;
use pelty_provisioning::registrar::{
    Contact, DnsZoneRecord, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain,
};
use pelty_provisioning::registrars::{RegRequest, RegTransport};
use pelty_provisioning::BoxFuture;
use serde_json::{json, Value};
use std::sync::Arc;

pub const ID: &str = "cloudflare";
pub const NAME: &str = "Cloudflare DNS";

pub fn settings() -> Vec<SettingField> {
    vec![
        f("api_token", "text", true),
        f("account_id", "text", false),
        f("api_base", "text", false),
    ]
}

pub struct Cloudflare(Conn);

impl std::ops::Deref for Cloudflare {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        &self.0
    }
}

impl Cloudflare {
    pub fn new(transport: Arc<dyn RegTransport>) -> Self {
        Cloudflare(Conn::new(NAME, transport, &Value::Null))
    }
    pub fn with_settings(transport: Arc<dyn RegTransport>, settings: &Value) -> Self {
        Cloudflare(Conn::new(NAME, transport, settings))
    }

    fn api_root(&self) -> String {
        Conn::base(
            self,
            "https://api.cloudflare.com/client/v4",
            "https://api.cloudflare.com/client/v4",
        )
    }

    async fn cf(&self, method: &'static str, path: &str, body: Option<Value>) -> RegistrarResult<Value> {
        self.need(&["api_token"])?;
        let body_s = body.map(|b| b.to_string()).unwrap_or_default();
        let mut headers = vec![
            ("Accept".into(), "application/json".into()),
            (
                "Authorization".into(),
                format!("Bearer {}", self.s("api_token")),
            ),
        ];
        if !body_s.is_empty() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        let (st, resp) = self
            .send(RegRequest {
                method,
                url: format!("{}{path}", self.api_root()),
                headers,
                body: body_s,
            })
            .await?;
        let v: Value = serde_json::from_str(&resp).unwrap_or(Value::Null);
        if !ok2xx(st) || v["success"] == false {
            let msg = v["errors"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|e| e["message"].as_str())
                .or(v["errors"].as_str())
                .unwrap_or("Cloudflare API error");
            return Err(RegistrarError::Remote(msg.chars().take(300).collect()));
        }
        Ok(v["result"].clone())
    }

    async fn zone_id(&self, domain: &str) -> RegistrarResult<String> {
        let v = self
            .cf(
                "GET",
                &format!("/zones?name={}&status=active", enc(domain)),
                None,
            )
            .await?;
        v.as_array()
            .and_then(|a| a.first())
            .and_then(|z| z["id"].as_str())
            .map(str::to_string)
            .ok_or(RegistrarError::UnknownDomain)
    }

    fn record_name(domain: &str, host: &str) -> String {
        let h = host.trim().trim_end_matches('.');
        if h.is_empty() || h == "@" {
            domain.to_string()
        } else if h.eq_ignore_ascii_case(domain) || h.ends_with(&format!(".{domain}")) {
            h.to_string()
        } else {
            format!("{h}.{domain}")
        }
    }
}

impl RegistrarModule for Cloudflare {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn configure(&self, settings: &Value) -> Option<Arc<dyn RegistrarModule>> {
        Some(Arc::new(Cloudflare::with_settings(
            self.transport.clone(),
            settings,
        )))
    }

    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        Box::pin(async move {
            match self.zone_id(domain).await {
                Ok(_) => Ok(false),
                Err(RegistrarError::UnknownDomain) => Ok(true),
                Err(e) => Err(e),
            }
        })
    }

    fn register<'a>(
        &'a self,
        domain: &'a str,
        _years: u32,
        _expires_on: &'a str,
        _registrant: &'a Contact,
        _nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let mut body = json!({ "name": domain, "jump_start": false, "type": "full" });
            let account = self.s("account_id");
            if !account.is_empty() {
                body["account"] = json!({ "id": account });
            }
            self.cf("POST", "/zones", Some(body)).await.map(|_| ())
        })
    }

    fn renew<'a>(
        &'a self,
        _domain: &'a str,
        _years: u32,
        _expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async { Err(RegistrarError::Unsupported) })
    }

    fn transfer<'a>(
        &'a self,
        _domain: &'a str,
        _auth_code: &'a str,
        _expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async { Err(RegistrarError::Unsupported) })
    }

    fn set_nameservers<'a>(
        &'a self,
        _domain: &'a str,
        _nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async { Err(RegistrarError::Unsupported) })
    }

    fn set_contact<'a>(
        &'a self,
        _domain: &'a str,
        _registrant: &'a Contact,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async { Err(RegistrarError::Unsupported) })
    }

    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        Box::pin(async move {
            let zid = self.zone_id(domain).await?;
            let z = self.cf("GET", &format!("/zones/{zid}"), None).await?;
            let ns = z["name_servers"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Ok(RemoteDomain {
                expires_on: "2099-01-01".into(),
                status: "active".into(),
                nameservers: ns,
            })
        })
    }

    fn replace_dns<'a>(
        &'a self,
        domain: &'a str,
        records: &'a [DnsZoneRecord],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let zid = self.zone_id(domain).await?;
            let existing = self
                .cf(
                    "GET",
                    &format!("/zones/{zid}/dns_records?per_page=500"),
                    None,
                )
                .await?;
            if let Some(arr) = existing.as_array() {
                for r in arr {
                    if let Some(id) = r["id"].as_str() {
                        let _ = self
                            .cf("DELETE", &format!("/zones/{zid}/dns_records/{id}"), None)
                            .await;
                    }
                }
            }
            for r in records {
                let mut body = json!({
                    "type": r.rtype,
                    "name": Self::record_name(domain, &r.host),
                    "content": r.value,
                    "ttl": r.ttl.max(1),
                });
                if let Some(pr) = r.priority {
                    body["priority"] = json!(pr);
                }
                self.cf("POST", &format!("/zones/{zid}/dns_records"), Some(body))
                    .await?;
            }
            Ok(())
        })
    }

    fn get_dns<'a>(
        &'a self,
        domain: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<Vec<DnsZoneRecord>>> {
        Box::pin(async move {
            let zid = self.zone_id(domain).await?;
            let existing = self
                .cf(
                    "GET",
                    &format!("/zones/{zid}/dns_records?per_page=500"),
                    None,
                )
                .await?;
            let mut out = vec![];
            for r in existing.as_array().into_iter().flatten() {
                let name = r["name"].as_str().unwrap_or_default();
                let host = if name.eq_ignore_ascii_case(domain) {
                    "@".into()
                } else {
                    name.trim_end_matches(&format!(".{domain}"))
                        .trim_end_matches('.')
                        .to_string()
                };
                out.push(DnsZoneRecord {
                    rtype: r["type"].as_str().unwrap_or("A").into(),
                    host,
                    value: r["content"].as_str().unwrap_or_default().into(),
                    ttl: r["ttl"].as_u64().unwrap_or(3600) as u32,
                    priority: r["priority"].as_u64().map(|p| p as u32),
                });
            }
            Ok(out)
        })
    }

    fn set_dnssec<'a>(
        &'a self,
        domain: &'a str,
        enabled: bool,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        Box::pin(async move {
            let zid = self.zone_id(domain).await?;
            let status = if enabled { "active" } else { "disabled" };
            self.cf(
                "PATCH",
                &format!("/zones/{zid}/dnssec"),
                Some(json!({ "status": status })),
            )
            .await
            .map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_provisioning::registrars::MockRegTransport;

    #[tokio::test]
    async fn pushes_zone_and_dns() {
        let t = MockRegTransport::default();
        t.push(
            200,
            r#"{"success":true,"result":[{"id":"zid1","name":"example.com"}]}"#,
        );
        t.push(200, r#"{"success":true,"result":[]}"#);
        t.push(200, r#"{"success":true,"result":{"id":"rec1"}}"#);
        let m = Cloudflare::with_settings(
            Arc::new(t),
            &json!({"api_token": "tok"}),
        );
        m.replace_dns(
            "example.com",
            &[DnsZoneRecord {
                rtype: "A".into(),
                host: "@".into(),
                value: "1.2.3.4".into(),
                ttl: 300,
                priority: None,
            }],
        )
        .await
        .unwrap();
    }
}
