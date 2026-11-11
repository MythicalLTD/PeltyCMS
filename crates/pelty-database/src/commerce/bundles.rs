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

// ------------------------------------------------------- bundle ownership

/// Module `bundle_ownership`: buyers who already own some bundle items buy
/// the rest at the bundle price minus what they own.
pub const OWNERSHIP_MODULE: &str = "bundle_ownership";
pub const OWNERSHIP_MODES: &[&str] = &["share", "list"];

/// `share`: an owned item takes its share of the bundle price off (the rest
/// keeps the bundle percent). `list`: an owned item takes its full list price
/// off the bundle price. `min_percent`: the rest never costs less than this
/// percent of its list price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OwnershipSettings {
    pub mode: String,
    #[schema(value_type = String)]
    pub min_percent: Decimal,
}

impl Default for OwnershipSettings {
    fn default() -> Self {
        Self {
            mode: "share".into(),
            min_percent: Decimal::ZERO,
        }
    }
}

/// Tolerant parse: unknown modes fall back to `share`, the floor is clamped
/// to 0..=100.
pub fn parse_ownership(v: &serde_json::Value) -> OwnershipSettings {
    let mut s = OwnershipSettings::default();
    if let Some(m) = v.get("mode").and_then(|m| m.as_str()) {
        if OWNERSHIP_MODES.contains(&m) {
            s.mode = m.to_string();
        }
    }
    let min = match v.get("min_percent") {
        Some(serde_json::Value::String(t)) => t.parse().ok(),
        Some(serde_json::Value::Number(n)) => n.to_string().parse().ok(),
        _ => None,
    };
    if let Some(m) = min {
        s.min_percent = Decimal::clamp(m, Decimal::ZERO, Decimal::ONE_HUNDRED);
    }
    s
}

/// The settings while the module is on, else None.
pub async fn ownership(pool: &PgPool) -> AppResult<Option<OwnershipSettings>> {
    if !crate::feature_modules::is_enabled(pool, OWNERSHIP_MODULE).await? {
        return Ok(None);
    }
    Ok(Some(parse_ownership(
        &crate::feature_modules::settings(pool, OWNERSHIP_MODULE).await?,
    )))
}

/// Which of `products` the customer owns: an active service, or an active,
/// unexpired license.
pub async fn owned_products(
    pool: &PgPool,
    customer: Uuid,
    products: &[Uuid],
) -> AppResult<std::collections::HashSet<Uuid>> {
    if products.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<Uuid> = sqlx::query_scalar(
        "SELECT p FROM unnest($2::uuid[]) AS p
         WHERE EXISTS (SELECT 1 FROM services s WHERE s.customer_id = $1 AND s.product_id = p AND s.status = 'active')
            OR EXISTS (SELECT 1 FROM software_licenses l WHERE l.customer_id = $1 AND l.product_id = p AND l.status = 'active'
                       AND (l.expires_at IS NULL OR l.expires_at > now()))",
    )
    .bind(customer)
    .bind(products)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

/// Like [`match_cart`], but a bundle item missing from the cart may be one
/// the buyer owns. Returns (cart indices, owned items); None unless at least
/// one item is bought.
pub fn match_owned(
    b: &Bundle,
    cart: &[CartItem],
    owned: &std::collections::HashSet<Uuid>,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut used = vec![false; cart.len()];
    let (mut lines, mut have) = (vec![], vec![]);
    for (n, bi) in b.items.iter().enumerate() {
        match cart
            .iter()
            .enumerate()
            .position(|(i, c)| !used[i] && c.product_id == bi.product_id && c.cycle == bi.cycle)
        {
            Some(i) => {
                used[i] = true;
                lines.push(i);
            }
            None if owned.contains(&bi.product_id) => have.push(n),
            None => return None,
        }
    }
    (!lines.is_empty()).then_some((lines, have))
}

/// Percent off the bought lines (list total `rest`) when items with list
/// total `owned` are already owned. Share mode keeps the bundle percent (the
/// rest costs its share of the bundle price); list mode charges the bundle
/// price minus the owned items' list prices. Both are capped so the rest
/// costs at least `min_percent` of its list price.
pub fn ownership_percent(
    s: &OwnershipSettings,
    percent: Decimal,
    rest: Decimal,
    owned: Decimal,
) -> Decimal {
    let cap = (Decimal::ONE_HUNDRED - s.min_percent).max(Decimal::ZERO);
    let p = if s.mode == "list" && owned > Decimal::ZERO && rest > Decimal::ZERO {
        // pay = (rest + owned)(1 - p) - owned  =>  off = p (rest + owned)
        percent * (rest + owned) / rest
    } else {
        percent
    };
    p.min(cap).max(Decimal::ZERO).round_dp(6)
}

/// List price of one item (product, cycle) in `currency`.
pub async fn list_prices(
    pool: &PgPool,
    currency: &str,
    items: &[(Uuid, String)],
) -> AppResult<Vec<Option<Decimal>>> {
    let (ids, cycles): (Vec<Uuid>, Vec<String>) = items.iter().cloned().unzip();
    let rows: Vec<(i32, Option<Decimal>)> = sqlx::query_as(
        "SELECT t.n::int, pp.amount FROM unnest($1::uuid[], $2::text[]) WITH ORDINALITY AS t(p, c, n)
         LEFT JOIN product_prices pp ON pp.product_id = t.p AND pp.cycle = t.c AND pp.currency = $3
         ORDER BY t.n",
    )
    .bind(&ids)
    .bind(&cycles)
    .bind(currency)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|(_, a)| a).collect())
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use std::collections::HashSet;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }
    fn bundle(n: usize) -> (Bundle, Vec<Uuid>) {
        let ids: Vec<Uuid> = (0..n).map(|_| Uuid::new_v4()).collect();
        let b = Bundle {
            id: Uuid::new_v4(),
            name: "B".into(),
            description: String::new(),
            active: true,
            discount_percent: d("20"),
            items: ids
                .iter()
                .map(|p| BundleItem {
                    product_id: *p,
                    product_name: String::new(),
                    cycle: "one_time".into(),
                })
                .collect(),
        };
        (b, ids)
    }
    fn line(p: Uuid) -> CartItem {
        serde_json::from_value(serde_json::json!({"product_id": p, "cycle": "one_time"})).unwrap()
    }

    #[test]
    fn owned_items_complete_the_bundle() {
        let (b, ids) = bundle(5);
        let cart: Vec<CartItem> = ids[2..].iter().map(|p| line(*p)).collect();
        assert_eq!(match_owned(&b, &cart, &HashSet::new()), None);
        let owned: HashSet<Uuid> = ids[..2].iter().copied().collect();
        assert_eq!(
            match_owned(&b, &cart, &owned),
            Some((vec![0, 1, 2], vec![0, 1]))
        );
        // Something must be bought; one non-owned missing item breaks it.
        assert_eq!(match_owned(&b, &[], &ids.iter().copied().collect()), None);
        assert_eq!(match_owned(&b, &cart[1..], &owned), None);
    }

    #[test]
    fn share_and_list_modes_with_floor() {
        let share = OwnershipSettings::default();
        // 5 items at 10, 20% off, 2 owned: share pays 30 * 0.8 = 24.
        assert_eq!(
            ownership_percent(&share, d("20"), d("30"), d("20")),
            d("20")
        );
        // List: bundle 40 - owned 20 = 20 for the rest (30) => 33.33..%.
        let list = OwnershipSettings {
            mode: "list".into(),
            min_percent: Decimal::ZERO,
        };
        let p = ownership_percent(&list, d("20"), d("30"), d("20"));
        assert_eq!(
            (d("30") * (Decimal::ONE_HUNDRED - p) / Decimal::ONE_HUNDRED).round_dp(2),
            d("20")
        );
        // Floor: the rest costs at least 80% of its list price.
        let floored = OwnershipSettings {
            mode: "list".into(),
            min_percent: d("80"),
        };
        assert_eq!(
            ownership_percent(&floored, d("20"), d("30"), d("20")),
            d("20")
        );
        // Owning nearly everything in list mode is capped at free (100%).
        assert_eq!(
            ownership_percent(&list, d("50"), d("10"), d("90")),
            d("100")
        );
        // Nothing owned: the bundle percent.
        assert_eq!(ownership_percent(&list, d("20"), d("50"), d("0")), d("20"));
    }

    #[test]
    fn settings_parse_tolerantly() {
        assert_eq!(
            parse_ownership(&serde_json::json!({})),
            OwnershipSettings::default()
        );
        let s = parse_ownership(&serde_json::json!({"mode": "list", "min_percent": "150"}));
        assert_eq!(
            (s.mode.as_str(), s.min_percent),
            ("list", Decimal::ONE_HUNDRED)
        );
        let s = parse_ownership(&serde_json::json!({"mode": "bogus", "min_percent": 25}));
        assert_eq!((s.mode.as_str(), s.min_percent), ("share", d("25")));
    }
}
