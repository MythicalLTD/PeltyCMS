//! Default nameservers and DNS template for new domain orders (WHMCS
//! "Default Nameservers" and DNS templates).
use crate::domains::{normalize_ns, validate_dns, DnsRecord};
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const KEY: &str = "domains.defaults";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DomainDefaults {
    /// Used when the customer leaves nameservers empty (0 or 2..=13 hosts).
    pub nameservers: Vec<String>,
    /// Records added to new domains ordered with DNS management.
    /// `{domain}` in a value is replaced with the domain name.
    #[schema(value_type = Vec<Object>)]
    pub dns_template: Vec<DnsRecord>,
}

impl DomainDefaults {
    pub fn normalized(self) -> AppResult<Self> {
        let nameservers = normalize_ns(&self.nameservers)?;
        if self.dns_template.len() > 50 {
            return Err(AppError::Validation("error.too_many_records"));
        }
        // Validate with a sample name so `{domain}` placeholders pass.
        let dns_template = self
            .dns_template
            .iter()
            .map(|r| {
                let mut v = validate_dns(&DnsRecord {
                    value: r.value.replace("{domain}", "example.com"),
                    ..r.clone()
                })?;
                v.value = r.value.trim().to_string();
                v.id = uuid::Uuid::nil();
                Ok(v)
            })
            .collect::<AppResult<_>>()?;
        Ok(Self {
            nameservers,
            dns_template,
        })
    }

    /// The template for one domain, placeholders filled in.
    pub fn records_for(&self, domain: &str) -> Vec<DnsRecord> {
        self.dns_template
            .iter()
            .map(|r| DnsRecord {
                value: r.value.replace("{domain}", domain),
                ..r.clone()
            })
            .collect()
    }
}

pub async fn get(pool: &PgPool) -> AppResult<DomainDefaults> {
    Ok(crate::settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put(pool: &PgPool, d: DomainDefaults) -> AppResult<DomainDefaults> {
    let d = d.normalized()?;
    crate::settings::set(
        pool,
        KEY,
        serde_json::to_value(&d).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await?;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(t: &str, h: &str, v: &str) -> DnsRecord {
        DnsRecord {
            id: uuid::Uuid::nil(),
            rtype: t.into(),
            host: h.into(),
            value: v.into(),
            ttl: 3600,
            priority: None,
        }
    }
    #[test]
    fn template_placeholders() {
        let d = DomainDefaults {
            nameservers: vec!["NS1.Host.test".into(), "ns2.host.test".into()],
            dns_template: vec![
                rec("cname", "www", "{domain}"),
                rec("MX", "@", "mail.{domain}"),
            ],
        }
        .normalized()
        .unwrap();
        assert_eq!(d.nameservers, vec!["ns1.host.test", "ns2.host.test"]);
        let r = d.records_for("shop.ro");
        assert_eq!(
            (r[0].rtype.as_str(), r[0].value.as_str()),
            ("CNAME", "shop.ro")
        );
        assert_eq!(
            (r[1].value.as_str(), r[1].priority),
            ("mail.shop.ro", Some(10))
        );
        assert!(DomainDefaults {
            nameservers: vec!["ns1.only.test".into()],
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(DomainDefaults {
            dns_template: vec![rec("A", "@", "{domain}")],
            ..Default::default()
        }
        .normalized()
        .is_err());
    }
}
