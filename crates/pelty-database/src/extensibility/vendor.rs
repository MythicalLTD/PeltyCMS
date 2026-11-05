//! MarketConnect-style vendor resale: a vendor server's catalog is synced
//! here, staff import items as normal products (module + server + price =
//! cost + markup), and orders run through the normal provisioning lifecycle.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SyncItem {
    pub sku: String,
    pub name: String,
    pub description: String,
    pub cost: Decimal,
    pub currency: String,
    pub cycle: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Item {
    pub id: Uuid,
    pub server_id: Uuid,
    pub server_name: String,
    pub module: String,
    pub sku: String,
    pub name: String,
    pub description: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub cost: Decimal,
    pub currency: String,
    pub cycle: String,
    pub available: bool,
    pub product_id: Option<Uuid>,
    pub product_name: Option<String>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub price: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub markup_percent: Option<Decimal>,
    #[serde(with = "time::serde::rfc3339")]
    pub synced_at: OffsetDateTime,
}

/// Sale price = cost × (1 + markup/100), rounded half away from zero to the
/// currency exponent (pure; unit-tested).
pub fn sale_price(cost: Decimal, markup_percent: Decimal, exponent: u32) -> AppResult<Decimal> {
    if markup_percent < Decimal::from(-100) || markup_percent > Decimal::from(10_000) {
        return Err(AppError::Validation("error.invalid_markup"));
    }
    let p = cost * (Decimal::ONE + markup_percent / Decimal::from(100));
    Ok(p.round_dp_with_strategy(exponent, RoundingStrategy::MidpointAwayFromZero).max(Decimal::ZERO))
}

/// Upsert the vendor's catalog; SKUs no longer offered become unavailable
/// (linked products are kept). Returns (upserted, retired).
pub async fn sync(pool: &PgPool, server: Uuid, items: &[SyncItem]) -> AppResult<(usize, u64)> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    for i in items {
        if !pelty_billing::cycle::is_valid(&i.cycle) || i.cost < Decimal::ZERO {
            continue;
        }
        sqlx::query(
            "INSERT INTO vendor_catalog (id, server_id, sku, name, description, cost, currency, cycle, config, available, synced_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,true,now())
             ON CONFLICT (server_id, sku) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description, cost = EXCLUDED.cost,
               currency = EXCLUDED.currency, cycle = EXCLUDED.cycle, config = EXCLUDED.config, available = true, synced_at = now()",
        )
        .bind(Uuid::new_v4())
        .bind(server)
        .bind(&i.sku)
        .bind(&i.name)
        .bind(&i.description)
        .bind(i.cost)
        .bind(&i.currency)
        .bind(&i.cycle)
        .bind(&i.config)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    let skus: Vec<&str> = items.iter().map(|i| i.sku.as_str()).collect();
    let r = sqlx::query("UPDATE vendor_catalog SET available = false WHERE server_id = $1 AND available AND NOT (sku = ANY($2))")
        .bind(server)
        .bind(&skus)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((items.len(), r.rows_affected()))
}

const SELECT: &str = "SELECT v.id, v.server_id, s.name AS server_name, s.module, v.sku, v.name, v.description, v.cost, v.currency::text AS currency, v.cycle,
        v.available, v.product_id, p.name AS product_name, pp.amount AS price, v.markup_percent, v.synced_at
   FROM vendor_catalog v JOIN servers s ON s.id = v.server_id
   LEFT JOIN products p ON p.id = v.product_id
   LEFT JOIN product_prices pp ON pp.product_id = v.product_id AND pp.currency = v.currency AND pp.cycle = v.cycle";

pub async fn list(pool: &PgPool, server: Option<Uuid>) -> AppResult<Vec<Item>> {
    sqlx::query_as(&format!("{SELECT} WHERE ($1::uuid IS NULL OR v.server_id = $1) ORDER BY s.name, v.name, v.sku"))
        .bind(server)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Item> {
    sqlx::query_as(&format!("{SELECT} WHERE v.id = $1")).bind(id).fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound)
}

async fn exponent(pool: &PgPool, currency: &str) -> AppResult<u32> {
    let e: Option<i16> = sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1 AND enabled").bind(currency).fetch_optional(pool).await.map_err(db_err)?;
    e.map(|e| e as u32).ok_or(AppError::Validation("error.currency_disabled"))
}

/// Create a product for a catalog item: module + server + config copied,
/// one price (cost + markup) in the vendor's currency/cycle.
pub async fn import(pool: &PgPool, id: Uuid, group: Uuid, markup: Decimal, name: Option<&str>) -> AppResult<Uuid> {
    let it = get(pool, id).await?;
    if it.product_id.is_some() {
        return Err(AppError::Conflict("error.vendor_already_imported"));
    }
    if !it.available {
        return Err(AppError::Conflict("error.vendor_unavailable"));
    }
    let price = sale_price(it.cost, markup, exponent(pool, &it.currency).await?)?;
    let config: serde_json::Value = sqlx::query_scalar("SELECT config FROM vendor_catalog WHERE id = $1").bind(id).fetch_one(pool).await.map_err(db_err)?;
    let slug_base: String = it.sku.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let slug = format!("{}-{}", slug_base.trim_matches('-'), &Uuid::new_v4().simple().to_string()[..6]);
    let input = crate::catalog::ProductInput {
        group_id: Some(group),
        name: Some(name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&it.name).to_string()),
        slug: Some(slug),
        description: Some(it.description.clone()),
        status: Some("active".into()),
        module: Some(it.module.clone()),
        module_config: Some(config.clone()),
        sort_order: None,
    };
    let p = crate::catalog::create_product(pool, &input).await?;
    crate::provisioning::set_product_module(pool, p.id, Some(&it.module), Some(it.server_id), &config).await?;
    crate::catalog::set_prices(pool, p.id, &[crate::catalog::Price { product_id: Some(p.id), currency: it.currency.clone(), cycle: it.cycle.clone(), amount: price, setup_fee: Decimal::ZERO }]).await?;
    let r = sqlx::query("UPDATE vendor_catalog SET product_id = $2, markup_percent = $3 WHERE id = $1 AND product_id IS NULL").bind(id).bind(p.id).bind(markup).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.vendor_already_imported"));
    }
    Ok(p.id)
}

/// Re-apply a markup to the linked product's vendor-currency price (other
/// prices staff added by hand are left alone). Returns the new price.
pub async fn reprice(pool: &PgPool, id: Uuid, markup: Decimal) -> AppResult<Decimal> {
    let it = get(pool, id).await?;
    let pid = it.product_id.ok_or(AppError::Conflict("error.vendor_not_imported"))?;
    let price = sale_price(it.cost, markup, exponent(pool, &it.currency).await?)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query(
        "INSERT INTO product_prices (id, product_id, currency, cycle, amount, setup_fee) VALUES ($1,$2,$3,$4,$5,0)
         ON CONFLICT (product_id, currency, cycle) DO UPDATE SET amount = EXCLUDED.amount",
    )
    .bind(Uuid::new_v4())
    .bind(pid)
    .bind(&it.currency)
    .bind(&it.cycle)
    .bind(price)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query("UPDATE vendor_catalog SET markup_percent = $2 WHERE id = $1").bind(id).bind(markup).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(price)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markup_rounding() {
        let d = |s: &str| s.parse::<Decimal>().unwrap();
        assert_eq!(sale_price(d("4.50"), d("40"), 2).unwrap(), d("6.30"));
        assert_eq!(sale_price(d("3.333"), d("0"), 2).unwrap(), d("3.33"));
        assert_eq!(sale_price(d("0.125"), d("0"), 2).unwrap(), d("0.13"));
        assert_eq!(sale_price(d("1500"), d("12.5"), 0).unwrap(), d("1688"));
        assert_eq!(sale_price(d("10"), d("-100"), 2).unwrap(), d("0"));
        assert!(sale_price(d("10"), d("-101"), 2).is_err());
    }
}
