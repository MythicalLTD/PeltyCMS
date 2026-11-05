//! Coupons / promo codes: percent or fixed (per eligible item), first invoice
//! only or recurring, global and per-customer limits, validity window and
//! product restrictions. Redemption happens inside the order transaction with
//! the coupon row locked, so limits hold under concurrency.
use crate::db_err;
use pelty_billing::math;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Coupon {
    pub id: Uuid,
    pub code: String,
    pub description: String,
    pub kind: String,
    #[schema(value_type = String)]
    pub value: Decimal,
    pub currency: Option<String>,
    pub recurring: bool,
    pub max_uses: Option<i32>,
    pub max_uses_per_customer: Option<i32>,
    pub uses: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub starts_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<OffsetDateTime>,
    pub active: bool,
    #[serde(default)]
    #[sqlx(default)]
    pub product_ids: Vec<Uuid>,
    /// Recurring discount stops after this many invoices (None = forever).
    #[serde(default)]
    #[sqlx(default)]
    pub recurring_cycles: Option<i32>,
    #[serde(default)]
    #[sqlx(default)]
    pub applies_addons: bool,
    #[serde(default)]
    #[sqlx(default)]
    pub applies_domains: bool,
    /// Empty = every TLD (domain promos).
    #[serde(default)]
    #[sqlx(default)]
    pub tlds: Vec<String>,
    /// Empty = every customer.
    #[serde(default)]
    #[sqlx(default)]
    pub customer_ids: Vec<Uuid>,
    /// Empty = every client group.
    #[serde(default)]
    #[sqlx(default)]
    pub client_group_ids: Vec<Uuid>,
}

const COLS: &str = "c.id, c.code, c.description, c.kind, c.value, c.currency::text AS currency, c.recurring, c.max_uses,
    c.max_uses_per_customer, c.uses, c.starts_at, c.expires_at, c.active,
    c.recurring_cycles, c.applies_addons, c.applies_domains, c.tlds, c.customer_ids, c.client_group_ids,
    COALESCE((SELECT array_agg(product_id) FROM coupon_products WHERE coupon_id = c.id), '{}') AS product_ids";

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CouponInput {
    pub code: String,
    #[serde(default)]
    pub description: String,
    pub kind: String,
    #[schema(value_type = String)]
    pub value: Decimal,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub recurring: bool,
    #[serde(default)]
    pub max_uses: Option<i32>,
    #[serde(default)]
    pub max_uses_per_customer: Option<i32>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub starts_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub expires_at: Option<OffsetDateTime>,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub product_ids: Vec<Uuid>,
    #[serde(default)]
    pub recurring_cycles: Option<i32>,
    #[serde(default)]
    pub applies_addons: bool,
    #[serde(default)]
    pub applies_domains: bool,
    #[serde(default)]
    pub tlds: Vec<String>,
    #[serde(default)]
    pub customer_ids: Vec<Uuid>,
    #[serde(default)]
    pub client_group_ids: Vec<Uuid>,
}

fn yes() -> bool {
    true
}

/// Normalize a user-typed code (trim + uppercase).
pub fn normalize(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn validate(i: &CouponInput) -> AppResult<String> {
    let code = normalize(&i.code);
    let ok = (3..=40).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !ok {
        return Err(AppError::Validation("error.invalid_coupon_code"));
    }
    match i.kind.as_str() {
        "percent" if i.value > Decimal::ZERO && i.value <= Decimal::ONE_HUNDRED => {}
        "fixed" if i.value > Decimal::ZERO && i.currency.is_some() => {}
        _ => return Err(AppError::Validation("error.invalid_coupon_value")),
    }
    if let (Some(a), Some(b)) = (i.starts_at, i.expires_at) {
        if b <= a {
            return Err(AppError::Validation("error.invalid_coupon_dates"));
        }
    }
    if i.max_uses.is_some_and(|n| n <= 0)
        || i.max_uses_per_customer.is_some_and(|n| n <= 0)
        || i.description.len() > 500
        || i.recurring_cycles.is_some_and(|n| !(1..=600).contains(&n))
        || i.tlds.len() > 200
        || i.customer_ids.len() > 1000
        || i.client_group_ids.len() > 200
    {
        return Err(AppError::Validation("error.validation"));
    }
    Ok(code)
}

/// Discount for one line: percent of `base`, or a fixed amount capped at `base`.
/// Rounded to the currency exponent; never negative, never above `base`.
pub fn line_discount(kind: &str, value: Decimal, base: Decimal, exponent: u32) -> Decimal {
    if base <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let d = match kind {
        "percent" => math::round(base * value / Decimal::ONE_HUNDRED, exponent),
        _ => math::round(value, exponent),
    };
    d.clamp(Decimal::ZERO, base)
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<Coupon>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM coupons c ORDER BY c.created_at DESC, c.id"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Coupon>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM coupons c WHERE c.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

async fn set_products(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    products: &[Uuid],
) -> AppResult<()> {
    sqlx::query("DELETE FROM coupon_products WHERE coupon_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO coupon_products (coupon_id, product_id) SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING")
        .bind(id)
        .bind(products)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Create (`id = None`) or update a coupon.
pub async fn save(pool: &PgPool, id: Option<Uuid>, i: &CouponInput) -> AppResult<Coupon> {
    let code = validate(i)?;
    let currency = i.currency.as_ref().map(|c| c.to_ascii_uppercase());
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = match id {
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO coupons (id, code, description, kind, value, currency, recurring, max_uses, max_uses_per_customer, starts_at, expires_at, active)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
            )
            .bind(id).bind(&code).bind(&i.description).bind(&i.kind).bind(i.value).bind(&currency).bind(i.recurring)
            .bind(i.max_uses).bind(i.max_uses_per_customer).bind(i.starts_at).bind(i.expires_at).bind(i.active)
            .execute(&mut *tx).await.map_err(db_err)?;
            id
        }
        Some(id) => {
            let n = sqlx::query(
                "UPDATE coupons SET code=$2, description=$3, kind=$4, value=$5, currency=$6, recurring=$7, max_uses=$8,
                     max_uses_per_customer=$9, starts_at=$10, expires_at=$11, active=$12 WHERE id = $1",
            )
            .bind(id).bind(&code).bind(&i.description).bind(&i.kind).bind(i.value).bind(&currency).bind(i.recurring)
            .bind(i.max_uses).bind(i.max_uses_per_customer).bind(i.starts_at).bind(i.expires_at).bind(i.active)
            .execute(&mut *tx).await.map_err(db_err)?;
            if n.rows_affected() == 0 {
                return Err(AppError::NotFound);
            }
            id
        }
    };
    let tlds: Vec<String> = i
        .tlds
        .iter()
        .map(|t| t.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    sqlx::query("UPDATE coupons SET recurring_cycles=$2, applies_addons=$3, applies_domains=$4, tlds=$5, customer_ids=$6, client_group_ids=$7 WHERE id = $1")
        .bind(id).bind(i.recurring_cycles).bind(i.applies_addons).bind(i.applies_domains).bind(&tlds).bind(&i.customer_ids).bind(&i.client_group_ids)
        .execute(&mut *tx).await.map_err(db_err)?;
    set_products(&mut tx, id, &i.product_ids).await?;
    tx.commit().await.map_err(db_err)?;
    get(pool, id).await?.ok_or(AppError::NotFound)
}

/// Delete an unused coupon; used coupons can only be deactivated.
pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM coupons WHERE id = $1 AND uses = 0 AND NOT EXISTS (SELECT 1 FROM orders WHERE coupon_id = $1)")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(if get(pool, id).await?.is_some() {
            AppError::Conflict("error.coupon_in_use")
        } else {
            AppError::NotFound
        });
    }
    Ok(())
}

fn check_usable(c: &Coupon, currency: &str, now: OffsetDateTime) -> AppResult<()> {
    if !c.active || c.starts_at.is_some_and(|s| now < s) {
        return Err(AppError::Validation("error.coupon_invalid"));
    }
    if c.expires_at.is_some_and(|e| now >= e) {
        return Err(AppError::Validation("error.coupon_expired"));
    }
    if c.max_uses.is_some_and(|m| c.uses >= m) {
        return Err(AppError::Validation("error.coupon_exhausted"));
    }
    if c.kind == "fixed" && c.currency.as_deref() != Some(currency) {
        return Err(AppError::Validation("error.coupon_currency"));
    }
    Ok(())
}

async fn customer_uses(
    ex: impl sqlx::PgExecutor<'_>,
    coupon: Uuid,
    customer: Uuid,
) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM coupon_redemptions WHERE coupon_id = $1 AND customer_id = $2",
    )
    .bind(coupon)
    .bind(customer)
    .fetch_one(ex)
    .await
    .map_err(db_err)
}

/// Per-customer / client-group restriction.
async fn check_eligible(
    ex: impl sqlx::PgExecutor<'_>,
    c: &Coupon,
    customer: Uuid,
) -> AppResult<()> {
    if c.customer_ids.is_empty() && c.client_group_ids.is_empty() {
        return Ok(());
    }
    let group: Option<Uuid> =
        sqlx::query_scalar("SELECT client_group_id FROM customers WHERE id = $1")
            .bind(customer)
            .fetch_optional(ex)
            .await
            .map_err(db_err)?
            .flatten();
    let by_customer = c.customer_ids.contains(&customer);
    let by_group = group.is_some_and(|g| c.client_group_ids.contains(&g));
    if by_customer || by_group {
        Ok(())
    } else {
        Err(AppError::Validation("error.coupon_not_eligible"))
    }
}

/// Read-only check for the order form (no redemption).
pub async fn check(pool: &PgPool, code: &str, customer: Uuid, currency: &str) -> AppResult<Coupon> {
    let c: Coupon = sqlx::query_as(&format!("SELECT {COLS} FROM coupons c WHERE c.code = $1"))
        .bind(normalize(code))
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::Validation("error.coupon_invalid"))?;
    check_usable(
        &c,
        &currency.to_ascii_uppercase(),
        OffsetDateTime::now_utc(),
    )?;
    check_eligible(pool, &c, customer).await?;
    if let Some(m) = c.max_uses_per_customer {
        if customer_uses(pool, c.id, customer).await? >= i64::from(m) {
            return Err(AppError::Validation("error.coupon_exhausted"));
        }
    }
    Ok(c)
}

/// Lock and validate a coupon inside the order transaction.
pub async fn lock_for_order(
    tx: &mut Transaction<'_, Postgres>,
    code: &str,
    customer: Uuid,
    currency: &str,
) -> AppResult<Coupon> {
    let c: Coupon = sqlx::query_as(&format!(
        "SELECT {COLS} FROM coupons c WHERE c.code = $1 FOR UPDATE OF c"
    ))
    .bind(normalize(code))
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?
    .ok_or(AppError::Validation("error.coupon_invalid"))?;
    check_usable(&c, currency, OffsetDateTime::now_utc())?;
    check_eligible(&mut **tx, &c, customer).await?;
    if let Some(m) = c.max_uses_per_customer {
        if customer_uses(&mut **tx, c.id, customer).await? >= i64::from(m) {
            return Err(AppError::Validation("error.coupon_exhausted"));
        }
    }
    Ok(c)
}

impl Coupon {
    pub fn applies_to(&self, product: Uuid) -> bool {
        self.product_ids.is_empty() || self.product_ids.contains(&product)
    }
    pub fn applies_to_tld(&self, tld: &str) -> bool {
        self.applies_domains && (self.tlds.is_empty() || self.tlds.iter().any(|t| t == tld))
    }
}

/// Record a redemption (call after the order row exists, same transaction).
pub async fn redeem(
    tx: &mut Transaction<'_, Postgres>,
    c: &Coupon,
    customer: Uuid,
    order: Uuid,
    amount: Decimal,
) -> AppResult<()> {
    sqlx::query("UPDATE coupons SET uses = uses + 1 WHERE id = $1")
        .bind(c.id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO coupon_redemptions (id, coupon_id, customer_id, order_id, amount) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(c.id)
        .bind(customer)
        .bind(order)
        .bind(amount)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Record a domain-order redemption (same transaction as the domain row).
pub async fn redeem_domain(
    tx: &mut Transaction<'_, Postgres>,
    c: &Coupon,
    customer: Uuid,
    domain: Uuid,
    amount: Decimal,
) -> AppResult<()> {
    sqlx::query("UPDATE coupons SET uses = uses + 1 WHERE id = $1")
        .bind(c.id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO coupon_redemptions (id, coupon_id, customer_id, domain_id, amount) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(c.id)
        .bind(customer)
        .bind(domain)
        .bind(amount)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn percent_rounds_to_currency() {
        assert_eq!(
            line_discount("percent", dec("15"), dec("9.99"), 2),
            dec("1.50")
        );
        assert_eq!(
            line_discount("percent", dec("100"), dec("10"), 2),
            dec("10")
        );
        assert_eq!(
            line_discount("percent", dec("33.3333"), dec("1000"), 0),
            dec("333")
        );
    }

    #[test]
    fn fixed_is_capped_at_line() {
        assert_eq!(line_discount("fixed", dec("5"), dec("12"), 2), dec("5"));
        assert_eq!(line_discount("fixed", dec("50"), dec("12"), 2), dec("12"));
        assert_eq!(line_discount("fixed", dec("5"), dec("0"), 2), dec("0"));
    }

    #[test]
    fn codes_are_normalized_and_validated() {
        assert_eq!(normalize("  summer-10 "), "SUMMER-10");
        let mut i = CouponInput {
            code: "ok".into(),
            description: String::new(),
            kind: "percent".into(),
            value: dec("10"),
            currency: None,
            recurring: false,
            max_uses: None,
            max_uses_per_customer: None,
            starts_at: None,
            expires_at: None,
            active: true,
            product_ids: vec![],
            recurring_cycles: None,
            applies_addons: false,
            applies_domains: false,
            tlds: vec![],
            customer_ids: vec![],
            client_group_ids: vec![],
        };
        assert!(validate(&i).is_err());
        i.code = "WELCOME10".into();
        assert!(validate(&i).is_ok());
        i.value = dec("101");
        assert!(validate(&i).is_err());
        i.kind = "fixed".into();
        i.value = dec("5");
        assert!(validate(&i).is_err(), "fixed needs a currency");
    }
}
