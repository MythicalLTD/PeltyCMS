//! Game store rules (module `gaming_storefront`): per-customer and global
//! purchase limits, required packages, pay-what-you-want and gift purchases,
//! plus a ban list of in-game names and emails that may not buy.
//!
//! Everything here is a no-op while the module is off.
use crate::db_err;
use crate::orders::CartItem;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "gaming_storefront";
pub const PERIODS: &[&str] = &["all", "day", "week", "month"];
/// A pay-what-you-want amount may be at most this multiple of the price
/// (and never above [`PWYW_CAP`]); stops typos like 10000 instead of 10.00.
pub const PWYW_FACTOR: i64 = 100;
pub const PWYW_CAP: i64 = 100_000;
const MAX_REQUIRED: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[schema(as = PackageRule)]
pub struct Rule {
    pub product_id: Uuid,
    #[serde(default)]
    pub user_limit: Option<i32>,
    #[serde(default = "all")]
    pub user_period: String,
    #[serde(default)]
    pub global_limit: Option<i32>,
    #[serde(default = "all")]
    pub global_period: String,
    #[serde(default)]
    pub required_product_ids: Vec<Uuid>,
    #[serde(default)]
    pub pay_what_you_want: bool,
    #[serde(default = "yes")]
    pub giftable: bool,
}

fn all() -> String {
    "all".into()
}
fn yes() -> bool {
    true
}

impl Rule {
    pub fn validate(&self) -> AppResult<()> {
        let ok = PERIODS.contains(&self.user_period.as_str())
            && PERIODS.contains(&self.global_period.as_str())
            && self.user_limit.is_none_or(|n| (1..=100_000).contains(&n))
            && self
                .global_limit
                .is_none_or(|n| (1..=10_000_000).contains(&n))
            && self.required_product_ids.len() <= MAX_REQUIRED
            && !self.required_product_ids.contains(&self.product_id);
        if ok {
            Ok(())
        } else {
            Err(AppError::Validation("error.package_rule_invalid"))
        }
    }

    /// True when the rule changes nothing (stored rows are deleted instead).
    pub fn is_default(&self) -> bool {
        self.user_limit.is_none()
            && self.global_limit.is_none()
            && self.required_product_ids.is_empty()
            && !self.pay_what_you_want
            && self.giftable
    }
}

/// SQL interval start for a period (`'-infinity'` for `all`). Rolling windows.
fn since(period: &str) -> &'static str {
    match period {
        "day" => "now() - interval '1 day'",
        "week" => "now() - interval '7 days'",
        "month" => "now() - interval '30 days'",
        _ => "'-infinity'::timestamptz",
    }
}

pub async fn enabled(pool: &PgPool) -> AppResult<bool> {
    crate::feature_modules::is_enabled(pool, MODULE).await
}

const COLS: &str = "product_id, user_limit, user_period, global_limit, global_period, required_product_ids, pay_what_you_want, giftable";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Rule>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM package_rules ORDER BY updated_at DESC"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn for_products<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    ids: &[Uuid],
) -> AppResult<HashMap<Uuid, Rule>> {
    let rows: Vec<Rule> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM package_rules WHERE product_id = ANY($1)"
    )))
    .bind(ids)
    .fetch_all(ex)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|r| (r.product_id, r)).collect())
}

/// Save (or clear, when it is all defaults) the rule of one package.
pub async fn save(pool: &PgPool, r: &Rule) -> AppResult<()> {
    r.validate()?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM products WHERE id = $1)")
        .bind(r.product_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if !exists {
        return Err(AppError::NotFound);
    }
    if !r.required_product_ids.is_empty() {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM products WHERE id = ANY($1)")
            .bind(&r.required_product_ids)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
        if n != r.required_product_ids.len() as i64 {
            return Err(AppError::Validation("error.package_rule_invalid"));
        }
    }
    if r.is_default() {
        sqlx::query("DELETE FROM package_rules WHERE product_id = $1")
            .bind(r.product_id)
            .execute(pool)
            .await
            .map_err(db_err)?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO package_rules (product_id, user_limit, user_period, global_limit, global_period, required_product_ids, pay_what_you_want, giftable, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,now())
         ON CONFLICT (product_id) DO UPDATE SET user_limit=$2, user_period=$3, global_limit=$4, global_period=$5,
           required_product_ids=$6, pay_what_you_want=$7, giftable=$8, updated_at=now()",
    )
    .bind(r.product_id)
    .bind(r.user_limit)
    .bind(&r.user_period)
    .bind(r.global_limit)
    .bind(&r.global_period)
    .bind(&r.required_product_ids)
    .bind(r.pay_what_you_want)
    .bind(r.giftable)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Purchases of `product` (order lines on orders that were not cancelled or
/// marked fraud) since the start of `period`, for one customer or everyone.
async fn bought<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    product: Uuid,
    customer: Option<Uuid>,
    period: &str,
) -> AppResult<i64> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM order_items oi JOIN orders o ON o.id = oi.order_id
          WHERE oi.product_id = $1 AND o.status NOT IN ('cancelled', 'fraud')
            AND ($2::uuid IS NULL OR o.customer_id = $2) AND o.created_at >= {}",
        since(period)
    )))
    .bind(product)
    .bind(customer)
    .fetch_one(ex)
    .await
    .map_err(db_err)
}

/// Does the customer own one of `products` (an active or suspended service, or
/// a paid order line for one-time products)?
async fn owns_any<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    customer: Uuid,
    products: &[Uuid],
) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM services WHERE customer_id = $1 AND product_id = ANY($2) AND status IN ('active', 'suspended'))
             OR EXISTS (SELECT 1 FROM order_items oi JOIN orders o ON o.id = oi.order_id JOIN invoices i ON i.order_id = o.id
                         WHERE o.customer_id = $1 AND oi.product_id = ANY($2) AND i.status = 'paid')",
    )
    .bind(customer)
    .bind(products)
    .fetch_one(ex)
    .await
    .map_err(db_err)
}

/// Normalised gift recipient, or an error for names the delivery could not use.
pub fn gift_name(raw: &str) -> AppResult<String> {
    let n = raw.trim();
    if crate::community::username_ok(n) && !n.contains("  ") {
        Ok(n.to_string())
    } else {
        Err(AppError::Validation("error.gift_name_invalid"))
    }
}

/// The amount charged for a line: the listed price, or the buyer's
/// pay-what-you-want amount when the package allows it.
pub fn line_amount(
    rule: Option<&Rule>,
    cycle: &str,
    listed: Decimal,
    pay: Option<Decimal>,
) -> AppResult<Decimal> {
    let Some(pay) = pay else { return Ok(listed) };
    if !rule.is_some_and(|r| r.pay_what_you_want) || cycle != "one_time" {
        return Err(AppError::Validation("error.pay_amount_invalid"));
    }
    let max = (listed * Decimal::from(PWYW_FACTOR))
        .max(Decimal::ONE)
        .min(Decimal::from(PWYW_CAP));
    if pay < listed || pay > max || pay.scale() > 2 {
        return Err(AppError::Validation("error.pay_amount_invalid"));
    }
    Ok(pay)
}

/// Rules for the cart lines (empty while the module is off).
pub async fn for_cart(pool: &PgPool, items: &[CartItem]) -> AppResult<HashMap<Uuid, Rule>> {
    if !enabled(pool).await? {
        return Ok(HashMap::new());
    }
    let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
    for_products(pool, &ids).await
}

/// Input checks that need no customer: gifts and pay-what-you-want shape.
/// Used by the cart preview and again at checkout.
pub fn check_lines(
    rules: &HashMap<Uuid, Rule>,
    items: &[CartItem],
    module_on: bool,
) -> AppResult<()> {
    for it in items {
        if let Some(g) = &it.gift_to {
            if !module_on || !rules.get(&it.product_id).is_none_or(|r| r.giftable) {
                return Err(AppError::Validation("error.gift_not_allowed"));
            }
            gift_name(g)?;
        }
        if it.pay_amount.is_some()
            && !(module_on
                && rules
                    .get(&it.product_id)
                    .is_some_and(|r| r.pay_what_you_want))
        {
            return Err(AppError::Validation("error.pay_amount_invalid"));
        }
    }
    Ok(())
}

/// Checkout checks inside the order transaction: ban list, limits (rule rows
/// locked so two checkouts cannot both take the last unit) and required
/// packages. Returns the rules of the cart's packages (empty while the
/// module is off).
pub async fn enforce(
    tx: &mut Transaction<'_, Postgres>,
    customer: Uuid,
    items: &[CartItem],
) -> AppResult<HashMap<Uuid, Rule>> {
    let on: bool = sqlx::query_scalar(
        "SELECT COALESCE((SELECT enabled FROM feature_modules WHERE id = $1), false)",
    )
    .bind(MODULE)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    if !on {
        if items
            .iter()
            .any(|i| i.gift_to.is_some() || i.pay_amount.is_some())
        {
            return Err(AppError::Validation("error.gift_not_allowed"));
        }
        return Ok(HashMap::new());
    }
    banned(&mut **tx, customer, items).await?;
    let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
    let rows: Vec<Rule> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM package_rules WHERE product_id = ANY($1) ORDER BY product_id FOR UPDATE"
    )))
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let rules: HashMap<Uuid, Rule> = rows.into_iter().map(|r| (r.product_id, r)).collect();
    check_lines(&rules, items, true)?;
    for (pid, r) in &rules {
        let in_cart = items.iter().filter(|i| i.product_id == *pid).count() as i64;
        if let Some(n) = r.user_limit {
            if bought(&mut **tx, *pid, Some(customer), &r.user_period).await? + in_cart
                > i64::from(n)
            {
                return Err(AppError::Validation("error.purchase_limit_reached"));
            }
        }
        if let Some(n) = r.global_limit {
            if bought(&mut **tx, *pid, None, &r.global_period).await? + in_cart > i64::from(n) {
                return Err(AppError::Validation("error.package_sold_out"));
            }
        }
        if !r.required_product_ids.is_empty()
            && !items
                .iter()
                .any(|i| r.required_product_ids.contains(&i.product_id))
            && !owns_any(&mut **tx, customer, &r.required_product_ids).await?
        {
            return Err(AppError::Validation("error.package_requires"));
        }
    }
    Ok(rules)
}

// ------------------------------------------------------------------ public view

/// What the store shows on a package: limits left, requirements, PWYW, gifting.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[schema(as = PublicPackageRule)]
pub struct PublicRule {
    pub product_id: Uuid,
    pub user_limit: Option<i32>,
    pub user_period: String,
    /// Left for the signed-in customer (None for guests or without a limit).
    pub user_left: Option<i64>,
    pub global_limit: Option<i32>,
    pub global_period: String,
    pub global_left: Option<i64>,
    pub required: Vec<RequiredPackage>,
    /// The signed-in customer owns one of `required` (None for guests).
    pub requirement_met: Option<bool>,
    pub pay_what_you_want: bool,
    pub giftable: bool,
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct RequiredPackage {
    pub id: Uuid,
    pub name: String,
}

pub async fn public(pool: &PgPool, customer: Option<Uuid>) -> AppResult<Vec<PublicRule>> {
    if !enabled(pool).await? {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for r in list(pool).await? {
        let user_left = match (r.user_limit, customer) {
            (Some(n), Some(c)) => Some(
                (i64::from(n) - bought(pool, r.product_id, Some(c), &r.user_period).await?).max(0),
            ),
            _ => None,
        };
        let global_left = match r.global_limit {
            Some(n) => Some(
                (i64::from(n) - bought(pool, r.product_id, None, &r.global_period).await?).max(0),
            ),
            None => None,
        };
        let required: Vec<RequiredPackage> = if r.required_product_ids.is_empty() {
            vec![]
        } else {
            sqlx::query_as("SELECT id, name FROM products WHERE id = ANY($1) ORDER BY name")
                .bind(&r.required_product_ids)
                .fetch_all(pool)
                .await
                .map_err(db_err)?
        };
        let requirement_met = match customer {
            Some(c) if !required.is_empty() => {
                Some(owns_any(pool, c, &r.required_product_ids).await?)
            }
            _ => None,
        };
        out.push(PublicRule {
            product_id: r.product_id,
            user_limit: r.user_limit,
            user_period: r.user_period,
            user_left,
            global_limit: r.global_limit,
            global_period: r.global_period,
            global_left,
            required,
            requirement_met,
            pay_what_you_want: r.pay_what_you_want,
            giftable: r.giftable,
        });
    }
    Ok(out)
}

// ------------------------------------------------------------------ ban list

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
#[schema(as = StoreBan)]
pub struct Ban {
    pub id: Uuid,
    pub kind: String,
    pub value: String,
    pub reason: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

pub async fn bans(pool: &PgPool) -> AppResult<Vec<Ban>> {
    sqlx::query_as("SELECT id, kind, value, reason, created_at FROM store_bans ORDER BY created_at DESC LIMIT 1000")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Normalised ban value, or a validation error.
pub fn ban_value(kind: &str, value: &str) -> AppResult<String> {
    let v = value.trim().to_lowercase();
    let ok = match kind {
        "username" => crate::community::username_ok(&v),
        "email" => v.len() <= 254 && v.contains('@') && !v.contains(char::is_whitespace),
        _ => false,
    };
    if ok {
        Ok(v)
    } else {
        Err(AppError::Validation("error.store_ban_invalid"))
    }
}

pub async fn add_ban(pool: &PgPool, kind: &str, value: &str, reason: &str) -> AppResult<Ban> {
    let v = ban_value(kind, value)?;
    if reason.chars().count() > 500 {
        return Err(AppError::Validation("error.store_ban_invalid"));
    }
    sqlx::query_as(
        "INSERT INTO store_bans (kind, value, reason) VALUES ($1,$2,$3)
         ON CONFLICT (kind, value) DO UPDATE SET reason = EXCLUDED.reason
         RETURNING id, kind, value, reason, created_at",
    )
    .bind(kind)
    .bind(v)
    .bind(reason.trim())
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn remove_ban(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM store_bans WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// The buyer's email, their in-game names and any gift recipient are checked
/// against the ban list. The error does not say which entry matched.
/// Ban list check before an account exists (guest checkout): the email or
/// any of the player names is on the store ban list. `false` while the
/// store rules module is off.
pub async fn banned_guest(pool: &PgPool, email: &str, names: &[String]) -> AppResult<bool> {
    if !crate::feature_modules::is_enabled(pool, MODULE).await? {
        return Ok(false);
    }
    let names: Vec<String> = names.iter().map(|n| n.trim().to_lowercase()).collect();
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM store_bans b WHERE
            (b.kind = 'email' AND b.value = lower($1)) OR (b.kind = 'username' AND b.value = ANY($2)))",
    )
    .bind(email.trim())
    .bind(&names)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

async fn banned<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    customer: Uuid,
    items: &[CartItem],
) -> AppResult<()> {
    let gifts: Vec<String> = items
        .iter()
        .filter_map(|i| i.gift_to.as_deref())
        .map(|g| g.trim().to_lowercase())
        .collect();
    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM store_bans b WHERE
            (b.kind = 'email' AND b.value = (SELECT lower(email::text) FROM customers WHERE id = $1))
         OR (b.kind = 'username' AND (b.value = ANY($2) OR b.value IN
              (SELECT lower(a.value) FROM customer_game_accounts a WHERE a.customer_id = $1 AND a.field = 'username'))))",
    )
    .bind(customer)
    .bind(&gifts)
    .fetch_one(ex)
    .await
    .map_err(db_err)?;
    if hit {
        Err(AppError::Validation("error.store_banned"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        Rule {
            product_id: Uuid::new_v4(),
            user_limit: None,
            user_period: "all".into(),
            global_limit: None,
            global_period: "all".into(),
            required_product_ids: vec![],
            pay_what_you_want: false,
            giftable: true,
        }
    }

    #[test]
    fn validates_rules() {
        let r = rule();
        assert!(r.is_default());
        assert!(r.validate().is_ok());
        assert!(Rule {
            user_period: "year".into(),
            ..r.clone()
        }
        .validate()
        .is_err());
        assert!(Rule {
            user_limit: Some(0),
            ..r.clone()
        }
        .validate()
        .is_err());
        assert!(Rule {
            required_product_ids: vec![r.product_id],
            ..r.clone()
        }
        .validate()
        .is_err());
        assert!(!Rule {
            giftable: false,
            ..r
        }
        .is_default());
    }

    #[test]
    fn pay_what_you_want_amounts() {
        let r = Rule {
            pay_what_you_want: true,
            ..rule()
        };
        let d = |s: &str| s.parse::<Decimal>().unwrap();
        assert_eq!(line_amount(None, "monthly", d("5"), None).unwrap(), d("5"));
        assert_eq!(
            line_amount(Some(&r), "one_time", d("5"), Some(d("12.50"))).unwrap(),
            d("12.50")
        );
        assert!(line_amount(Some(&r), "one_time", d("5"), Some(d("4.99"))).is_err());
        assert!(line_amount(Some(&r), "one_time", d("5"), Some(d("501"))).is_err());
        assert!(line_amount(Some(&r), "one_time", d("5"), Some(d("5.001"))).is_err());
        assert!(line_amount(Some(&r), "monthly", d("5"), Some(d("6"))).is_err());
        assert!(line_amount(Some(&rule()), "one_time", d("5"), Some(d("6"))).is_err());
        // Free packages: up to 1 (factor of 0 would allow nothing).
        assert!(line_amount(Some(&r), "one_time", d("0"), Some(d("1"))).is_ok());
    }

    #[test]
    fn gift_and_ban_values() {
        assert_eq!(gift_name(" Steve ").unwrap(), "Steve");
        assert!(gift_name("x\" op").is_err());
        assert!(gift_name("").is_err());
        assert_eq!(
            ban_value("email", " Bad@Example.com ").unwrap(),
            "bad@example.com"
        );
        assert_eq!(ban_value("username", "Griefer_1").unwrap(), "griefer_1");
        assert!(ban_value("ip", "1.2.3.4").is_err());
        assert!(ban_value("email", "nope").is_err());
    }
}
