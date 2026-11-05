//! Tax rules, tax settings and applying the tax engine to a draft invoice
//! right before it is finalized. The result (rates, breakdown, customer
//! snapshot) is frozen with the invoice by the triggers in 0096.
use crate::{db_err, invoices::Tx, settings};
use pelty_billing::tax::{self as engine, FormatOnly, Rule, TaxPart};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub const TAX_KEY: &str = "billing.tax";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TaxSettings {
    /// Apply tax rules to invoices at all.
    pub enabled: bool,
    /// Catalog prices are gross (tax included).
    pub inclusive: bool,
    /// Seller country (ISO 3166-1 alpha-2), used for EU reverse charge.
    pub home_country: String,
    /// Apply EU VAT reverse charge to cross-border B2B customers.
    pub reverse_charge: bool,
    /// Reverse charge only for VAT numbers verified valid in VIES (not just
    /// well-formed).
    pub require_vies: bool,
}

impl TaxSettings {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        self.home_country = self.home_country.trim().to_ascii_uppercase();
        if !(self.home_country.is_empty()
            || (self.home_country.len() == 2
                && self.home_country.bytes().all(|b| b.is_ascii_uppercase())))
        {
            return Err("error.invalid_country");
        }
        Ok(self)
    }
}

pub async fn get_settings(pool: &PgPool) -> AppResult<TaxSettings> {
    Ok(settings::get(pool, TAX_KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put_settings(pool: &PgPool, s: &TaxSettings) -> AppResult<()> {
    settings::set(
        pool,
        TAX_KEY,
        serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct TaxRule {
    pub id: Uuid,
    pub name: String,
    pub country: String,
    pub region: String,
    #[schema(value_type = String)]
    pub rate: Decimal,
    pub compound: bool,
    pub is_vat: bool,
    pub priority: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    pub name: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub region: String,
    #[schema(value_type = String)]
    pub rate: Decimal,
    #[serde(default)]
    pub compound: bool,
    #[serde(default = "yes")]
    pub is_vat: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

impl RuleInput {
    fn validate(&mut self) -> AppResult<()> {
        self.name = self.name.trim().to_string();
        self.country = self.country.trim().to_ascii_uppercase();
        self.region = self.region.trim().to_string();
        if self.name.is_empty()
            || self.name.chars().count() > 100
            || self.region.chars().count() > 100
        {
            return Err(AppError::Validation("error.validation"));
        }
        if !(self.country.is_empty()
            || (self.country.len() == 2 && self.country.bytes().all(|b| b.is_ascii_uppercase())))
        {
            return Err(AppError::Validation("error.invalid_country"));
        }
        if self.rate < Decimal::ZERO || self.rate > Decimal::ONE_HUNDRED || self.rate.scale() > 4 {
            return Err(AppError::Validation("error.invalid_tax_rate"));
        }
        Ok(())
    }
}

const RCOLS: &str = "id, name, country, region, rate, compound, is_vat, priority, enabled";

pub async fn list_rules(pool: &PgPool) -> AppResult<Vec<TaxRule>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {RCOLS} FROM tax_rules ORDER BY country, region, priority, name"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_rule(pool: &PgPool, mut r: RuleInput) -> AppResult<TaxRule> {
    r.validate()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO tax_rules (id, name, country, region, rate, compound, is_vat, priority, enabled)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {RCOLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(&r.name)
    .bind(&r.country)
    .bind(&r.region)
    .bind(r.rate)
    .bind(r.compound)
    .bind(r.is_vat)
    .bind(r.priority)
    .bind(r.enabled)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update_rule(pool: &PgPool, id: Uuid, mut r: RuleInput) -> AppResult<Option<TaxRule>> {
    r.validate()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE tax_rules SET name=$2, country=$3, region=$4, rate=$5, compound=$6, is_vat=$7, priority=$8, enabled=$9
         WHERE id = $1 RETURNING {RCOLS}"
    )))
    .bind(id)
    .bind(&r.name)
    .bind(&r.country)
    .bind(&r.region)
    .bind(r.rate)
    .bind(r.compound)
    .bind(r.is_vat)
    .bind(r.priority)
    .bind(r.enabled)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn delete_rule(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    Ok(sqlx::query("DELETE FROM tax_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected()
        > 0)
}

/// Pick the most specific matching tier: country+region, then country,
/// then global ('' country). Rules inside the tier apply in priority order.
pub fn select(rules: &[TaxRule], country: &str, region: &str) -> Vec<TaxRule> {
    let region = region.trim();
    let tier = |r: &TaxRule| -> Option<u8> {
        if !r.enabled {
            return None;
        }
        match (r.country.as_str(), r.region.as_str()) {
            ("", "") => Some(0),
            (c, "") if c == country => Some(1),
            (c, rg) if c == country && rg.eq_ignore_ascii_case(region) => Some(2),
            _ => None,
        }
    };
    let Some(best) = rules.iter().filter_map(tier).max() else {
        return vec![];
    };
    let mut v: Vec<TaxRule> = rules
        .iter()
        .filter(|r| tier(r) == Some(best))
        .cloned()
        .collect();
    v.sort_by_key(|r| r.priority);
    v
}

#[derive(FromRow)]
struct Cust {
    organization: String,
    first_name: String,
    last_name: String,
    email: String,
    address_line1: String,
    address_line2: String,
    city: String,
    region: String,
    postal_code: String,
    country: String,
    tax_id: String,
    vat_number: String,
    tax_exempt: bool,
    vat_status: String,
    vat_checked_num: String,
}

/// Format check plus (optionally) the stored VIES verdict.
struct Verified(bool);
impl engine::VatValidator for Verified {
    fn validate(&self, vat: &str) -> Result<bool, String> {
        Ok(self.0 && <FormatOnly as engine::VatValidator>::validate(&FormatOnly, vat)?)
    }
}

/// Price all `tax_auto` lines of a draft invoice and snapshot the customer.
pub async fn apply(tx: &mut Tx<'_>, invoice_id: Uuid) -> AppResult<()> {
    let cfg: TaxSettings = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM system_settings WHERE key = $1",
    )
    .bind(TAX_KEY)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?
    .and_then(|v| serde_json::from_value(v).ok())
    .unwrap_or_default();
    let (currency,): (String,) =
        sqlx::query_as("SELECT currency::text FROM invoices WHERE id = $1")
            .bind(invoice_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
    let c: Cust = sqlx::query_as(
        "SELECT organization, first_name, last_name, email::text AS email, address_line1, address_line2, city, region, postal_code,
                country, tax_id, vat_number, tax_exempt, vat_status, vat_checked_num
         FROM customers WHERE id = (SELECT customer_id FROM invoices WHERE id = $1)",
    )
    .bind(invoice_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    let snapshot = serde_json::json!({
        "organization": c.organization, "first_name": c.first_name, "last_name": c.last_name, "email": c.email,
        "address_line1": c.address_line1, "address_line2": c.address_line2, "city": c.city, "region": c.region,
        "postal_code": c.postal_code, "country": c.country, "tax_id": c.tax_id, "vat_number": c.vat_number, "tax_exempt": c.tax_exempt,
    });
    let reverse = cfg.enabled
        && cfg.reverse_charge
        && !c.tax_exempt
        && engine::reverse_charge(
            &cfg.home_country,
            &c.country,
            &c.vat_number,
            &Verified(
                !cfg.require_vies || (c.vat_status == "valid" && c.vat_checked_num == c.vat_number),
            ),
        );
    let rules: Vec<Rule> = if cfg.enabled && !c.tax_exempt {
        let all: Vec<TaxRule> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {RCOLS} FROM tax_rules WHERE enabled"
        )))
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
        select(&all, &c.country, &c.region)
            .into_iter()
            .filter(|r| !(reverse && r.is_vat))
            .map(|r| Rule {
                name: r.name,
                rate: r.rate,
                compound: r.compound,
                is_vat: r.is_vat,
            })
            .collect()
    } else {
        vec![]
    };
    let exp = crate::invoices::exponent(tx, &currency).await?;
    let rate = engine::effective_rate(&rules)
        .map_err(|_| AppError::Validation("error.invalid_tax_rate"))?;
    let items: Vec<(Uuid, Decimal, Decimal)> =
        sqlx::query_as("SELECT id, quantity, unit_price FROM invoice_items WHERE invoice_id = $1 AND tax_auto ORDER BY sort_order, id")
            .bind(invoice_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?;
    let mut breakdown: Vec<TaxPart> = vec![];
    let inclusive = cfg.enabled && cfg.inclusive;
    for (id, qty, unit) in items {
        let gross_or_net = pelty_billing::math::round(qty * unit, exp);
        let l = if inclusive {
            engine::on_gross(gross_or_net, &rules, exp)
        } else {
            engine::on_net(gross_or_net, &rules, exp)
        }
        .map_err(|_| AppError::Validation("error.invalid_amount"))?;
        engine::merge(&mut breakdown, &l.parts);
        sqlx::query(
            "UPDATE invoice_items SET amount = $2, tax_amount = $3, tax_rate = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(l.net)
        .bind(l.tax)
        .bind(rate)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    // Manual-rate lines show up in the breakdown under their own rate.
    let manual: Vec<(Decimal, Decimal)> = sqlx::query_as(
        "SELECT tax_rate, sum(tax_amount) FROM invoice_items WHERE invoice_id = $1 AND NOT tax_auto AND tax_amount <> 0 GROUP BY tax_rate ORDER BY tax_rate",
    )
    .bind(invoice_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    for (rate, amount) in manual {
        engine::merge(
            &mut breakdown,
            &[TaxPart {
                name: "Tax".into(),
                rate,
                compound: false,
                amount,
            }],
        );
    }
    crate::invoices::recompute(tx, invoice_id).await?;
    sqlx::query("UPDATE invoices SET tax_inclusive = $2, reverse_charge = $3, tax_breakdown = $4, customer_snapshot = $5 WHERE id = $1")
        .bind(invoice_id)
        .bind(inclusive)
        .bind(reverse)
        .bind(serde_json::to_value(&breakdown).map_err(|e| AppError::Internal(e.to_string()))?)
        .bind(snapshot)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Stored VAT number of a customer (`None` = no such customer).
pub async fn vat_number_of(pool: &PgPool, customer: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar("SELECT vat_number FROM customers WHERE id = $1")
        .bind(customer)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Persist a VIES verdict for the number that was checked.
pub async fn set_vat_result(
    pool: &PgPool,
    customer: Uuid,
    number: &str,
    valid: bool,
    name: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE customers SET vat_status = $2, vat_checked_num = $3, vat_checked_at = now(), vat_name = $4 WHERE id = $1")
        .bind(customer)
        .bind(if valid { "valid" } else { "invalid" })
        .bind(number)
        .bind(name)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(country: &str, region: &str, prio: i32) -> TaxRule {
        TaxRule {
            id: Uuid::new_v4(),
            name: format!("{country}{region}"),
            country: country.into(),
            region: region.into(),
            rate: Decimal::ONE,
            compound: false,
            is_vat: true,
            priority: prio,
            enabled: true,
        }
    }
    #[test]
    fn most_specific_tier_wins() {
        let rules = vec![
            r("", "", 0),
            r("US", "", 0),
            r("US", "CA", 1),
            r("US", "CA", 0),
        ];
        assert_eq!(
            select(&rules, "US", "ca")
                .iter()
                .map(|r| r.priority)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(select(&rules, "US", "NY")[0].name, "US");
        assert_eq!(select(&rules, "DE", "")[0].name, "");
        assert!(select(&rules[1..], "DE", "").is_empty());
    }
}
