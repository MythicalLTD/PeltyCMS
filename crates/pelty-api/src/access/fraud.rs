//! Order fraud screening: built-in rules plus pluggable [`FraudHook`]s
//! (e.g. a MaxMind or FraudLabs integration). Scores add up (capped at 100);
//! the settings thresholds pick accept / review / reject. A hook that errors
//! is logged and ignored (fail-open) so an outage never blocks sales.
use crate::trusted_proxy::RequestSignals;
use crate::AppState;
use ipnet::IpNet;
use pelty_core::AppResult;
use pelty_database::fraud::{self, Facts, FraudSettings};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct OrderContext {
    pub order_id: Uuid,
    pub customer_id: Uuid,
    pub ip: Option<String>,
    pub total: Decimal,
    pub currency: String,
    pub country: String,
    pub email: String,
    /// Geo country from a trusted edge proxy (`T1` = Tor).
    pub geo_country: Option<String>,
    pub proxy_headers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub rule: String,
    pub score: u8,
}

pub trait FraudHook: Send + Sync {
    fn name(&self) -> &'static str;
    fn check<'a>(&'a self, ctx: &'a OrderContext) -> BoxFuture<'a, Result<Vec<Hit>, String>>;
}

/// External fraud network (MaxMind / FraudLabs / custom) over HTTPS.
/// Configure via Admin → Security → Fraud (`http_hook_url` / `http_hook_key`).
pub struct HttpFraudHook {
    pub url: String,
    pub api_key: String,
}

impl FraudHook for HttpFraudHook {
    fn name(&self) -> &'static str {
        "http"
    }
    fn check<'a>(&'a self, ctx: &'a OrderContext) -> BoxFuture<'a, Result<Vec<Hit>, String>> {
        Box::pin(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.to_string())?;
            let body = json!({
                "order_id": ctx.order_id,
                "customer_id": ctx.customer_id,
                "ip": ctx.ip,
                "total": ctx.total.to_string(),
                "currency": ctx.currency,
                "country": ctx.country,
                "email": ctx.email,
                "geo_country": ctx.geo_country,
            })
            .to_string();
            let mut req = client
                .post(&self.url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
            if !self.api_key.is_empty() {
                req = req.bearer_auth(&self.api_key);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            let body = resp.text().await.map_err(|e| e.to_string())?;
            let v: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| format!("bad JSON: {e}"))?;
            if let Some(arr) = v.get("hits").and_then(|h| h.as_array()) {
                let mut out = Vec::new();
                for h in arr {
                    let rule = h
                        .get("rule")
                        .and_then(|r| r.as_str())
                        .unwrap_or("score")
                        .chars()
                        .take(64)
                        .collect::<String>();
                    let score = h
                        .get("score")
                        .and_then(|s| s.as_u64())
                        .unwrap_or(0)
                        .min(100) as u8;
                    if score > 0 {
                        out.push(Hit { rule, score });
                    }
                }
                return Ok(out);
            }
            let score = v
                .get("score")
                .and_then(|s| s.as_u64())
                .unwrap_or(0)
                .min(100) as u8;
            if score == 0 {
                return Ok(vec![]);
            }
            Ok(vec![Hit {
                rule: v
                    .get("rule")
                    .and_then(|r| r.as_str())
                    .unwrap_or("score")
                    .chars()
                    .take(64)
                    .collect(),
                score,
            }])
        })
    }
}

/// Replace the built-in HTTP fraud hook from current settings (call after save).
pub fn sync_http_hook(s: &AppState, cfg: &FraudSettings) {
    let Ok(mut hooks) = s.fraud_hooks.write() else {
        return;
    };
    hooks.retain(|h| h.name() != "http");
    let url = cfg.http_hook_url.trim();
    if !url.is_empty() {
        hooks.push(Arc::new(HttpFraudHook {
            url: url.to_string(),
            api_key: cfg.http_hook_key.clone(),
        }));
    }
}

fn hit(rule: &str, score: u8) -> Hit {
    Hit {
        rule: rule.into(),
        score,
    }
}

const BUILTIN_DISPOSABLE: &str = include_str!("disposable_domains.txt");

/// The domain part of an address, lowercased.
pub fn email_domain(email: &str) -> String {
    email
        .rsplit_once('@')
        .map(|(_, d)| d.trim().trim_end_matches('.').to_ascii_lowercase())
        .unwrap_or_default()
}

/// `domain` or one of its parents is a disposable provider (built-in list
/// plus the admin's extra domains).
pub fn is_disposable(domain: &str, extra: &[String]) -> bool {
    let listed = |d: &str| {
        BUILTIN_DISPOSABLE.lines().map(str::trim).any(|l| l == d) || extra.iter().any(|x| x == d)
    };
    let mut d = domain;
    loop {
        if d.contains('.') && listed(d) {
            return true;
        }
        match d.split_once('.') {
            Some((_, rest)) if rest.contains('.') => d = rest,
            _ => return false,
        }
    }
}

/// The first configured hosting/VPN range containing `ip`.
pub fn vpn_range(ranges: &[String], ip: Option<&str>) -> Option<String> {
    let ip: std::net::IpAddr = ip?.parse().ok()?;
    ranges
        .iter()
        .find(|r| crate::security::parse_cidr(r).is_some_and(|n: IpNet| n.contains(&ip)))
        .cloned()
}

/// Request-side inputs to the built-in rules.
#[derive(Debug, Clone)]
pub struct RuleInput<'a> {
    pub req: &'a RequestSignals,
    pub vpn_range: Option<&'a str>,
}

/// Built-in rules (pure).
pub fn rules(cfg: &FraudSettings, f: &Facts, total: Decimal, i: &RuleInput<'_>) -> Vec<Hit> {
    let mut out = Vec::new();
    if !f.country.is_empty() && cfg.blocked_countries.iter().any(|c| c == &f.country) {
        out.push(hit("blocked_country", 100));
    }
    if cfg.max_orders_per_ip_day > 0 && f.orders_from_ip_day >= i64::from(cfg.max_orders_per_ip_day)
    {
        out.push(hit("ip_velocity", 60));
    }
    if cfg.max_orders_per_customer_day > 0
        && f.orders_by_customer_day >= i64::from(cfg.max_orders_per_customer_day)
    {
        out.push(hit("customer_velocity", 50));
    }
    if cfg.max_first_order_total > Decimal::ZERO
        && f.prior_orders == 0
        && total > cfg.max_first_order_total
    {
        out.push(hit("large_first_order", 50));
    }
    if f.country.is_empty() {
        out.push(hit("no_billing_country", 10));
    }
    if cfg.disposable_email_score > 0
        && is_disposable(&email_domain(&f.email), &cfg.disposable_domains)
    {
        out.push(hit("disposable_email", cfg.disposable_email_score));
    }
    match i.req.geo_country.as_deref() {
        Some("T1") if cfg.tor_score > 0 => out.push(hit("tor", cfg.tor_score)),
        Some(g)
            if g != "T1"
                && !f.country.is_empty()
                && g != f.country
                && cfg.country_mismatch_score > 0 =>
        {
            out.push(hit("ip_country_mismatch", cfg.country_mismatch_score));
        }
        _ => {}
    }
    // The connection's country can be blocked even if the billing one isn't.
    if let Some(g) = i.req.geo_country.as_deref() {
        if g != f.country && cfg.blocked_countries.iter().any(|c| c == g) {
            out.push(hit("blocked_ip_country", 100));
        }
    }
    if cfg.proxy_score > 0 && !i.req.proxy_headers.is_empty() {
        out.push(hit("proxy_headers", cfg.proxy_score));
    }
    if cfg.vpn_score > 0 && i.vpn_range.is_some() {
        out.push(hit("vpn_ip", cfg.vpn_score));
    }
    if cfg.max_customers_per_ip > 0
        && cfg.shared_ip_score > 0
        && f.customers_from_ip_30d >= i64::from(cfg.max_customers_per_ip)
    {
        out.push(hit("shared_ip", cfg.shared_ip_score));
    }
    out
}

pub fn verdict(cfg: &FraudSettings, score: u8) -> &'static str {
    if score >= cfg.reject_score {
        "reject"
    } else if score >= cfg.review_score {
        "review"
    } else {
        "accept"
    }
}

pub fn total_score(hits: &[Hit]) -> u8 {
    hits.iter()
        .map(|h| u32::from(h.score))
        .sum::<u32>()
        .min(100) as u8
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub check_id: Uuid,
    pub score: u8,
    pub verdict: &'static str,
    pub hits: Vec<Hit>,
}

/// Screen a just-placed order. `None` when screening is disabled.
pub async fn screen(
    s: &AppState,
    order_id: Uuid,
    customer_id: Uuid,
    ip: Option<&str>,
    req: &RequestSignals,
    total: Decimal,
    currency: &str,
) -> AppResult<Option<Outcome>> {
    let cfg = fraud::get_settings(&s.pool).await?;
    if !cfg.enabled {
        return Ok(None);
    }
    let f = fraud::facts(&s.pool, customer_id, order_id, ip).await?;
    let vpn = vpn_range(&cfg.vpn_ranges, ip);
    let mut hits = rules(
        &cfg,
        &f,
        total,
        &RuleInput {
            req,
            vpn_range: vpn.as_deref(),
        },
    );
    let ctx = OrderContext {
        order_id,
        customer_id,
        ip: ip.map(str::to_string),
        total,
        currency: currency.to_string(),
        country: f.country.clone(),
        email: f.email.clone(),
        geo_country: req.geo_country.clone(),
        proxy_headers: req.proxy_headers.clone(),
    };
    let hooks: Vec<Arc<dyn FraudHook>> =
        s.fraud_hooks.read().map(|h| h.clone()).unwrap_or_default();
    for h in hooks {
        match h.check(&ctx).await {
            Ok(mut v) => {
                for x in &mut v {
                    x.rule = format!("{}:{}", h.name(), x.rule);
                    x.score = x.score.min(100);
                }
                hits.extend(v);
            }
            Err(e) => tracing::warn!(hook = h.name(), error = %e, "fraud hook failed; ignored"),
        }
    }
    let score = total_score(&hits);
    let v = verdict(&cfg, score);
    let signals = json!({
        "billing_country": f.country,
        "geo_country": req.geo_country,
        "proxy_headers": req.proxy_headers,
        "vpn_range": vpn,
        "email_domain": email_domain(&f.email),
        "orders_from_ip_day": f.orders_from_ip_day,
        "orders_by_customer_day": f.orders_by_customer_day,
        "customers_from_ip_30d": f.customers_from_ip_30d,
        "prior_orders": f.prior_orders,
    });
    let check_id = fraud::record(
        &s.pool,
        order_id,
        customer_id,
        ip,
        score,
        v,
        &json!(hits),
        &signals,
    )
    .await?;
    Ok(Some(Outcome {
        check_id,
        score,
        verdict: v,
        hits,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            country: "DE".into(),
            email: "a@example.com".into(),
            orders_from_ip_day: 0,
            orders_by_customer_day: 0,
            prior_orders: 3,
            customers_from_ip_30d: 0,
        }
    }

    fn none() -> RuleInput<'static> {
        static R: std::sync::OnceLock<RequestSignals> = std::sync::OnceLock::new();
        RuleInput {
            req: R.get_or_init(RequestSignals::default),
            vpn_range: None,
        }
    }

    #[test]
    fn rules_and_verdicts() {
        let cfg = FraudSettings {
            enabled: true,
            blocked_countries: vec!["XX".into()],
            max_first_order_total: Decimal::from(100),
            ..Default::default()
        };
        assert!(rules(&cfg, &facts(), Decimal::from(1000), &none()).is_empty());
        let f = Facts {
            country: "XX".into(),
            ..facts()
        };
        assert_eq!(
            rules(&cfg, &f, Decimal::ONE, &none()),
            vec![hit("blocked_country", 100)]
        );
        let f = Facts {
            prior_orders: 0,
            orders_from_ip_day: 5,
            ..facts()
        };
        let h = rules(&cfg, &f, Decimal::from(101), &none());
        assert_eq!(
            h,
            vec![hit("ip_velocity", 60), hit("large_first_order", 50)]
        );
        assert_eq!(total_score(&h), 100);
        assert_eq!(verdict(&cfg, 100), "reject");
        assert_eq!(verdict(&cfg, 60), "review");
        assert_eq!(verdict(&cfg, 49), "accept");
        let f = Facts {
            country: String::new(),
            ..facts()
        };
        assert_eq!(total_score(&rules(&cfg, &f, Decimal::ONE, &none())), 10);
    }

    #[test]
    fn heuristics() {
        let cfg = FraudSettings {
            enabled: true,
            blocked_countries: vec!["KP".into()],
            disposable_domains: vec!["burner.test".into()],
            ..Default::default()
        };
        let rule = |f: &Facts, req: &RequestSignals, vpn: Option<&str>| {
            rules(
                &cfg,
                f,
                Decimal::ONE,
                &RuleInput {
                    req,
                    vpn_range: vpn,
                },
            )
            .into_iter()
            .map(|h| (h.rule, h.score))
            .collect::<Vec<_>>()
        };
        let plain = RequestSignals::default();
        // Disposable e-mail: built-in, subdomain and admin-added.
        for e in ["x@mailinator.com", "x@eu.Mailinator.com", "x@burner.test"] {
            assert_eq!(
                rule(
                    &Facts {
                        email: e.into(),
                        ..facts()
                    },
                    &plain,
                    None
                ),
                vec![("disposable_email".to_string(), 40)],
                "{e}"
            );
        }
        assert!(rule(
            &Facts {
                email: "x@notmailinator.com".into(),
                ..facts()
            },
            &plain,
            None
        )
        .is_empty());
        assert!(!is_disposable("com", &[]));
        // Geo country vs billing country; Tor; blocked geo country.
        let geo = |c: &str| RequestSignals {
            geo_country: Some(c.into()),
            proxy_headers: vec![],
        };
        assert!(rule(&facts(), &geo("DE"), None).is_empty());
        assert_eq!(
            rule(&facts(), &geo("FR"), None),
            vec![("ip_country_mismatch".to_string(), 30)]
        );
        assert_eq!(
            rule(&facts(), &geo("T1"), None),
            vec![("tor".to_string(), 60)]
        );
        assert_eq!(
            rule(&facts(), &geo("KP"), None),
            vec![
                ("ip_country_mismatch".to_string(), 30),
                ("blocked_ip_country".to_string(), 100)
            ]
        );
        // Proxy headers, VPN range, shared IP.
        let px = RequestSignals {
            geo_country: None,
            proxy_headers: vec!["via".into()],
        };
        assert_eq!(
            rule(&facts(), &px, None),
            vec![("proxy_headers".to_string(), 25)]
        );
        assert_eq!(
            rule(&facts(), &plain, Some("198.51.100.0/24")),
            vec![("vpn_ip".to_string(), 40)]
        );
        assert_eq!(
            rule(
                &Facts {
                    customers_from_ip_30d: 3,
                    ..facts()
                },
                &plain,
                None
            ),
            vec![("shared_ip".to_string(), 30)]
        );
        assert!(rule(
            &Facts {
                customers_from_ip_30d: 2,
                ..facts()
            },
            &plain,
            None
        )
        .is_empty());
        // A zero score turns a rule off.
        let off = FraudSettings {
            proxy_score: 0,
            tor_score: 0,
            ..cfg.clone()
        };
        assert!(rules(
            &off,
            &facts(),
            Decimal::ONE,
            &RuleInput {
                req: &px,
                vpn_range: None
            }
        )
        .is_empty());
        assert!(rules(
            &off,
            &facts(),
            Decimal::ONE,
            &RuleInput {
                req: &geo("T1"),
                vpn_range: None
            }
        )
        .is_empty());
        assert_eq!(
            vpn_range(&["198.51.100.0/24".into()], Some("198.51.100.7")).as_deref(),
            Some("198.51.100.0/24")
        );
        assert_eq!(
            vpn_range(&["198.51.100.0/24".into()], Some("203.0.113.1")),
            None
        );
        assert_eq!(vpn_range(&["198.51.100.0/24".into()], None), None);
        assert_eq!(email_domain("A@Example.COM."), "example.com");
    }

    #[test]
    fn settings_validation() {
        assert!(FraudSettings {
            review_score: 80,
            reject_score: 50,
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(FraudSettings {
            blocked_countries: vec!["DEU".into()],
            ..Default::default()
        }
        .normalized()
        .is_err());
        let s = FraudSettings {
            blocked_countries: vec![" ru".into(), "RU".into(), "kp".into()],
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(s.blocked_countries, vec!["KP", "RU"]);
        assert!(FraudSettings {
            disposable_domains: vec!["no_dots".into()],
            ..Default::default()
        }
        .normalized()
        .is_err());
        assert!(FraudSettings {
            tor_score: 101,
            ..Default::default()
        }
        .normalized()
        .is_err());
        let s = FraudSettings {
            disposable_domains: vec![" @Burner.test".into(), "burner.test".into()],
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(s.disposable_domains, vec!["burner.test"]);
        // Old stored settings (no new keys) still load with defaults.
        let old: FraudSettings =
            serde_json::from_value(json!({"enabled": true, "review_score": 40})).unwrap();
        assert_eq!(
            (
                old.review_score,
                old.disposable_email_score,
                old.max_customers_per_ip
            ),
            (40, 40, 3)
        );
    }
}
