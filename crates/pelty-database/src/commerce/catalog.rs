//! Product catalog: groups, products, per-cycle/currency prices, config
//! options. Listing is done in a fixed number of queries (no N+1).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub sort_order: i32,
    pub hidden: bool,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Product {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub status: String,
    pub module: Option<String>,
    pub module_config: serde_json::Value,
    pub sort_order: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Stock tracking on; `stock_qty` units left (may go negative on backorder).
    pub stock_enabled: bool,
    pub stock_qty: i32,
    pub allow_backorder: bool,
}

impl Product {
    /// Can one more unit be ordered right now?
    pub fn orderable(&self) -> bool {
        stock_allows(self.stock_enabled, self.stock_qty, self.allow_backorder)
    }
}

/// Stock rule shared by the storefront badge and order placement.
pub fn stock_allows(enabled: bool, qty: i32, backorder: bool) -> bool {
    !enabled || qty > 0 || backorder
}

/// Take one unit of stock inside an order transaction. Locks the product row
/// so concurrent checkouts can't oversell.
pub async fn take_stock(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    product: Uuid,
) -> AppResult<()> {
    let row: Option<(bool, i32, bool)> = sqlx::query_as(
        "SELECT stock_enabled, stock_qty, allow_backorder FROM products WHERE id = $1 FOR UPDATE",
    )
    .bind(product)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (enabled, qty, backorder) = row.ok_or(AppError::NotFound)?;
    if !enabled {
        return Ok(());
    }
    if !stock_allows(enabled, qty, backorder) {
        return Err(AppError::Validation("error.out_of_stock"));
    }
    sqlx::query("UPDATE products SET stock_qty = stock_qty - 1 WHERE id = $1")
        .bind(product)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Admin stock settings.
pub async fn set_stock(
    pool: &PgPool,
    product: Uuid,
    enabled: bool,
    qty: i32,
    backorder: bool,
) -> AppResult<()> {
    if !(-1_000_000..=1_000_000).contains(&qty) {
        return Err(AppError::Validation("error.validation"));
    }
    let r = sqlx::query("UPDATE products SET stock_enabled = $2, stock_qty = $3, allow_backorder = $4 WHERE id = $1")
        .bind(product)
        .bind(enabled)
        .bind(qty)
        .bind(backorder)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod stock_tests {
    use super::stock_allows;
    #[test]
    fn rules() {
        assert!(stock_allows(false, 0, false));
        assert!(stock_allows(true, 1, false));
        assert!(!stock_allows(true, 0, false));
        assert!(stock_allows(true, -3, true));
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Price {
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub product_id: Option<Uuid>,
    pub currency: String,
    pub cycle: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    #[serde(default)]
    #[schema(value_type = String)]
    pub setup_fee: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigOption {
    pub id: Uuid,
    pub name: String,
    pub required: bool,
    pub option_type: String,
    pub qty_min: i32,
    pub qty_max: i32,
    pub choices: Vec<ConfigChoice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigChoice {
    pub id: Uuid,
    pub label: String,
    pub prices: Vec<ChoicePrice>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChoicePrice {
    pub currency: String,
    pub cycle: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductFull {
    #[serde(flatten)]
    pub product: Product,
    pub prices: Vec<Price>,
    pub options: Vec<ConfigOption>,
}

const PCOLS: &str =
    "id, group_id, name, slug, description, status, module, module_config, sort_order, created_at, stock_enabled, stock_qty, allow_backorder";

pub fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn check_name(n: &str, max: usize) -> AppResult<()> {
    if n.trim().is_empty() || n.chars().count() > max {
        return Err(AppError::Validation("error.validation"));
    }
    Ok(())
}

pub async fn create_group(
    pool: &PgPool,
    name: &str,
    slug: &str,
    sort: i32,
    hidden: bool,
) -> AppResult<Group> {
    check_name(name, 120)?;
    if !valid_slug(slug) {
        return Err(AppError::Validation("error.invalid_slug"));
    }
    sqlx::query_as("INSERT INTO product_groups (id, name, slug, sort_order, hidden) VALUES ($1,$2,$3,$4,$5) RETURNING *")
        .bind(Uuid::new_v4())
        .bind(name.trim())
        .bind(slug)
        .bind(sort)
        .bind(hidden)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn groups(pool: &PgPool) -> AppResult<Vec<Group>> {
    sqlx::query_as("SELECT * FROM product_groups ORDER BY sort_order, name")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductInput {
    pub group_id: Option<Uuid>,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    /// Provisioning module id (plugin), or empty string to clear.
    pub module: Option<String>,
    pub module_config: Option<serde_json::Value>,
    pub sort_order: Option<i32>,
}

impl ProductInput {
    fn validate(&self) -> AppResult<()> {
        if let Some(n) = &self.name {
            check_name(n, 200)?;
        }
        if self.slug.as_deref().is_some_and(|s| !valid_slug(s)) {
            return Err(AppError::Validation("error.invalid_slug"));
        }
        if self.description.as_ref().is_some_and(|d| d.len() > 5000) {
            return Err(AppError::Validation("error.field_too_long"));
        }
        if self
            .status
            .as_deref()
            .is_some_and(|s| !["active", "hidden", "retired"].contains(&s))
        {
            return Err(AppError::Validation("error.invalid_status"));
        }
        if self
            .module_config
            .as_ref()
            .is_some_and(|c| !c.is_object() || c.to_string().len() > 16_384)
        {
            return Err(AppError::Validation("error.validation"));
        }
        Ok(())
    }
}

pub async fn create_product(pool: &PgPool, i: &ProductInput) -> AppResult<Product> {
    i.validate()?;
    let (Some(g), Some(n), Some(s)) = (i.group_id, i.name.as_deref(), i.slug.as_deref()) else {
        return Err(AppError::Validation("error.validation"));
    };
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO products (id, group_id, name, slug, description, status, module, module_config, sort_order)
         VALUES ($1,$2,$3,$4,COALESCE($5,''),COALESCE($6,'active'),NULLIF($7,''),COALESCE($8,'{{}}'::jsonb),COALESCE($9,0))
         RETURNING {PCOLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(g)
    .bind(n.trim())
    .bind(s)
    .bind(&i.description)
    .bind(&i.status)
    .bind(&i.module)
    .bind(&i.module_config)
    .bind(i.sort_order)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update_product(
    pool: &PgPool,
    id: Uuid,
    i: &ProductInput,
) -> AppResult<Option<Product>> {
    i.validate()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE products SET group_id = COALESCE($2, group_id), name = COALESCE($3, name), slug = COALESCE($4, slug),
            description = COALESCE($5, description), status = COALESCE($6, status),
            module = CASE WHEN $7::text IS NULL THEN module ELSE NULLIF($7, '') END,
            module_config = COALESCE($8, module_config), sort_order = COALESCE($9, sort_order)
         WHERE id = $1 RETURNING {PCOLS}"
    )))
    .bind(id)
    .bind(i.group_id)
    .bind(&i.name)
    .bind(&i.slug)
    .bind(&i.description)
    .bind(&i.status)
    .bind(&i.module)
    .bind(&i.module_config)
    .bind(i.sort_order)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Replace the full price list of a product atomically.
pub async fn set_prices(pool: &PgPool, product_id: Uuid, prices: &[Price]) -> AppResult<()> {
    for p in prices {
        if !pelty_billing::cycle::is_valid(&p.cycle)
            || p.amount < Decimal::ZERO
            || p.setup_fee < Decimal::ZERO
        {
            return Err(AppError::Validation("error.invalid_price"));
        }
    }
    let cur = currencies(pool).await?;
    for p in prices {
        check_scale(&cur, &p.currency, &[p.amount, p.setup_fee])?;
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE id = $1 FOR UPDATE)")
            .bind(product_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    if !exists {
        return Err(AppError::NotFound);
    }
    sqlx::query("DELETE FROM product_prices WHERE product_id = $1")
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let cur: Vec<String> = prices
        .iter()
        .map(|p| p.currency.to_ascii_uppercase())
        .collect();
    let cyc: Vec<&str> = prices.iter().map(|p| p.cycle.as_str()).collect();
    let amt: Vec<Decimal> = prices.iter().map(|p| p.amount).collect();
    let setup: Vec<Decimal> = prices.iter().map(|p| p.setup_fee).collect();
    let ids: Vec<Uuid> = prices.iter().map(|_| Uuid::new_v4()).collect();
    sqlx::query(
        "INSERT INTO product_prices (id, product_id, currency, cycle, amount, setup_fee)
         SELECT i, $2, c, y, a, s FROM unnest($1::uuid[], $3::text[], $4::text[], $5::numeric[], $6::numeric[]) AS t(i, c, y, a, s)",
    )
    .bind(&ids)
    .bind(product_id)
    .bind(&cur)
    .bind(&cyc)
    .bind(&amt)
    .bind(&setup)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct OptionInput {
    pub name: String,
    #[serde(default = "yes")]
    pub required: bool,
    /// dropdown | radio | quantity | yesno
    #[serde(default = "dropdown")]
    pub option_type: String,
    #[serde(default)]
    pub qty_min: i32,
    #[serde(default = "hundred")]
    pub qty_max: i32,
    pub choices: Vec<ChoiceInput>,
}
fn yes() -> bool {
    true
}
fn dropdown() -> String {
    "dropdown".into()
}
fn hundred() -> i32 {
    100
}

pub const OPTION_TYPES: [&str; 4] = ["dropdown", "radio", "quantity", "yesno"];

/// A customer's selection for one option: a choice id (dropdown/radio/yesno)
/// or a quantity (quantity options).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    Choice(Uuid),
    Qty(u32),
}

/// One option as needed for pricing.
#[derive(Debug, Clone)]
pub struct PricedOption {
    pub id: Uuid,
    pub name: String,
    pub required: bool,
    pub option_type: String,
    pub qty_min: i32,
    pub qty_max: i32,
    /// (choice id, label, price for the requested currency+cycle; None = free)
    pub choices: Vec<(Uuid, String, Option<Decimal>)>,
}

/// Pure pricing of a selection: returns (recurring delta, line labels).
/// Quantity options multiply the per-unit price; yes/no adds the price only
/// when its choice is selected. Unknown options or choices are rejected.
pub fn price_config(
    opts: &[PricedOption],
    sel: &HashMap<Uuid, ConfigValue>,
) -> AppResult<(Decimal, Vec<String>)> {
    let bad = || AppError::Validation("error.invalid_option");
    if sel.keys().any(|k| !opts.iter().any(|o| o.id == *k)) {
        return Err(bad());
    }
    let mut total = Decimal::ZERO;
    let mut labels = Vec::new();
    for o in opts {
        let v = sel.get(&o.id);
        if o.option_type == "quantity" {
            let qty = match v {
                Some(ConfigValue::Qty(q)) => *q,
                None if !o.required => 0,
                None => o.qty_min.max(0) as u32,
                Some(ConfigValue::Choice(_)) => return Err(bad()),
            };
            let skip = qty == 0 && !o.required; // optional quantity left at zero
            if !skip && ((qty as i64) < o.qty_min as i64 || (qty as i64) > o.qty_max as i64) {
                return Err(bad());
            }
            let (_, label, price) = o.choices.first().ok_or_else(bad)?;
            if qty > 0 {
                total += price.unwrap_or(Decimal::ZERO) * Decimal::from(qty);
                labels.push(format!("{}: {qty} × {label}", o.name));
            }
            continue;
        }
        match v {
            Some(ConfigValue::Choice(cid)) => {
                let (_, label, price) = o.choices.iter().find(|c| c.0 == *cid).ok_or_else(bad)?;
                total += price.unwrap_or(Decimal::ZERO);
                labels.push(if o.option_type == "yesno" {
                    o.name.clone()
                } else {
                    label.clone()
                });
            }
            Some(ConfigValue::Qty(_)) => return Err(bad()),
            None if o.required && o.option_type != "yesno" => return Err(bad()),
            None => {}
        }
    }
    Ok((total, labels))
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct ChoiceInput {
    pub label: String,
    #[serde(default)]
    pub prices: Vec<ChoicePrice>,
}

pub async fn add_option(pool: &PgPool, product_id: Uuid, o: &OptionInput) -> AppResult<Uuid> {
    check_name(&o.name, 120)?;
    if o.choices.is_empty()
        || o.choices.len() > 100
        || !OPTION_TYPES.contains(&o.option_type.as_str())
    {
        return Err(AppError::Validation("error.validation"));
    }
    if matches!(o.option_type.as_str(), "quantity" | "yesno") && o.choices.len() != 1 {
        return Err(AppError::Validation("error.validation"));
    }
    if o.qty_min < 0 || o.qty_max < o.qty_min || o.qty_max > 100_000 {
        return Err(AppError::Validation("error.validation"));
    }
    let cur = currencies(pool).await?;
    for c in &o.choices {
        for p in &c.prices {
            check_scale(&cur, &p.currency, &[p.amount])?;
        }
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let oid = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO product_config_options (id, product_id, name, required, option_type, qty_min, qty_max, sort_order)
         VALUES ($1,$2,$3,$4,$5,$6,$7, COALESCE((SELECT max(sort_order) + 1 FROM product_config_options WHERE product_id = $2), 0))",
    )
    .bind(oid)
    .bind(product_id)
    .bind(o.name.trim())
    .bind(o.required)
    .bind(&o.option_type)
    .bind(o.qty_min)
    .bind(o.qty_max)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    for (i, c) in o.choices.iter().enumerate() {
        check_name(&c.label, 120)?;
        let cid = Uuid::new_v4();
        sqlx::query("INSERT INTO product_config_choices (id, option_id, label, sort_order) VALUES ($1,$2,$3,$4)")
            .bind(cid)
            .bind(oid)
            .bind(c.label.trim())
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for p in &c.prices {
            if !pelty_billing::cycle::is_valid(&p.cycle) || p.amount < Decimal::ZERO {
                return Err(AppError::Validation("error.invalid_price"));
            }
            sqlx::query("INSERT INTO product_config_prices (choice_id, currency, cycle, amount) VALUES ($1,$2,$3,$4)")
                .bind(cid)
                .bind(p.currency.to_ascii_uppercase())
                .bind(&p.cycle)
                .bind(p.amount)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(oid)
}

/// Products with prices and options. `public` hides non-active products and
/// hidden groups. Always 4 queries regardless of catalog size.
pub async fn products(
    pool: &PgPool,
    public: bool,
    only: Option<Uuid>,
) -> AppResult<Vec<ProductFull>> {
    let products: Vec<Product> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT p.{} FROM products p JOIN product_groups g ON g.id = p.group_id
         WHERE ($1 = FALSE OR (p.status = 'active' AND NOT g.hidden)) AND ($2::uuid IS NULL OR p.id = $2)
         ORDER BY g.sort_order, p.sort_order, p.name",
        PCOLS.replace(", ", ", p.")
    )))
    .bind(public)
    .bind(only)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let ids: Vec<Uuid> = products.iter().map(|p| p.id).collect();
    let prices: Vec<Price> = sqlx::query_as(
        "SELECT product_id, currency::text AS currency, cycle, amount, setup_fee FROM product_prices
         WHERE product_id = ANY($1) ORDER BY currency, array_position(ARRAY['monthly','quarterly','semiannual','annual','biennial','triennial','one_time'], cycle)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let opts: Vec<(Uuid, Uuid, String, bool, String, i32, i32)> = sqlx::query_as(
        "SELECT product_id, id, name, required, option_type, qty_min, qty_max FROM product_config_options WHERE product_id = ANY($1) ORDER BY sort_order, name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let oids: Vec<Uuid> = opts.iter().map(|o| o.1).collect();
    let choices: Vec<(Uuid, Uuid, String, serde_json::Value)> = sqlx::query_as(
        "SELECT c.option_id, c.id, c.label,
                COALESCE((SELECT json_agg(json_build_object('currency', cp.currency, 'cycle', cp.cycle, 'amount', cp.amount::text))
                          FROM product_config_prices cp WHERE cp.choice_id = c.id), '[]'::json)::jsonb
         FROM product_config_choices c WHERE c.option_id = ANY($1) ORDER BY c.sort_order",
    )
    .bind(&oids)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut ch_by_opt: HashMap<Uuid, Vec<ConfigChoice>> = HashMap::new();
    for (oid, id, label, prices) in choices {
        let prices = serde_json::from_value(prices).unwrap_or_default();
        ch_by_opt
            .entry(oid)
            .or_default()
            .push(ConfigChoice { id, label, prices });
    }
    let mut opt_by_prod: HashMap<Uuid, Vec<ConfigOption>> = HashMap::new();
    for (pid, id, name, required, option_type, qty_min, qty_max) in opts {
        opt_by_prod.entry(pid).or_default().push(ConfigOption {
            id,
            name,
            required,
            option_type,
            qty_min,
            qty_max,
            choices: ch_by_opt.remove(&id).unwrap_or_default(),
        });
    }
    let mut pr_by_prod: HashMap<Uuid, Vec<Price>> = HashMap::new();
    for p in prices {
        if let Some(id) = p.product_id {
            pr_by_prod.entry(id).or_default().push(p);
        }
    }
    Ok(products
        .into_iter()
        .map(|p| ProductFull {
            prices: pr_by_prod.remove(&p.id).unwrap_or_default(),
            options: opt_by_prod.remove(&p.id).unwrap_or_default(),
            product: p,
        })
        .collect())
}

/// A priced cart line.
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub product_id: Uuid,
    pub product_name: String,
    pub cycle: String,
    pub amount: Decimal,
    pub setup_fee: Decimal,
    pub config: serde_json::Value,
    pub labels: Vec<String>,
}

/// Price one line inside a transaction (reads current catalog prices; the
/// client never supplies amounts). Validates every required option.
pub async fn quote(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    cycle: &str,
    currency: &str,
    config: &HashMap<Uuid, ConfigValue>,
) -> AppResult<Quote> {
    let row: Option<(String, String, Decimal, Decimal)> = sqlx::query_as(
        "SELECT p.name, p.status, pp.amount, pp.setup_fee FROM products p
         JOIN product_groups g ON g.id = p.group_id
         JOIN product_prices pp ON pp.product_id = p.id AND pp.currency = $2 AND pp.cycle = $3
         WHERE p.id = $1 AND NOT g.hidden",
    )
    .bind(product_id)
    .bind(currency)
    .bind(cycle)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let Some((name, status, mut amount, setup_fee)) = row else {
        return Err(AppError::Validation("error.product_unavailable"));
    };
    if status != "active" {
        return Err(AppError::Validation("error.product_unavailable"));
    }
    type OptRow = (Uuid, String, bool, String, i32, i32);
    let rows: Vec<OptRow> = sqlx::query_as(
        "SELECT id, name, required, option_type, qty_min, qty_max FROM product_config_options
         WHERE product_id = $1 ORDER BY sort_order, name",
    )
    .bind(product_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    let choices: Vec<(Uuid, Uuid, String, Option<Decimal>)> = sqlx::query_as(
        "SELECT c.option_id, c.id, c.label, cp.amount FROM product_config_choices c
         LEFT JOIN product_config_prices cp ON cp.choice_id = c.id AND cp.currency = $2 AND cp.cycle = $3
         WHERE c.option_id = ANY($1) ORDER BY c.sort_order",
    )
    .bind(&ids)
    .bind(currency)
    .bind(cycle)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let opts: Vec<PricedOption> = rows
        .into_iter()
        .map(
            |(id, name, required, option_type, qty_min, qty_max)| PricedOption {
                choices: choices
                    .iter()
                    .filter(|c| c.0 == id)
                    .map(|c| (c.1, c.2.clone(), c.3))
                    .collect(),
                id,
                name,
                required,
                option_type,
                qty_min,
                qty_max,
            },
        )
        .collect();
    let (delta, labels) = price_config(&opts, config)?;
    amount += delta;
    Ok(Quote {
        product_id,
        product_name: name,
        cycle: cycle.into(),
        amount,
        setup_fee,
        config: serde_json::to_value(config).unwrap_or_default(),
        labels,
    })
}

/// Enabled currencies and their exponents (minor-unit digits).
pub async fn currencies(pool: &PgPool) -> AppResult<HashMap<String, u32>> {
    let rows: Vec<(String, i16)> =
        sqlx::query_as("SELECT code::text, exponent FROM currencies WHERE enabled")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(c, e)| (c, e.max(0) as u32))
        .collect())
}

/// Reject amounts with more decimals than the currency allows.
pub fn check_scale(
    cur: &HashMap<String, u32>,
    currency: &str,
    amounts: &[Decimal],
) -> AppResult<()> {
    let e = *cur
        .get(&currency.to_ascii_uppercase())
        .ok_or(AppError::Validation("error.invalid_currency"))?;
    if amounts.iter().any(|a| a.normalize().scale() > e) {
        return Err(AppError::Validation("error.invalid_price"));
    }
    Ok(())
}

/// Remove one option (and its choices/prices) from a product.
pub async fn delete_option(pool: &PgPool, product_id: Uuid, option_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM product_config_options WHERE id = $1 AND product_id = $2")
        .bind(option_id)
        .bind(product_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod config_tests {
    use super::*;
    use std::str::FromStr;
    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn opt(t: &str, required: bool, choices: Vec<(Uuid, &str, Option<Decimal>)>) -> PricedOption {
        PricedOption {
            id: Uuid::new_v4(),
            name: "Opt".into(),
            required,
            option_type: t.into(),
            qty_min: 1,
            qty_max: 8,
            choices: choices
                .into_iter()
                .map(|(a, b, c)| (a, b.into(), c))
                .collect(),
        }
    }

    #[test]
    fn prices_each_type() {
        let (c1, c2, ram, yes) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let dd = opt(
            "dropdown",
            true,
            vec![(c1, "A", None), (c2, "B", Some(dec("2.50")))],
        );
        let q = opt("quantity", true, vec![(ram, "GB", Some(dec("1.2500")))]);
        let yn = opt("yesno", false, vec![(yes, "Yes", Some(dec("3")))]);
        let opts = vec![dd.clone(), q.clone(), yn.clone()];
        let sel = HashMap::from([
            (dd.id, ConfigValue::Choice(c2)),
            (q.id, ConfigValue::Qty(4)),
            (yn.id, ConfigValue::Choice(yes)),
        ]);
        let (t, labels) = price_config(&opts, &sel).unwrap();
        assert_eq!(t, dec("10.50"));
        assert_eq!(labels.len(), 3);
        // yes/no omitted = no; quantity defaults to min when required.
        let sel = HashMap::from([(dd.id, ConfigValue::Choice(c1))]);
        assert_eq!(price_config(&opts, &sel).unwrap().0, dec("1.25"));
    }

    #[test]
    fn rejects_bad_input() {
        let c = Uuid::new_v4();
        let dd = opt("dropdown", true, vec![(c, "A", None)]);
        let q = opt(
            "quantity",
            false,
            vec![(Uuid::new_v4(), "GB", Some(dec("1")))],
        );
        let opts = vec![dd.clone(), q.clone()];
        // missing required dropdown
        assert!(price_config(&opts, &HashMap::new()).is_err());
        // out of range quantity
        let s = HashMap::from([(dd.id, ConfigValue::Choice(c)), (q.id, ConfigValue::Qty(9))]);
        assert!(price_config(&opts, &s).is_err());
        // wrong value kind, unknown choice, unknown option
        let s = HashMap::from([(dd.id, ConfigValue::Qty(1))]);
        assert!(price_config(&opts, &s).is_err());
        let s = HashMap::from([(dd.id, ConfigValue::Choice(Uuid::new_v4()))]);
        assert!(price_config(&opts, &s).is_err());
        let s = HashMap::from([
            (dd.id, ConfigValue::Choice(c)),
            (Uuid::new_v4(), ConfigValue::Qty(1)),
        ]);
        assert!(price_config(&opts, &s).is_err());
        // optional quantity omitted = 0
        let s = HashMap::from([(dd.id, ConfigValue::Choice(c))]);
        assert_eq!(price_config(&opts, &s).unwrap().0, Decimal::ZERO);
    }

    #[test]
    fn untagged_value_parses() {
        let id = Uuid::new_v4();
        let v: ConfigValue = serde_json::from_value(serde_json::json!(id.to_string())).unwrap();
        assert_eq!(v, ConfigValue::Choice(id));
        let v: ConfigValue = serde_json::from_value(serde_json::json!(3)).unwrap();
        assert_eq!(v, ConfigValue::Qty(3));
        assert!(serde_json::from_value::<ConfigValue>(serde_json::json!(-1)).is_err());
    }
}
