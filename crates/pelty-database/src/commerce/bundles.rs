//! Bundles and upsell / cross-sell recommendations.
use crate::db_err;
use crate::orders::CartItem;
use pelty_billing::cycle;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub const MAX_ITEMS: usize = 10;
pub const MAX_RECS: usize = 6;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct BundleItem {
    pub product_id: Uuid,
    pub product_name: String,
    pub cycle: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bundle {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub active: bool,
    pub discount_percent: Decimal,
    pub items: Vec<BundleItem>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct ItemInput {
    pub product_id: Uuid,
    pub cycle: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct BundleInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "yes")]
    pub active: bool,
    #[schema(value_type = String)]
    pub discount_percent: Decimal,
    pub items: Vec<ItemInput>,
}

fn yes() -> bool {
    true
}

pub fn validate(b: &BundleInput) -> AppResult<()> {
    let name = b.name.trim();
    if name.is_empty() || name.len() > 200 || b.description.len() > 2000 {
        return Err(AppError::Validation("error.invalid_bundle"));
    }
    if b.discount_percent < Decimal::ZERO
        || b.discount_percent > Decimal::ONE_HUNDRED
        || b.discount_percent.scale() > 2
    {
        return Err(AppError::Validation("error.invalid_bundle"));
    }
    if b.items.len() < 2
        || b.items.len() > MAX_ITEMS
        || b.items.iter().any(|i| !cycle::is_valid(&i.cycle))
    {
        return Err(AppError::Validation("error.invalid_bundle"));
    }
    let mut ids: Vec<Uuid> = b.items.iter().map(|i| i.product_id).collect();
    ids.sort();
    ids.dedup();
    if ids.len() != b.items.len() {
        return Err(AppError::Validation("error.invalid_bundle"));
    }
    Ok(())
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    name: String,
    description: String,
    active: bool,
    discount_percent: Decimal,
}

pub async fn list(pool: &PgPool, active_only: bool) -> AppResult<Vec<Bundle>> {
    let rows: Vec<Row> = sqlx::query_as("SELECT id, name, description, active, discount_percent FROM bundles WHERE ($1 = false OR active) ORDER BY name")
        .bind(active_only).fetch_all(pool).await.map_err(db_err)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let items: Vec<BundleItem> = sqlx::query_as(
            "SELECT bi.product_id, p.name AS product_name, bi.cycle FROM bundle_items bi JOIN products p ON p.id = bi.product_id
             WHERE bi.bundle_id = $1 AND ($2 = false OR p.status = 'active') ORDER BY bi.position, p.name",
        )
        .bind(r.id).bind(active_only).fetch_all(pool).await.map_err(db_err)?;
        // A bundle with a retired product is not orderable.
        if active_only && items.len() < 2 {
            continue;
        }
        out.push(Bundle {
            id: r.id,
            name: r.name,
            description: r.description,
            active: r.active,
            discount_percent: r.discount_percent,
            items,
        });
    }
    Ok(out)
}

pub async fn get(pool: &PgPool, id: Uuid, active_only: bool) -> AppResult<Option<Bundle>> {
    Ok(list(pool, active_only)
        .await?
        .into_iter()
        .find(|b| b.id == id))
}

async fn write_items(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    items: &[ItemInput],
) -> AppResult<()> {
    sqlx::query("DELETE FROM bundle_items WHERE bundle_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    for (n, i) in items.iter().enumerate() {
        sqlx::query("INSERT INTO bundle_items (bundle_id, product_id, cycle, position) VALUES ($1,$2,$3,$4)")
            .bind(id).bind(i.product_id).bind(&i.cycle).bind(n as i32)
            .execute(&mut **tx).await
            .map_err(|e| match &e {
                sqlx::Error::Database(d) if d.is_foreign_key_violation() => AppError::Validation("error.invalid_bundle"),
                _ => db_err(e),
            })?;
    }
    Ok(())
}

pub async fn create(pool: &PgPool, b: &BundleInput) -> AppResult<Uuid> {
    validate(b)?;
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("INSERT INTO bundles (id, name, description, active, discount_percent) VALUES ($1,$2,$3,$4,$5)")
        .bind(id).bind(b.name.trim()).bind(&b.description).bind(b.active).bind(b.discount_percent)
        .execute(&mut *tx).await.map_err(db_err)?;
    write_items(&mut tx, id, &b.items).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

pub async fn update(pool: &PgPool, id: Uuid, b: &BundleInput) -> AppResult<()> {
    validate(b)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("UPDATE bundles SET name = $2, description = $3, active = $4, discount_percent = $5 WHERE id = $1")
        .bind(id).bind(b.name.trim()).bind(&b.description).bind(b.active).bind(b.discount_percent)
        .execute(&mut *tx).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    write_items(&mut tx, id, &b.items).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM bundles WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Cart indices covering every bundle item (one cart line per item, same
/// product and cycle), or None when the cart doesn't contain the bundle.
pub fn match_cart(b: &Bundle, cart: &[CartItem]) -> Option<Vec<usize>> {
    let mut used = vec![false; cart.len()];
    let mut out = Vec::with_capacity(b.items.len());
    for bi in &b.items {
        let i = cart
            .iter()
            .enumerate()
            .position(|(i, c)| !used[i] && c.product_id == bi.product_id && c.cycle == bi.cycle)?;
        used[i] = true;
        out.push(i);
    }
    Some(out)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Recommendations {
    #[serde(default)]
    pub upsell: Vec<Uuid>,
    #[serde(default)]
    pub cross_sell: Vec<Uuid>,
}

/// Recommendations for a product; `public` hides non-active targets.
pub async fn recommendations(
    pool: &PgPool,
    product: Uuid,
    public: bool,
) -> AppResult<Recommendations> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT r.target_id, r.kind FROM product_recommendations r JOIN products p ON p.id = r.target_id
         WHERE r.product_id = $1 AND ($2 = false OR p.status = 'active') ORDER BY r.kind, r.position",
    )
    .bind(product).bind(public).fetch_all(pool).await.map_err(db_err)?;
    let mut out = Recommendations::default();
    for (id, kind) in rows {
        if kind == "upsell" {
            out.upsell.push(id)
        } else {
            out.cross_sell.push(id)
        }
    }
    Ok(out)
}

pub async fn set_recommendations(
    pool: &PgPool,
    product: Uuid,
    r: &Recommendations,
) -> AppResult<()> {
    let bad = |v: &[Uuid]| {
        let mut s = v.to_vec();
        s.sort();
        s.dedup();
        v.len() > MAX_RECS || s.len() != v.len() || v.contains(&product)
    };
    if bad(&r.upsell) || bad(&r.cross_sell) {
        return Err(AppError::Validation("error.invalid_recommendations"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM product_recommendations WHERE product_id = $1")
        .bind(product)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for (kind, ids) in [("upsell", &r.upsell), ("cross_sell", &r.cross_sell)] {
        for (n, t) in ids.iter().enumerate() {
            sqlx::query("INSERT INTO product_recommendations (product_id, target_id, kind, position) VALUES ($1,$2,$3,$4)")
                .bind(product).bind(t).bind(kind).bind(n as i32)
                .execute(&mut *tx).await
                .map_err(|e| match &e {
                    sqlx::Error::Database(d) if d.is_foreign_key_violation() => AppError::Validation("error.invalid_recommendations"),
                    _ => db_err(e),
                })?;
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cart(p: Uuid, c: &str) -> CartItem {
        serde_json::from_value(serde_json::json!({ "product_id": p, "cycle": c })).unwrap()
    }

    #[test]
    fn matches_bundle_lines() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let bundle = Bundle {
            id: Uuid::nil(),
            name: "x".into(),
            description: String::new(),
            active: true,
            discount_percent: Decimal::TEN,
            items: vec![
                BundleItem {
                    product_id: a,
                    product_name: "A".into(),
                    cycle: "monthly".into(),
                },
                BundleItem {
                    product_id: b,
                    product_name: "B".into(),
                    cycle: "annual".into(),
                },
            ],
        };
        assert_eq!(
            match_cart(&bundle, &[cart(b, "annual"), cart(a, "monthly")]),
            Some(vec![1, 0])
        );
        assert_eq!(
            match_cart(&bundle, &[cart(a, "monthly"), cart(b, "monthly")]),
            None
        );
        assert_eq!(match_cart(&bundle, &[cart(a, "monthly")]), None);
    }

    #[test]
    fn validates_input() {
        let a = Uuid::new_v4();
        let mk = |items: Vec<ItemInput>, pct: &str| BundleInput {
            name: "B".into(),
            description: String::new(),
            active: true,
            discount_percent: pct.parse().unwrap(),
            items,
        };
        let it = |p: Uuid| ItemInput {
            product_id: p,
            cycle: "monthly".into(),
        };
        assert!(validate(&mk(vec![it(a), it(Uuid::new_v4())], "10")).is_ok());
        assert!(validate(&mk(vec![it(a), it(a)], "10")).is_err());
        assert!(validate(&mk(vec![it(a)], "10")).is_err());
        assert!(validate(&mk(vec![it(a), it(Uuid::new_v4())], "100.5")).is_err());
    }
}
