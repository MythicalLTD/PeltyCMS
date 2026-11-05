//! In-memory registrar for tests and demos.
use pelty_provisioning::registrar::{
    Contact, PremiumQuote, RegistrarError, RegistrarModule, RegistrarResult, RemoteDomain, TldCost,
};
use pelty_provisioning::BoxFuture;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct MockEntry {
    expires_on: String,
    nameservers: Vec<String>,
    contact: Contact,
    status: String,
}

/// In-memory registrar: names starting with `taken` are unavailable and the
/// transfer auth code `BAD` is rejected. `calls` records every operation.
#[derive(Default)]
pub struct MockRegistrar {
    domains: Mutex<HashMap<String, MockEntry>>,
    pub calls: Mutex<Vec<String>>,
}

impl MockRegistrar {
    fn log(&self, s: String) {
        if let Ok(mut c) = self.calls.lock() {
            c.push(s);
        }
    }
    fn with<T>(
        &self,
        f: impl FnOnce(&mut HashMap<String, MockEntry>) -> RegistrarResult<T>,
    ) -> RegistrarResult<T> {
        let mut g = self
            .domains
            .lock()
            .map_err(|_| RegistrarError::Remote("poisoned".into()))?;
        f(&mut g)
    }
    /// Domain table as JSON, so a stateless host (a WASM addon instance is
    /// fresh per call) can persist it between calls.
    pub fn snapshot(&self) -> String {
        self.with(|m| Ok(serde_json::to_string(m).unwrap_or_else(|_| "{}".into())))
            .unwrap_or_else(|_| "{}".into())
    }
    /// Rebuild from [`Self::snapshot`]; unreadable input starts empty.
    pub fn from_snapshot(s: &str) -> Self {
        MockRegistrar {
            domains: Mutex::new(serde_json::from_str(s).unwrap_or_default()),
            calls: Mutex::new(Vec::new()),
        }
    }
    /// Test hook: make the remote report a different expiry/status.
    pub fn set_remote(&self, domain: &str, expires_on: &str, status: &str) {
        let _ = self.with(|m| {
            if let Some(e) = m.get_mut(domain) {
                e.expires_on = expires_on.into();
                e.status = status.into();
            }
            Ok(())
        });
    }
}

impl RegistrarModule for MockRegistrar {
    fn id(&self) -> &'static str {
        "mock"
    }
    fn name(&self) -> &'static str {
        "Mock registrar"
    }
    /// Fixed EUR cost list (com, net, org) for price-sync tests.
    fn tld_prices<'a>(&'a self) -> BoxFuture<'a, RegistrarResult<Option<Vec<TldCost>>>> {
        let c = |t: &str, r: &str, n: &str, x: &str| TldCost {
            tld: t.into(),
            currency: "EUR".into(),
            register: r.into(),
            renew: n.into(),
            transfer: x.into(),
        };
        Box::pin(async move {
            Ok(Some(vec![
                c("com", "8.50", "9.10", "8.50"),
                c("net", "10.00", "10.00", "9.00"),
                c("org", "7.25", "7.25", "7.25"),
            ]))
        })
    }
    /// Names whose label starts with `premium` are premium (EUR 250 / 30).
    fn premium_price<'a>(
        &'a self,
        domain: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<Option<PremiumQuote>>> {
        let p = domain.starts_with("premium").then(|| PremiumQuote {
            currency: "EUR".into(),
            register: "250.00".into(),
            renew: "30.00".into(),
        });
        Box::pin(async move { Ok(p) })
    }
    fn available<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<bool>> {
        let r = self.with(|m| Ok(!domain.starts_with("taken") && !m.contains_key(domain)));
        Box::pin(async move { r })
    }
    fn register<'a>(
        &'a self,
        domain: &'a str,
        years: u32,
        expires_on: &'a str,
        registrant: &'a Contact,
        nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.log(format!("register {domain} {years}"));
        let r = self.with(|m| {
            if domain.starts_with("taken") || m.contains_key(domain) {
                return Err(RegistrarError::Unavailable);
            }
            m.insert(
                domain.into(),
                MockEntry {
                    expires_on: expires_on.into(),
                    nameservers: nameservers.to_vec(),
                    contact: registrant.clone(),
                    status: "active".into(),
                },
            );
            Ok(())
        });
        Box::pin(async move { r })
    }
    fn renew<'a>(
        &'a self,
        domain: &'a str,
        years: u32,
        expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.log(format!("renew {domain} {years}"));
        let r = self.with(|m| {
            let e = m.get_mut(domain).ok_or(RegistrarError::UnknownDomain)?;
            e.expires_on = expires_on.into();
            e.status = "active".into();
            Ok(())
        });
        Box::pin(async move { r })
    }
    fn transfer<'a>(
        &'a self,
        domain: &'a str,
        auth_code: &'a str,
        expires_on: &'a str,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.log(format!("transfer {domain}"));
        let r = self.with(|m| {
            if auth_code == "BAD" || auth_code.is_empty() {
                return Err(RegistrarError::BadAuthCode);
            }
            m.insert(
                domain.into(),
                MockEntry {
                    expires_on: expires_on.into(),
                    nameservers: Vec::new(),
                    contact: Contact::default(),
                    status: "active".into(),
                },
            );
            Ok(())
        });
        Box::pin(async move { r })
    }
    fn set_nameservers<'a>(
        &'a self,
        domain: &'a str,
        nameservers: &'a [String],
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.log(format!("ns {domain}"));
        let r = self.with(|m| {
            m.get_mut(domain)
                .ok_or(RegistrarError::UnknownDomain)?
                .nameservers = nameservers.to_vec();
            Ok(())
        });
        Box::pin(async move { r })
    }
    fn set_contact<'a>(
        &'a self,
        domain: &'a str,
        registrant: &'a Contact,
    ) -> BoxFuture<'a, RegistrarResult<()>> {
        self.log(format!("contact {domain}"));
        let r = self.with(|m| {
            m.get_mut(domain)
                .ok_or(RegistrarError::UnknownDomain)?
                .contact = registrant.clone();
            Ok(())
        });
        Box::pin(async move { r })
    }
    fn epp_code<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<Option<String>>> {
        Box::pin(async move {
            self.log(format!("epp {domain}"));
            self.with(|m| {
                if !m.contains_key(domain) {
                    return Err(RegistrarError::UnknownDomain);
                }
                // Deterministic, non-secret demo code.
                let sum: u32 = domain.bytes().map(u32::from).sum();
                Ok(Some(format!("MOCK-{sum:06}")))
            })
        })
    }
    fn info<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, RegistrarResult<RemoteDomain>> {
        let r = self.with(|m| {
            let e = m.get(domain).ok_or(RegistrarError::UnknownDomain)?;
            Ok(RemoteDomain {
                expires_on: e.expires_on.clone(),
                status: e.status.clone(),
                nameservers: e.nameservers.clone(),
            })
        });
        Box::pin(async move { r })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mock_registrar_lifecycle() {
        let m = MockRegistrar::default();
        assert!(m.available("example.com").await.unwrap());
        assert!(!m.available("taken.com").await.unwrap());
        let c = Contact {
            name: "A".into(),
            email: "a@x.io".into(),
            ..Default::default()
        };
        m.register("example.com", 1, "2027-01-01", &c, &["ns1.x".into()])
            .await
            .unwrap();
        assert!(matches!(
            m.register("example.com", 1, "2027-01-01", &c, &[]).await,
            Err(RegistrarError::Unavailable)
        ));
        m.renew("example.com", 2, "2029-01-01").await.unwrap();
        assert_eq!(
            m.info("example.com").await.unwrap().expires_on,
            "2029-01-01"
        );
        assert!(matches!(
            m.transfer("other.com", "BAD", "2027-01-01").await,
            Err(RegistrarError::BadAuthCode)
        ));
        assert!(matches!(
            m.renew("nope.com", 1, "2027-01-01").await,
            Err(RegistrarError::UnknownDomain)
        ));
    }
}
