//! Tier upgrades (module `tier_upgrades`, Tebex "cumulative packages").
//!
//! Packages in a tier group are ranked (VIP < VIP+ < MVP). When the player
//! who receives a line already owns a lower tier of the same group, the line
//! gets an upgrade credit:
//!
//! - `paid`: what was actually paid for the best owned tier, carried through
//!   earlier upgrades (VIP paid 10, VIP+ paid 10 more: VIP+ is worth 20), net
//!   of discounts and of refunds;
//! - `difference`: the best owned tier's list price (scaled down by partial
//!   refunds).
//!
//! The credit never takes the price below the group's minimum. Exactly one
//! owned service is the credit `source` of a line, and a service that is
//! already the source of another live upgrade cannot be credited again, so
//! a credit is never counted twice. A refunded/cancelled order stops being
//! owned and releases the credit it carried.
//!
//! "The player" is the gift recipient (by in-game name) for gift lines and
//! otherwise the buyer: their own services plus anything bought for or by
//! their linked in-game name.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MODULE: &str = "tier_upgrades";
pub const MODES: &[&str] = &["paid", "difference"];
pub const MAX_TIERS: usize = 20;

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
#[schema(as = TierGroupTier)]
pub struct Tier {
    pub product_id: Uuid,
    pub name: String,
    pub rank: i32,
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct TierGroup {
    pub id: Uuid,
    pub name: String,
    pub credit_mode: String,
    #[schema(value_type = String)]
    pub min_price: Decimal,
    pub revoke_old: bool,
    pub cancel_old_recurring: bool,
    #[sqlx(skip)]
    pub tiers: Vec<Tier>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TierGroupInput {
    pub name: String,
    #[serde(default = "paid")]
    pub credit_mode: String,
    #[serde(default)]
    #[schema(value_type = String)]
    pub min_price: Decimal,
    #[serde(default)]
    pub revoke_old: bool,
    #[serde(default = "yes")]
    pub cancel_old_recurring: bool,
    /// Packages from the lowest tier to the highest.
    pub product_ids: Vec<Uuid>,
}

fn paid() -> String {
    "paid".into()
}
fn yes() -> bool {
    true
}

impl TierGroupInput {
    pub fn validate(&self) -> AppResult<()> {
        let mut ids = self.product_ids.clone();
        ids.sort();
        ids.dedup();
        let ok = (1..=100).contains(&self.name.trim().chars().count())
            && MODES.contains(&self.credit_mode.as_str())
            && self.min_price >= Decimal::ZERO
            && self.min_price <= Decimal::from(100_000)
            && self.min_price.scale() <= 4
            && (2..=MAX_TIERS).contains(&self.product_ids.len())
            && ids.len() == self.product_ids.len();
        if ok {
            Ok(())
        } else {
            Err(AppError::Validation("error.tier_group_invalid"))
        }
    }
}

const COLS: &str = "id, name, credit_mode, min_price, revoke_old, cancel_old_recurring";

pub async fn enabled(pool: &PgPool) -> AppResult<bool> {
    crate::feature_modules::is_enabled(pool, MODULE).await
}

async fn tiers_of(pool: &PgPool, group: Uuid) -> AppResult<Vec<Tier>> {
    sqlx::query_as(
        "SELECT gi.product_id, p.name, gi.rank FROM tier_group_items gi JOIN products p ON p.id = gi.product_id
         WHERE gi.group_id = $1 ORDER BY gi.rank",
    )
    .bind(group)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<TierGroup>> {
    let mut out: Vec<TierGroup> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM tier_groups ORDER BY name, id"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for g in &mut out {
        g.tiers = tiers_of(pool, g.id).await?;
    }
    Ok(out)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<TierGroup>> {
    let g: Option<TierGroup> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM tier_groups WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match g {
        Some(mut g) => {
            g.tiers = tiers_of(pool, g.id).await?;
            Ok(Some(g))
        }
        None => Ok(None),
    }
}

/// Create (`id` None) or replace a group and its ranking.
pub async fn save(pool: &PgPool, id: Option<Uuid>, i: &TierGroupInput) -> AppResult<TierGroup> {
    i.validate()?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = match id {
        Some(id) => {
            let n = sqlx::query(
                "UPDATE tier_groups SET name = $2, credit_mode = $3, min_price = $4, revoke_old = $5, cancel_old_recurring = $6, updated_at = now() WHERE id = $1",
            )
            .bind(id)
            .bind(i.name.trim())
            .bind(&i.credit_mode)
            .bind(i.min_price)
            .bind(i.revoke_old)
            .bind(i.cancel_old_recurring)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            if n.rows_affected() == 0 {
                return Err(AppError::NotFound);
            }
            id
        }
        None => sqlx::query_scalar(
            "INSERT INTO tier_groups (name, credit_mode, min_price, revoke_old, cancel_old_recurring) VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(i.name.trim())
        .bind(&i.credit_mode)
        .bind(i.min_price)
        .bind(i.revoke_old)
        .bind(i.cancel_old_recurring)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?,
    };
    sqlx::query("DELETE FROM tier_group_items WHERE group_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for (n, p) in i.product_ids.iter().enumerate() {
        sqlx::query("INSERT INTO tier_group_items (group_id, product_id, rank) VALUES ($1,$2,$3)")
            .bind(id)
            .bind(p)
            .bind(n as i32 + 1)
            .execute(&mut *tx)
            .await
            .map_err(|e| match &e {
                // Unknown package, or one already ranked in another group.
                sqlx::Error::Database(d)
                    if d.is_foreign_key_violation() || d.is_unique_violation() =>
                {
                    AppError::Validation("error.tier_group_invalid")
                }
                _ => db_err(e),
            })?;
    }
    tx.commit().await.map_err(db_err)?;
    get(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM tier_groups WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ------------------------------------------------------------ pricing math

/// One service of the player in a tier group, as read from the database.
#[derive(Debug, Clone, FromRow)]
pub struct ChainRow {
    pub service_id: Uuid,
    pub product_id: Uuid,
    pub rank: i32,
    /// Service is active or suspended (not expired / cancelled / terminated).
    pub active: bool,
    /// Its order is not cancelled/fraud and its invoice not refunded/void.
    pub valid: bool,
    /// Line price after discounts, before tax (first invoice).
    pub net: Decimal,
    /// Upgrade credit the line itself received.
    pub credit: Decimal,
    /// Service whose value that credit came from.
    pub source: Option<Uuid>,
    /// Share of the invoice still paid (0..=1): payments minus refunds.
    pub paid_ratio: Decimal,
    pub currency: String,
    /// Already the credit source of another live upgrade line.
    pub consumed: bool,
}

/// Value each service carries in its own currency: what was paid for it
/// (after refunds) plus the credit it received, but only as much of that
/// credit as its source still carries (a refunded source carries nothing).
pub fn carried(rows: &[ChainRow]) -> HashMap<Uuid, Decimal> {
    fn value(
        id: Uuid,
        by_id: &HashMap<Uuid, &ChainRow>,
        memo: &mut HashMap<Uuid, Decimal>,
        depth: usize,
    ) -> Decimal {
        if let Some(v) = memo.get(&id) {
            return *v;
        }
        let Some(r) = by_id.get(&id) else {
            return Decimal::ZERO;
        };
        // Depth guard: a chain is at most one link per tier.
        if !r.valid || depth > MAX_TIERS + 1 {
            memo.insert(id, Decimal::ZERO);
            return Decimal::ZERO;
        }
        let ratio = r.paid_ratio.clamp(Decimal::ZERO, Decimal::ONE);
        let paid = (r.net.max(Decimal::ZERO)) * ratio;
        let inherited = match r.source {
            Some(src) if src != id => r.credit.min(value(src, by_id, memo, depth + 1)),
            _ => Decimal::ZERO,
        };
        let v = paid + inherited.max(Decimal::ZERO);
        memo.insert(id, v);
        v
    }
    let by_id: HashMap<Uuid, &ChainRow> = rows.iter().map(|r| (r.service_id, r)).collect();
    let mut memo = HashMap::new();
    for r in rows {
        value(r.service_id, &by_id, &mut memo, 0);
    }
    memo
}

/// Lower tiers the player owns that may be credited towards `rank`.
pub fn candidates(rows: &[ChainRow], rank: i32) -> Vec<&ChainRow> {
    rows.iter()
        .filter(|r| r.active && r.valid && r.rank < rank && !r.consumed)
        .collect()
}

/// Lower tiers the upgrade replaces (owned, whether or not credited).
pub fn replaced(rows: &[ChainRow], rank: i32) -> Vec<Uuid> {
    rows.iter()
        .filter(|r| r.active && r.valid && r.rank < rank)
        .map(|r| r.service_id)
        .collect()
}

pub fn round(v: Decimal, exp: u32) -> Decimal {
    v.round_dp_with_strategy(exp, RoundingStrategy::MidpointAwayFromZero)
}

/// The credit for a line priced `price`: the best candidate value, capped so
/// the line still costs at least `floor` (and never below zero). Returns the
/// source service and the credit, or None when nothing is credited.
pub fn credit_amount(
    price: Decimal,
    floor: Decimal,
    values: &[(Uuid, Decimal)],
    exp: u32,
) -> Option<(Uuid, Decimal)> {
    let (src, best) = values
        .iter()
        .copied()
        .filter(|(_, v)| *v > Decimal::ZERO)
        // Highest value; ties broken by id so the choice is stable.
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))?;
    let room = (price - floor.max(Decimal::ZERO)).max(Decimal::ZERO);
    let c = round(best.min(room), exp);
    (c > Decimal::ZERO).then_some((src, c))
}

/// `amount` in a currency with rate `from` expressed in one with rate `to`
/// (rates are units per one default-currency unit).
pub fn convert(amount: Decimal, from: Decimal, to: Decimal, exp: u32) -> Decimal {
    if from <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    round(amount * to / from, exp)
}

// --------------------------------------------------------------- database

/// Credit granted to one cart line.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct LineCredit {
    pub group_id: Uuid,
    #[schema(value_type = String)]
    pub amount: Decimal,
    /// Owned service the credit comes from.
    pub source: Uuid,
    /// Package name of that service ("Upgrade credit from VIP").
    pub from_name: String,
    /// Every lower tier the upgrade replaces.
    #[serde(skip)]
    pub replaces: Vec<Uuid>,
}

const CHAIN: &str = "SELECT s.id AS service_id, s.product_id, gi.rank,
        s.status IN ('active','suspended') AS active,
        (o.status NOT IN ('cancelled','fraud') AND COALESCE(i.status, '') NOT IN ('refunded','void')) AS valid,
        COALESCE(oi.net_amount, oi.amount - oi.discount - oi.bundle_discount) AS net,
        oi.upgrade_credit AS credit,
        (SELECT tu.from_service_id FROM tier_upgrades tu WHERE tu.order_item_id = oi.id AND tu.source) AS source,
        CASE WHEN i.id IS NULL THEN 0::numeric
             WHEN i.total <= 0 THEN CASE WHEN i.status = 'paid' THEN 1::numeric ELSE 0::numeric END
             ELSE LEAST(GREATEST((i.amount_paid - COALESCE((SELECT sum(r.amount) FROM refunds r
                    WHERE r.invoice_id = i.id AND r.status = 'succeeded' AND r.credit_note_id IS NOT NULL), 0)) / i.total, 0), 1)
        END AS paid_ratio,
        s.currency::text AS currency,
        EXISTS (SELECT 1 FROM tier_upgrades tu2 JOIN order_items oi2 ON oi2.id = tu2.order_item_id JOIN orders o2 ON o2.id = oi2.order_id
                WHERE tu2.from_service_id = s.id AND tu2.source AND o2.status NOT IN ('cancelled','fraud')
                  AND NOT (oi2.order_id = ANY($4))) AS consumed
     FROM services s
     JOIN tier_group_items gi ON gi.product_id = s.product_id AND gi.group_id = $1
     JOIN order_items oi ON oi.id = s.order_item_id
     JOIN orders o ON o.id = oi.order_id
     LEFT JOIN invoices i ON i.id = o.invoice_id
     WHERE ($2::uuid IS NOT NULL AND s.customer_id = $2 AND oi.gift_to IS NULL)
        OR (cardinality($3::text[]) > 0 AND oi.gift_to IS NULL AND s.customer_id IN (
              SELECT a.customer_id FROM customer_game_accounts a WHERE a.field = 'username' AND lower(a.value) = ANY($3)))
        OR lower(oi.gift_to) = ANY($3)";

/// The player's services in `group` (see the module docs for "player").
/// Upgrade lines of `ignore_orders` do not mark their source as consumed
/// (re-pricing an order that is being placed).
pub async fn chain(
    tx: &mut Transaction<'_, Postgres>,
    group: Uuid,
    customer: Option<Uuid>,
    names: &[String],
    ignore_orders: &[Uuid],
) -> AppResult<Vec<ChainRow>> {
    sqlx::query_as(CHAIN)
        .bind(group)
        .bind(customer)
        .bind(names)
        .bind(ignore_orders)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)
}

/// Lower-cased in-game names that identify the player of a line.
async fn player(
    tx: &mut Transaction<'_, Postgres>,
    customer: Option<Uuid>,
    gift_to: Option<&str>,
) -> AppResult<(Option<Uuid>, Vec<String>)> {
    if let Some(g) = gift_to.map(str::trim).filter(|g| !g.is_empty()) {
        return Ok((None, vec![g.to_lowercase()]));
    }
    let Some(c) = customer else {
        return Ok((None, vec![]));
    };
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT lower(value) FROM customer_game_accounts WHERE customer_id = $1 AND field = 'username'",
    )
    .bind(c)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok((Some(c), names))
}

async fn rate(tx: &mut Transaction<'_, Postgres>, code: &str) -> AppResult<Option<Decimal>> {
    sqlx::query_scalar("SELECT rate FROM currencies WHERE code = $1")
        .bind(code)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)
}

/// Upgrade credit for one line, or None (module off, package not in a
/// group, nothing lower owned, or the line is already at the minimum).
/// With `lock`, the candidate services are locked for the rest of the
/// transaction so two concurrent orders cannot spend the same credit.
#[allow(clippy::too_many_arguments)]
pub async fn credit_for(
    tx: &mut Transaction<'_, Postgres>,
    customer: Option<Uuid>,
    gift_to: Option<&str>,
    product: Uuid,
    cycle: &str,
    currency: &str,
    price: Decimal,
    exp: u32,
    lock: bool,
) -> AppResult<Option<LineCredit>> {
    let on: bool = sqlx::query_scalar(
        "SELECT COALESCE((SELECT enabled FROM feature_modules WHERE id = $1), false)",
    )
    .bind(MODULE)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    if !on || price <= Decimal::ZERO {
        return Ok(None);
    }
    let g: Option<(Uuid, i32, String, Decimal)> = sqlx::query_as(
        "SELECT g.id, gi.rank, g.credit_mode, g.min_price FROM tier_group_items gi JOIN tier_groups g ON g.id = gi.group_id WHERE gi.product_id = $1",
    )
    .bind(product)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let Some((group, rank, mode, min_price)) = g else {
        return Ok(None);
    };
    let (cust, names) = player(tx, customer, gift_to).await?;
    if cust.is_none() && names.is_empty() {
        return Ok(None);
    }
    let mut rows = chain(tx, group, cust, &names, &[]).await?;
    if lock {
        let ids: Vec<Uuid> = candidates(&rows, rank)
            .iter()
            .map(|r| r.service_id)
            .collect();
        if ids.is_empty() {
            return Ok(None);
        }
        sqlx::query("SELECT id FROM services WHERE id = ANY($1) ORDER BY id FOR UPDATE")
            .bind(&ids)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        // Re-read after the lock: a concurrent upgrade may have consumed one.
        rows = chain(tx, group, cust, &names, &[]).await?;
    }
    let cands = candidates(&rows, rank);
    if cands.is_empty() {
        return Ok(None);
    }
    let to = rate(tx, currency).await?.unwrap_or(Decimal::ONE);
    let values_held = carried(&rows);
    let mut values = Vec::with_capacity(cands.len());
    for r in &cands {
        let v = if mode == "difference" {
            // Today's list price of the owned tier for this cycle (else its
            // one-time price), scaled by how much of it is still paid.
            let list: Option<Decimal> = sqlx::query_scalar(
                "SELECT amount FROM product_prices WHERE product_id = $1 AND currency = $2 AND cycle = ANY($3)
                 ORDER BY (cycle = $4) DESC LIMIT 1",
            )
            .bind(r.product_id)
            .bind(currency)
            .bind(vec![cycle.to_string(), "one_time".to_string()])
            .bind(cycle)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            round(
                list.unwrap_or_default() * r.paid_ratio.clamp(Decimal::ZERO, Decimal::ONE),
                exp,
            )
        } else {
            let held = values_held.get(&r.service_id).copied().unwrap_or_default();
            match rate(tx, &r.currency).await? {
                Some(from) => convert(held, from, to, exp),
                None => Decimal::ZERO,
            }
        };
        values.push((r.service_id, v));
    }
    let floor = round(min_price * to, exp);
    let Some((source, amount)) = credit_amount(price, floor, &values, exp) else {
        return Ok(None);
    };
    let from_name: String = sqlx::query_scalar(
        "SELECT p.name FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1",
    )
    .bind(source)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(Some(LineCredit {
        group_id: group,
        amount,
        source,
        from_name,
        replaces: replaced(&rows, rank),
    }))
}

/// Record the lower tiers an order line replaces (inside `place_full`).
pub async fn record(
    tx: &mut Transaction<'_, Postgres>,
    order_item: Uuid,
    c: &LineCredit,
) -> AppResult<()> {
    for s in &c.replaces {
        sqlx::query(
            "INSERT INTO tier_upgrades (order_item_id, from_service_id, source, credit) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",
        )
        .bind(order_item)
        .bind(s)
        .bind(*s == c.source)
        .bind(if *s == c.source { c.amount } else { Decimal::ZERO })
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// On approval: cancel the recurring lower tiers an upgrade replaced (when
/// the group says so) so they stop billing. Returns the cancelled services.
pub async fn apply(tx: &mut Transaction<'_, Postgres>, order: Uuid) -> AppResult<Vec<Uuid>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE services s SET status = 'cancelled', updated_at = now()
         FROM tier_upgrades tu
         JOIN order_items oi ON oi.id = tu.order_item_id
         JOIN tier_group_items gi ON gi.product_id = oi.product_id
         JOIN tier_groups g ON g.id = gi.group_id
         WHERE oi.order_id = $1 AND tu.from_service_id = s.id AND g.cancel_old_recurring
           AND s.cycle <> 'one_time' AND s.status IN ('active','suspended')
         RETURNING s.id",
    )
    .bind(order)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if !ids.is_empty() {
        sqlx::query(
            "UPDATE tier_upgrades SET cancelled_at = now() WHERE from_service_id = ANY($1)
             AND order_item_id IN (SELECT id FROM order_items WHERE order_id = $2)",
        )
        .bind(&ids)
        .bind(order)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(ids)
}

/// Lower tiers whose `expiry` commands run when `service` (an upgrade) is
/// delivered: only for groups with `revoke_old`.
pub async fn revocations(pool: &PgPool, service: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT tu.from_service_id FROM services s
         JOIN tier_upgrades tu ON tu.order_item_id = s.order_item_id
         JOIN tier_group_items gi ON gi.product_id = s.product_id
         JOIN tier_groups g ON g.id = gi.group_id
         WHERE s.id = $1 AND g.revoke_old ORDER BY tu.from_service_id",
    )
    .bind(service)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Public view: tier groups (names and ranks) for the store.
pub async fn public(pool: &PgPool) -> AppResult<Vec<TierGroup>> {
    list(pool).await
}

/// Owned-tier summary for a signed-in viewer: product id → credit preview
/// per package (cart currency), for the store's "Upgrade" badges.
pub async fn preview_for(
    pool: &PgPool,
    customer: Uuid,
    gift_to: Option<&str>,
    currency: &str,
) -> AppResult<HashMap<Uuid, LineCredit>> {
    let mut out = HashMap::new();
    if !enabled(pool).await? {
        return Ok(out);
    }
    let currency = currency.to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exp = crate::invoices::exponent(&mut tx, &currency).await?;
    let rows: Vec<(Uuid, Decimal, String)> = sqlx::query_as(
        "SELECT gi.product_id, pp.amount, pp.cycle FROM tier_group_items gi
         JOIN LATERAL (SELECT amount, cycle FROM product_prices WHERE product_id = gi.product_id AND currency = $1
                       ORDER BY (cycle = 'one_time') DESC, amount LIMIT 1) pp ON true",
    )
    .bind(&currency)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut seen = HashSet::new();
    for (p, price, cycle) in rows {
        if !seen.insert(p) {
            continue;
        }
        if let Some(c) = credit_for(
            &mut tx,
            Some(customer),
            gift_to,
            p,
            &cycle,
            &currency,
            price,
            exp,
            false,
        )
        .await?
        {
            out.insert(p, c);
        }
    }
    tx.rollback().await.map_err(db_err)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn row(rank: i32, net: &str) -> ChainRow {
        ChainRow {
            service_id: Uuid::new_v4(),
            product_id: Uuid::new_v4(),
            rank,
            active: true,
            valid: true,
            net: d(net),
            credit: Decimal::ZERO,
            source: None,
            paid_ratio: Decimal::ONE,
            currency: "EUR".into(),
            consumed: false,
        }
    }

    #[test]
    fn credit_is_capped_by_floor_and_price() {
        let s = Uuid::new_v4();
        assert_eq!(
            credit_amount(d("20"), d("0"), &[(s, d("10"))], 2),
            Some((s, d("10")))
        );
        // Owned tier worth more than the new one: free, never negative.
        assert_eq!(
            credit_amount(d("20"), d("0"), &[(s, d("35"))], 2),
            Some((s, d("20")))
        );
        // Minimum price 2.50 stays payable.
        assert_eq!(
            credit_amount(d("20"), d("2.50"), &[(s, d("35"))], 2),
            Some((s, d("17.50")))
        );
        // Floor above the price: no credit at all.
        assert_eq!(credit_amount(d("2"), d("5"), &[(s, d("1"))], 2), None);
        assert_eq!(credit_amount(d("20"), d("0"), &[(s, d("0"))], 2), None);
        assert_eq!(credit_amount(d("20"), d("0"), &[], 2), None);
        // Rounds to the currency (JPY has no decimals).
        assert_eq!(
            credit_amount(d("2000"), d("0"), &[(s, d("999.5"))], 0),
            Some((s, d("1000")))
        );
    }

    #[test]
    fn best_candidate_wins_and_is_stable() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert_eq!(
            credit_amount(d("30"), d("0"), &[(a, d("10")), (b, d("20"))], 2),
            Some((b, d("20")))
        );
        // Tie: the same source every time.
        let t1 = credit_amount(d("30"), d("0"), &[(a, d("10")), (b, d("10"))], 2);
        let t2 = credit_amount(d("30"), d("0"), &[(b, d("10")), (a, d("10"))], 2);
        assert_eq!(t1, t2);
    }

    #[test]
    fn cumulative_value_follows_the_chain() {
        // VIP paid 10; VIP+ (20) paid 10 with 10 credit from VIP.
        let vip = row(1, "10");
        let mut plus = row(2, "10");
        plus.credit = d("10");
        plus.source = Some(vip.service_id);
        let v = carried(&[vip.clone(), plus.clone()]);
        assert_eq!(v[&vip.service_id], d("10"));
        assert_eq!(v[&plus.service_id], d("20"));
        // MVP: the best candidate is VIP+ (20), not VIP + VIP+ (30).
        let vals: Vec<(Uuid, Decimal)> = candidates(&[vip.clone(), plus.clone()], 3)
            .iter()
            .map(|r| (r.service_id, v[&r.service_id]))
            .collect();
        assert_eq!(
            credit_amount(d("30"), d("0"), &vals, 2),
            Some((plus.service_id, d("20")))
        );
    }

    #[test]
    fn refunds_release_value_without_double_credit() {
        let mut vip = row(1, "10");
        let mut plus = row(2, "10");
        plus.credit = d("10");
        plus.source = Some(vip.service_id);
        // VIP refunded after the upgrade: VIP+ no longer carries its credit.
        vip.valid = false;
        let v = carried(&[vip.clone(), plus.clone()]);
        assert_eq!(v[&plus.service_id], d("10"));
        // Half of VIP+ refunded: only the paid part shrinks.
        vip.valid = true;
        plus.paid_ratio = d("0.5");
        let v = carried(&[vip.clone(), plus.clone()]);
        assert_eq!(v[&plus.service_id], d("15"));
        // VIP+ fully refunded (order cancelled): it carries nothing and is
        // no longer a candidate, so VIP is credited again, once.
        plus.valid = false;
        let rows = [vip.clone(), plus.clone()];
        let v = carried(&rows);
        assert_eq!(v[&plus.service_id], d("0"));
        let c: Vec<Uuid> = candidates(&rows, 3).iter().map(|r| r.service_id).collect();
        assert_eq!(c, vec![vip.service_id]);
    }

    #[test]
    fn expired_consumed_and_higher_tiers_are_not_candidates() {
        let mut expired = row(1, "10");
        expired.active = false;
        let mut consumed = row(1, "10");
        consumed.consumed = true;
        let higher = row(3, "50");
        let same = row(2, "20");
        let rows = [expired.clone(), consumed.clone(), higher, same];
        assert!(candidates(&rows, 2).is_empty());
        // Replaced still lists the consumed lower tier (it is owned), not the
        // expired one.
        assert_eq!(replaced(&rows, 2), vec![consumed.service_id]);
    }

    #[test]
    fn unpaid_and_cyclic_rows_are_safe() {
        let mut a = row(1, "10");
        a.paid_ratio = Decimal::ZERO;
        let mut b = row(2, "10");
        b.source = Some(a.service_id);
        b.credit = d("5");
        a.source = Some(b.service_id);
        a.credit = d("5");
        let v = carried(&[a.clone(), b.clone()]);
        // Terminates, and never exceeds paid + credit.
        assert!(v[&b.service_id] <= d("15"));
        assert!(v[&a.service_id] <= d("5"));
        // A source missing from the chain contributes nothing.
        let mut c = row(1, "10");
        c.source = Some(Uuid::new_v4());
        c.credit = d("100");
        assert_eq!(carried(&[c.clone()])[&c.service_id], d("10"));
    }

    #[test]
    fn converts_between_currencies() {
        // 10 EUR (rate 1) in USD at 1.10.
        assert_eq!(convert(d("10"), d("1"), d("1.1"), 2), d("11.00"));
        assert_eq!(convert(d("11"), d("1.1"), d("1"), 2), d("10.00"));
        assert_eq!(convert(d("10"), d("0"), d("1"), 2), d("0"));
    }

    #[test]
    fn validates_groups() {
        let ok = TierGroupInput {
            name: "Ranks".into(),
            credit_mode: "paid".into(),
            min_price: Decimal::ZERO,
            revoke_old: false,
            cancel_old_recurring: true,
            product_ids: vec![Uuid::new_v4(), Uuid::new_v4()],
        };
        assert!(ok.validate().is_ok());
        let mut bad = ok.clone();
        bad.product_ids.truncate(1);
        assert!(bad.validate().is_err());
        let mut bad = ok.clone();
        bad.product_ids = vec![ok.product_ids[0], ok.product_ids[0]];
        assert!(bad.validate().is_err());
        let mut bad = ok.clone();
        bad.credit_mode = "free".into();
        assert!(bad.validate().is_err());
        let mut bad = ok.clone();
        bad.min_price = d("-1");
        assert!(bad.validate().is_err());
        let mut bad = ok;
        bad.name = " ".into();
        assert!(bad.validate().is_err());
    }
}
