//! Timed sales (module `timed_sales`): percent off products or whole groups
//! inside a time window, applied without a code to new orders' first invoice.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "timed_sales";

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = TimedSale)]
pub struct Sale {
    pub id: Uuid,
    pub name: String,
    #[schema(value_type = String)]
    pub percent: Decimal,
    pub product_ids: Vec<Uuid>,
    pub group_ids: Vec<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub starts_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ends_at: OffsetDateTime,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = TimedSaleInput)]
pub struct SaleInput {
    pub name: String,
    #[schema(value_type = String)]
    pub percent: Decimal,
    #[serde(default)]
    pub product_ids: Vec<Uuid>,
    #[serde(default)]
    pub group_ids: Vec<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub starts_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ends_at: OffsetDateTime,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl SaleInput {
    pub fn check(&self) -> AppResult<()> {
        let n = self.product_ids.len() + self.group_ids.len();
        if self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.percent <= Decimal::ZERO
            || self.percent > Decimal::from(95)
            || self.percent.scale() > 2
            || self.ends_at <= self.starts_at
            || !(1..=500).contains(&n)
        {
            return Err(AppError::Validation("error.sale_invalid"));
        }
        Ok(())
    }
}

const COLS: &str = "id, name, percent, product_ids, group_ids, starts_at, ends_at, enabled";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Sale>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM sales ORDER BY starts_at DESC, name"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Sales running right now.
pub async fn active(pool: &PgPool) -> AppResult<Vec<Sale>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM sales WHERE enabled AND starts_at <= now() AND ends_at > now() ORDER BY percent DESC, name"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create(pool: &PgPool, i: &SaleInput) -> AppResult<Sale> {
    i.check()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO sales (name, percent, product_ids, group_ids, starts_at, ends_at, enabled)
         VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {COLS}"
    )))
    .bind(i.name.trim())
    .bind(i.percent)
    .bind(&i.product_ids)
    .bind(&i.group_ids)
    .bind(i.starts_at)
    .bind(i.ends_at)
    .bind(i.enabled)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, i: &SaleInput) -> AppResult<Sale> {
    i.check()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE sales SET name=$2, percent=$3, product_ids=$4, group_ids=$5, starts_at=$6, ends_at=$7, enabled=$8, updated_at=now()
         WHERE id=$1 RETURNING {COLS}"
    )))
    .bind(id)
    .bind(i.name.trim())
    .bind(i.percent)
    .bind(&i.product_ids)
    .bind(&i.group_ids)
    .bind(i.starts_at)
    .bind(i.ends_at)
    .bind(i.enabled)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    Ok(sqlx::query("DELETE FROM sales WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected()
        > 0)
}

/// The best running sale for a product in `group` (highest percent wins;
/// sales never stack with each other).
pub fn best(sales: &[Sale], product: Uuid, group: Uuid) -> Option<&Sale> {
    sales
        .iter()
        .filter(|s| s.product_ids.contains(&product) || s.group_ids.contains(&group))
        .max_by(|a, b| a.percent.cmp(&b.percent))
}

/// Product → group for the given products.
pub async fn groups(pool: &PgPool, products: &[Uuid]) -> AppResult<Vec<(Uuid, Uuid)>> {
    sqlx::query_as("SELECT id, group_id FROM products WHERE id = ANY($1)")
        .bind(products)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    fn sale(pct: i64, products: Vec<Uuid>, groups: Vec<Uuid>) -> Sale {
        let now = OffsetDateTime::now_utc();
        Sale {
            id: Uuid::new_v4(),
            name: format!("{pct}%"),
            percent: Decimal::from(pct),
            product_ids: products,
            group_ids: groups,
            starts_at: now - Duration::hours(1),
            ends_at: now + Duration::hours(1),
            enabled: true,
        }
    }

    #[test]
    fn highest_matching_sale_wins() {
        let (p, g, other) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let s = vec![
            sale(10, vec![p], vec![]),
            sale(25, vec![], vec![g]),
            sale(50, vec![other], vec![]),
        ];
        assert_eq!(best(&s, p, g).unwrap().percent, Decimal::from(25));
        assert_eq!(
            best(&s, p, Uuid::new_v4()).unwrap().percent,
            Decimal::from(10)
        );
        assert!(best(&s, Uuid::new_v4(), Uuid::new_v4()).is_none());
    }

    #[test]
    fn input_checks() {
        let now = OffsetDateTime::now_utc();
        let mut i = SaleInput {
            name: "Weekend".into(),
            percent: Decimal::from(20),
            product_ids: vec![Uuid::new_v4()],
            group_ids: vec![],
            starts_at: now,
            ends_at: now + Duration::days(2),
            enabled: true,
        };
        assert!(i.check().is_ok());
        i.percent = Decimal::from(96);
        assert!(i.check().is_err());
        i.percent = Decimal::new(12345, 3);
        assert!(i.check().is_err());
        i.percent = Decimal::from(20);
        i.ends_at = now;
        assert!(i.check().is_err());
        i.ends_at = now + Duration::days(1);
        i.product_ids.clear();
        assert!(i.check().is_err());
    }
}
