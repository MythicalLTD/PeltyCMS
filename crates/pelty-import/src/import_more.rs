//! WHMCS servers, contacts, promotions and tax rules (same transaction and
//! `import_map` idempotency as the rest of [`crate::import`]).
use crate::import::{lookup, remember, ts, ImportError, Report, Sealer, Tx};
use crate::map;
use crate::whmcs::WhmcsData;
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

/// Module name accepted by `servers.module` (`^[a-z][a-z0-9_-]{1,63}$`).
fn module(t: &str) -> String {
    let m: String = t
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    if m.len() >= 2 && m.starts_with(|c: char| c.is_ascii_lowercase()) {
        m
    } else {
        "whmcs".into()
    }
}

/// `tblservers` -> `servers`. Returns WHMCS server id -> PeltyCMS id.
pub(crate) async fn servers(
    tx: &mut Tx<'_>,
    data: &WhmcsData,
    rep: &mut Report,
    seal: Option<Sealer<'_>>,
) -> Result<HashMap<i64, Uuid>, ImportError> {
    let mut out = HashMap::new();
    for s in &data.servers {
        let sid = s.id.to_string();
        if let Some(id) = lookup(tx, "server", &sid).await? {
            out.insert(s.id, id);
            rep.skipped("servers");
            continue;
        }
        let host = [s.hostname.trim(), s.ipaddress.trim()]
            .into_iter()
            .find(|h| !h.is_empty() && !h.contains(['/', ' ', '@']));
        let Some(host) = host else {
            rep.warnings.push(format!(
                "server {} ({}) has no hostname or IP, skipped",
                s.id, s.name
            ));
            continue;
        };
        let port = if s.port > 0 && s.port < 65536 {
            format!(":{}", s.port)
        } else {
            String::new()
        };
        let base_url = format!("{}://{host}{port}", if s.secure { "https" } else { "http" });
        // cPanel/Plesk-style API tokens live in accesshash; else the password.
        let (secret, auth) = if !s.accesshash.trim().is_empty() {
            (s.accesshash.trim().to_string(), "accesshash")
        } else if s.password_plain {
            (s.password.clone(), "password")
        } else {
            (String::new(), "none")
        };
        if !s.password.is_empty() && !s.password_plain && auth == "none" {
            rep.warnings.push(format!("server {} ({}): password still encrypted (pass --cc-hash with WHMCS' cc_encryption_hash); imported disabled", s.id, s.name));
        }
        let sealed = match (seal, secret.is_empty()) {
            (Some(f), false) => Some(f(secret.as_bytes())),
            _ => None,
        };
        if sealed.is_none() && auth != "none" {
            rep.warnings.push(format!("server {} ({}): no encryption key to store its credential; imported disabled, re-enter it in Admin > Servers", s.id, s.name));
        }
        let name = if s.name.trim().is_empty() {
            format!("WHMCS server {}", s.id)
        } else {
            map::clip(s.name.trim(), 120)
        };
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO servers (id, name, module, base_url, api_key_sealed, config, enabled, max_services) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(id).bind(name).bind(module(&s.type_)).bind(map::clip(&base_url, 500))
            .bind(sealed.clone().unwrap_or_default())
            .bind(serde_json::json!({ "username": map::clip(&s.username, 100), "auth": auth, "whmcs_type": s.type_, "whmcs_id": s.id }))
            .bind(!s.disabled && sealed.is_some())
            .bind((s.maxaccounts > 0).then_some(s.maxaccounts.min(i32::MAX as i64) as i32))
            .execute(&mut **tx).await?;
        remember(tx, "server", &sid, id).await?;
        rep.created("servers");
        out.insert(s.id, id);
    }
    Ok(out)
}

/// Promo code accepted by `coupons.code` (`^[A-Z0-9_-]{3,40}$`).
pub(crate) fn coupon_code(c: &str) -> Option<String> {
    let c: String = c
        .trim()
        .to_ascii_uppercase()
        .chars()
        .map(|x| {
            if x.is_ascii_alphanumeric() || x == '_' || x == '-' {
                x
            } else {
                '-'
            }
        })
        .collect();
    (3..=40).contains(&c.len()).then_some(c)
}

/// Contacts, promotions (coupons) and tax rules.
pub(crate) async fn rest(
    tx: &mut Tx<'_>,
    data: &WhmcsData,
    rep: &mut Report,
    default_cur: &str,
    customers: &HashMap<i64, (Uuid, String)>,
    products: &HashMap<i64, Uuid>,
) -> Result<(), ImportError> {
    // ---- contacts
    for c in &data.contacts {
        let sid = c.id.to_string();
        if lookup(tx, "contact", &sid).await?.is_some() {
            rep.skipped("contacts");
            continue;
        }
        let Some((cid, _)) = customers.get(&c.userid) else {
            rep.warnings.push(format!(
                "contact {} belongs to unknown client {}, skipped",
                c.id, c.userid
            ));
            continue;
        };
        let email = c.email.trim();
        if !(email.contains('@') && (3..=254).contains(&email.len())) {
            rep.warnings.push(format!(
                "contact {} has invalid email {:?}, skipped",
                c.id, c.email
            ));
            continue;
        }
        let full = format!("{} {}", c.firstname.trim(), c.lastname.trim());
        let name = if full.trim().is_empty() {
            email.to_string()
        } else {
            full.trim().to_string()
        };
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO customer_contacts (id, customer_id, name, email, phone, is_billing) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(id).bind(cid).bind(map::clip(&name, 200)).bind(email).bind(map::clip(&c.phonenumber, 40)).bind(c.billing)
            .execute(&mut **tx).await?;
        remember(tx, "contact", &sid, id).await?;
        rep.created("contacts");
    }

    // ---- promotions -> coupons
    for p in &data.promotions {
        let sid = p.id.to_string();
        if lookup(tx, "promotion", &sid).await?.is_some() {
            rep.skipped("coupons");
            continue;
        }
        let Some(code) = coupon_code(&p.code) else {
            rep.warnings.push(format!(
                "promotion {} code {:?} is not 3-40 characters, skipped",
                p.id, p.code
            ));
            continue;
        };
        let (kind, currency) = match p.type_.trim() {
            "Percentage" => ("percent", None),
            "Fixed Amount" => ("fixed", Some(default_cur.to_string())),
            other => {
                rep.warnings.push(format!(
                    "promotion {} ({code}) type {other:?} has no PeltyCMS equivalent, skipped",
                    p.id
                ));
                continue;
            }
        };
        if p.value <= Decimal::ZERO || (kind == "percent" && p.value > Decimal::ONE_HUNDRED) {
            rep.warnings.push(format!(
                "promotion {} ({code}) has value {}, skipped",
                p.id, p.value
            ));
            continue;
        }
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM coupons WHERE code=$1)")
            .bind(&code)
            .fetch_one(&mut **tx)
            .await?;
        if taken {
            rep.warnings.push(format!(
                "promotion {} code {code} already exists in PeltyCMS, skipped",
                p.id
            ));
            continue;
        }
        let max = (p.maxuses > 0).then_some(p.maxuses.min(i32::MAX as i64) as i32);
        let uses = p
            .uses
            .clamp(0, max.map(i64::from).unwrap_or(i32::MAX as i64)) as i32;
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO coupons (id, code, description, kind, value, currency, recurring, max_uses, uses, starts_at, expires_at, active) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,TRUE)")
            .bind(id).bind(&code).bind(map::clip(&p.notes, 500)).bind(kind).bind(p.value).bind(currency).bind(p.recurring)
            .bind(max).bind(uses).bind(ts(p.startdate)).bind(ts(p.expirationdate))
            .execute(&mut **tx).await?;
        for pid in p
            .appliesto
            .split(',')
            .filter_map(|x| x.trim().parse::<i64>().ok())
        {
            if let Some(prod) = products.get(&pid) {
                sqlx::query("INSERT INTO coupon_products (coupon_id, product_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
                    .bind(id).bind(prod).execute(&mut **tx).await?;
            }
        }
        remember(tx, "promotion", &sid, id).await?;
        rep.created("coupons");
    }

    // ---- tax rules
    for t in &data.tax_rules {
        let sid = t.id.to_string();
        if lookup(tx, "tax_rule", &sid).await?.is_some() {
            rep.skipped("tax_rules");
            continue;
        }
        let country = t.country.trim().to_ascii_uppercase();
        let country = if country.len() == 2 && country.chars().all(|c| c.is_ascii_uppercase()) {
            country
        } else {
            String::new()
        };
        let name = if t.name.trim().is_empty() {
            format!("Tax {}", t.level.max(1))
        } else {
            map::clip(t.name.trim(), 100)
        };
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO tax_rules (id, name, country, region, rate, priority) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(id).bind(name).bind(country).bind(map::clip(t.state.trim(), 100))
            .bind(t.taxrate.clamp(Decimal::ZERO, Decimal::ONE_HUNDRED)).bind(t.level)
            .execute(&mut **tx).await?;
        remember(tx, "tax_rule", &sid, id).await?;
        rep.created("tax_rules");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn names() {
        assert_eq!(super::module("cPanel"), "cpanel");
        assert_eq!(super::module("1x"), "whmcs");
        assert_eq!(
            super::coupon_code("summer 25%").as_deref(),
            Some("SUMMER-25-")
        );
        assert_eq!(super::coupon_code("ab"), None);
    }
}
