//! Orders: cart validation → order + finalized invoice in one transaction;
//! approval creates services in one transaction.
use crate::addons;
use crate::catalog;
use crate::coupons;
use crate::db_err;
use crate::invoices::{self, Invoice, NewItem};
use pelty_billing::cycle;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MAX_ITEMS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CartItem {
    pub product_id: Uuid,
    pub cycle: String,
    /// option_id → choice_id (or quantity for quantity options)
    #[serde(default)]
    #[schema(value_type = Object)]
    pub config: HashMap<Uuid, catalog::ConfigValue>,
    /// Addon ids to order with this product.
    #[serde(default)]
    pub addons: Vec<Uuid>,
    /// Custom order field values (field_id → value).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub fields: HashMap<Uuid, String>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Order {
    pub id: Uuid,
    pub number: i64,
    pub customer_id: Uuid,
    pub status: String,
    pub currency: String,
    pub total: Decimal,
    pub invoice_id: Option<Uuid>,
    /// Status of the order's invoice, so an accepted-but-unpaid order can be
    /// shown as awaiting payment.
    pub invoice_status: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub approved_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, number, customer_id, status, currency::text AS currency, total, invoice_id, \
    (SELECT i.status::text FROM invoices i WHERE i.id = orders.invoice_id) AS invoice_status, approved_at, created_at";

/// Validate cart, price every line from the catalog (never from the client),
/// create order + items + a finalized invoice atomically.
pub async fn place(
    pool: &PgPool,
    customer_id: Uuid,
    currency: &str,
    items: &[CartItem],
    coupon: Option<&str>,
    due_days: i64,
) -> AppResult<(Order, Invoice)> {
    place_with(pool, customer_id, currency, items, coupon, due_days, None).await
}

/// Bundle discount applied to an order: percent off the matched cart lines'
/// recurring price (after any promo), first invoice only.
#[derive(Debug, Clone)]
pub struct BundleDiscount {
    pub bundle_id: Uuid,
    pub name: String,
    pub percent: Decimal,
    /// Cart indices covered by the bundle.
    pub items: Vec<usize>,
}

/// A first-invoice discount on one cart line, computed by an addon (filter
/// `checkout.discounts`) before the transaction and audited by the caller.
/// Clamped to the line's recurring price as quoted inside the transaction.
#[derive(Debug, Clone)]
pub struct AddonDiscount {
    /// Cart index.
    pub item: usize,
    pub amount: Decimal,
    pub label: String,
}

/// [`place`] with an optional bundle discount.
pub async fn place_with(
    pool: &PgPool,
    customer_id: Uuid,
    currency: &str,
    items: &[CartItem],
    coupon: Option<&str>,
    due_days: i64,
    bundle: Option<&BundleDiscount>,
) -> AppResult<(Order, Invoice)> {
    place_full(
        pool,
        customer_id,
        currency,
        items,
        coupon,
        due_days,
        bundle,
        &[],
    )
    .await
}

/// Price the current catalog amounts of `items` without placing anything
/// (input for addon discounts; `place_full` re-quotes inside its own
/// transaction).
pub async fn line_amounts(
    pool: &PgPool,
    currency: &str,
    items: &[CartItem],
) -> AppResult<Vec<catalog::Quote>> {
    let currency = currency.to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let mut out = Vec::with_capacity(items.len());
    for it in items.iter().take(MAX_ITEMS) {
        out.push(catalog::quote(&mut tx, it.product_id, &it.cycle, &currency, &it.config).await?);
    }
    tx.rollback().await.map_err(db_err)?;
    Ok(out)
}

/// [`place_with`] plus addon discounts (see [`AddonDiscount`]).
#[allow(clippy::too_many_arguments)]
pub async fn place_full(
    pool: &PgPool,
    customer_id: Uuid,
    currency: &str,
    items: &[CartItem],
    coupon: Option<&str>,
    due_days: i64,
    bundle: Option<&BundleDiscount>,
    addon_discounts: &[AddonDiscount],
) -> AppResult<(Order, Invoice)> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(AppError::Validation("error.cart_invalid"));
    }
    if items.iter().any(|i| !cycle::is_valid(&i.cycle)) {
        return Err(AppError::Validation("error.invalid_cycle"));
    }
    let currency = currency.to_ascii_uppercase();
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM customers WHERE id = $1")
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    if status.as_deref() != Some("active") {
        return Err(AppError::Forbidden);
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exp = invoices::exponent(&mut tx, &currency).await?;
    let coupon = match coupon.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) => Some(coupons::lock_for_order(&mut tx, code, customer_id, &currency).await?),
        None => None,
    };
    // Client-group discount: percent off recurring prices (product + addons,
    // after any promo code). Setup fees are never discounted.
    let group: Option<(String, Decimal)> = sqlx::query_as(
        "SELECT g.name, g.discount_percent FROM customers c JOIN client_groups g ON g.id = c.client_group_id
         WHERE c.id = $1 AND g.discount_percent > 0",
    )
    .bind(customer_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut group_total = Decimal::ZERO;
    let mut quotes = Vec::with_capacity(items.len());
    for it in items {
        let q = catalog::quote(&mut tx, it.product_id, &it.cycle, &currency, &it.config).await?;
        let extras =
            addons::quote(&mut tx, it.product_id, &it.cycle, &currency, &it.addons).await?;
        let fields = addons::check_fields(&mut tx, it.product_id, &it.fields).await?;
        // After validation so bad input stays a 422, not a stock lookup 404.
        catalog::take_stock(&mut tx, it.product_id).await?;
        quotes.push((q, extras, fields));
    }
    let order_id = Uuid::new_v4();
    sqlx::query("INSERT INTO orders (id, customer_id, currency, total) VALUES ($1,$2,$3,0)")
        .bind(order_id)
        .bind(customer_id)
        .bind(&currency)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let inv = invoices::create_draft(&mut tx, customer_id, &currency, Some(order_id)).await?;
    let mut lines = Vec::new();
    let mut discount_total = Decimal::ZERO;
    let mut bundle_total = Decimal::ZERO;
    // Per line: discounts already granted (coupon, bundle, group share).
    let mut used = vec![Decimal::ZERO; quotes.len()];
    for (idx, (q, extras, fields)) in quotes.iter().enumerate() {
        let item_id = Uuid::new_v4();
        let mut discount = match &coupon {
            Some(c) if c.applies_to(q.product_id) => {
                coupons::line_discount(&c.kind, c.value, q.amount, exp)
            }
            _ => Decimal::ZERO,
        };
        if let Some(c) = coupon
            .as_ref()
            .filter(|c| c.applies_addons && c.applies_to(q.product_id))
        {
            discount += extras
                .iter()
                .filter(|a| a.cycle != "one_time")
                .map(|a| coupons::line_discount(&c.kind, c.value, a.amount, exp))
                .sum::<Decimal>();
        }
        let recurring_discount = match &coupon {
            Some(c) if c.recurring && q.cycle != "one_time" => discount,
            _ => Decimal::ZERO,
        };
        discount_total += discount;
        used[idx] += discount;
        if let Some(b) = bundle.filter(|b| b.items.contains(&idx)) {
            let d = coupons::line_discount(
                "percent",
                b.percent,
                (q.amount - discount).max(Decimal::ZERO),
                exp,
            );
            bundle_total += d;
            used[idx] += d;
        }
        if let Some((_, pct)) = &group {
            let base = q.amount + extras.iter().map(|a| a.amount).sum::<Decimal>() - discount;
            let d = coupons::line_discount("percent", *pct, base, exp);
            group_total += d;
            used[idx] += d;
        }
        sqlx::query("INSERT INTO order_items (id, order_id, product_id, cycle, amount, setup_fee, config, custom_fields, discount, recurring_discount, promo_cycles) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
            .bind(item_id)
            .bind(order_id)
            .bind(q.product_id)
            .bind(&q.cycle)
            .bind(q.amount)
            .bind(q.setup_fee)
            .bind(&q.config)
            .bind(fields)
            .bind(discount)
            .bind(recurring_discount)
            .bind(coupon.as_ref().filter(|c| c.recurring).and_then(|c| c.recurring_cycles))
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let mut desc = format!("{}, {}", q.product_name, q.cycle);
        if !q.labels.is_empty() {
            desc = format!("{desc} ({})", q.labels.join(", "));
        }
        lines.push(NewItem {
            description: desc,
            quantity: Decimal::ONE,
            unit_price: q.amount,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
        if q.setup_fee > Decimal::ZERO {
            lines.push(NewItem {
                description: format!("{}, setup", q.product_name),
                quantity: Decimal::ONE,
                unit_price: q.setup_fee,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            });
        }
        for a in extras {
            sqlx::query("INSERT INTO order_item_addons (order_item_id, addon_id, cycle, amount, setup_fee) VALUES ($1,$2,$3,$4,$5)")
                .bind(item_id)
                .bind(a.addon_id)
                .bind(&a.cycle)
                .bind(a.amount)
                .bind(a.setup_fee)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            let line = |description: String, unit_price: Decimal| NewItem {
                description,
                quantity: Decimal::ONE,
                unit_price,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            };
            if a.amount > Decimal::ZERO || a.setup_fee == Decimal::ZERO {
                lines.push(line(
                    format!("{}, addon: {}, {}", q.product_name, a.name, a.cycle),
                    a.amount,
                ));
            }
            if a.setup_fee > Decimal::ZERO {
                lines.push(line(
                    format!("{}, addon: {}, setup", q.product_name, a.name),
                    a.setup_fee,
                ));
            }
        }
    }
    if let Some(c) = &coupon {
        if discount_total <= Decimal::ZERO {
            return Err(AppError::Validation("error.coupon_not_applicable"));
        }
        let what = match (c.recurring, c.recurring_cycles) {
            (true, Some(n)) => format!("{n} invoices"),
            (true, None) => "recurring".to_string(),
            _ => "first invoice".to_string(),
        };
        lines.push(NewItem {
            description: format!("Promo {} ({what})", c.code),
            quantity: Decimal::ONE,
            unit_price: -discount_total,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
        coupons::redeem(&mut tx, c, customer_id, order_id, discount_total).await?;
    }
    if let Some((name, pct)) = &group {
        if group_total > Decimal::ZERO {
            lines.push(NewItem {
                description: format!("Client group discount: {name} ({}%)", pct.normalize()),
                quantity: Decimal::ONE,
                unit_price: -group_total,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            });
        }
    }
    if let Some(b) = bundle.filter(|_| bundle_total > Decimal::ZERO) {
        lines.push(NewItem {
            description: format!("Bundle discount: {} ({}%)", b.name, b.percent.normalize()),
            quantity: Decimal::ONE,
            unit_price: -bundle_total,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
    }
    let mut addon_total = Decimal::ZERO;
    for (idx, (q, _, _)) in quotes.iter().enumerate() {
        let mut left = (q.amount - used[idx]).max(Decimal::ZERO);
        for d in addon_discounts.iter().filter(|d| d.item == idx) {
            let amt = d.amount.round_dp(exp).min(left);
            if amt <= Decimal::ZERO {
                continue;
            }
            left -= amt;
            addon_total += amt;
            lines.push(NewItem {
                description: format!("{}, {}", d.label, q.product_name),
                quantity: Decimal::ONE,
                unit_price: -amt,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            });
        }
    }
    invoices::add_items(&mut tx, inv, &currency, &lines).await?;
    let invoice = invoices::finalize(&mut tx, inv, due_days).await?;
    if let Some(b) = bundle {
        sqlx::query("UPDATE orders SET bundle_id = $2 WHERE id = $1")
            .bind(order_id)
            .bind(b.bundle_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    let order: Order = sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE orders SET total = $2, invoice_id = $3, coupon_id = $4, discount = $5 WHERE id = $1 RETURNING {COLS}")))
        .bind(order_id)
        .bind(invoice.total)
        .bind(inv)
        .bind(coupon.as_ref().map(|c| c.id))
        .bind(discount_total + group_total + bundle_total + addon_total)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((order, invoice))
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Order>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM orders WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Look up an order by its human-facing number (as shown in lists).
pub async fn get_by_number(pool: &PgPool, number: i64) -> AppResult<Option<Order>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM orders WHERE number = $1"
    )))
    .bind(number)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn list(
    pool: &PgPool,
    customer: Option<Uuid>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Order>, i64)> {
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        o: Order,
        total_count: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS}, count(*) OVER() AS total_count FROM orders
         WHERE ($1::uuid IS NULL OR customer_id = $1) AND ($2::text IS NULL OR status = $2)
         ORDER BY created_at DESC, id LIMIT $3 OFFSET $4"
    )))
    .bind(customer)
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = rows.first().map(|r| r.total_count).unwrap_or(0);
    Ok((rows.into_iter().map(|r| r.o).collect(), total))
}

/// Approve a pending order: creates one active service per item, all in one
/// transaction (either every service exists or none). Returns service ids.
pub async fn approve(
    pool: &PgPool,
    id: Uuid,
    actor: impl Into<Option<Uuid>>,
) -> AppResult<Vec<Uuid>> {
    let actor: Option<Uuid> = actor.into();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let st: String = sqlx::query_scalar("SELECT status FROM orders WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    if st != "pending" {
        return Err(AppError::Conflict("error.order_not_pending"));
    }
    // Staff approving the order is also their decision on a fraud hold.
    if let Some(a) = actor {
        sqlx::query("UPDATE fraud_checks SET decision = 'approved', reviewed_by = $2, reviewed_at = now(), review_note = 'order approved' WHERE order_id = $1 AND verdict = 'review' AND decision IS NULL")
            .bind(id)
            .bind(a)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    let items: Vec<(Uuid, Uuid, String, Decimal, Decimal, Option<i32>)> = sqlx::query_as("SELECT id, product_id, cycle, amount, recurring_discount, promo_cycles FROM order_items WHERE order_id = $1")
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
    let (cust, cur): (Uuid, String) =
        sqlx::query_as("SELECT customer_id, currency::text FROM orders WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let today = OffsetDateTime::now_utc().date();
    let mut ids = Vec::with_capacity(items.len());
    for (item_id, product, cyc, full, rd, cycles) in items {
        let sid = Uuid::new_v4();
        // Limited recurring promo: the first invoice used one cycle.
        let (amount, promo, left) = match cycles {
            Some(n) if n <= 1 => (full, Decimal::ZERO, None),
            Some(n) => (full - rd, rd, Some(n - 1)),
            None => (full - rd, Decimal::ZERO, None),
        };
        let next = cycle::months(&cyc).map(|m| cycle::add_months(today, m));
        sqlx::query(
            "INSERT INTO services (id, customer_id, product_id, order_item_id, status, cycle, currency, recurring_amount, next_due_date, activated_at, promo_discount, promo_cycles_left)
             VALUES ($1,$2,$3,$4,'active',$5,$6,$7,$8,now(),$9,$10)",
        )
        .bind(sid)
        .bind(cust)
        .bind(product)
        .bind(item_id)
        .bind(&cyc)
        .bind(&cur)
        .bind(full)
        .bind(next)
        .bind(promo)
        .bind(left)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        // Recurring addons follow the service's renewal; one-time ones are
        // recorded with a zero recurring amount.
        sqlx::query(
            "INSERT INTO service_addons (id, service_id, addon_id, cycle, recurring_amount)
             SELECT gen_random_uuid(), $1, addon_id, cycle, CASE WHEN cycle = 'one_time' THEN 0 ELSE amount END
             FROM order_item_addons WHERE order_item_id = $2",
        )
        .bind(sid)
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "UPDATE services s SET custom_fields = oi.custom_fields,
                 recurring_amount = GREATEST(s.recurring_amount + COALESCE((SELECT sum(recurring_amount) FROM service_addons WHERE service_id = s.id), 0) - $3, 0)
             FROM order_items oi WHERE s.id = $1 AND oi.id = $2",
        )
        .bind(sid)
        .bind(item_id)
        .bind(full - amount)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        ids.push(sid);
    }
    sqlx::query(
        "UPDATE orders SET status = 'active', approved_at = now(), approved_by = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(ids)
}

/// Cancel a pending order and void its invoice if nothing was paid.
pub async fn cancel(pool: &PgPool, id: Uuid, fraud: bool) -> AppResult<Order> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let o: Order = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE orders SET status = $2 WHERE id = $1 AND status = 'pending' RETURNING {COLS}"
    )))
    .bind(id)
    .bind(if fraud { "fraud" } else { "cancelled" })
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?
    .ok_or(AppError::Conflict("error.order_not_pending"))?;
    // Release the coupon use so a cancelled order doesn't burn a limit.
    sqlx::query("UPDATE coupons SET uses = uses - 1 WHERE id = (SELECT coupon_id FROM coupon_redemptions WHERE order_id = $1) AND uses > 0")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("DELETE FROM coupon_redemptions WHERE order_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE invoices SET status = 'void' WHERE id = $1 AND status = 'unpaid' AND amount_paid = 0")
        .bind(o.invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(o)
}

/// WHMCS `PendingOrder`: put a cancelled/fraud order back to pending. Its
/// invoice is reopened only when it was voided with nothing paid; the coupon
/// redemption released on cancel is not restored.
pub async fn reopen(pool: &PgPool, id: Uuid) -> AppResult<Order> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let o: Order = sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE orders SET status = 'pending' WHERE id = $1 AND status IN ('cancelled','fraud') RETURNING {COLS}")))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::Conflict("error.order_not_cancelled"))?;
    sqlx::query("UPDATE invoices SET status = 'unpaid' WHERE id = $1 AND status = 'void' AND amount_paid = 0 AND kind = 'invoice'")
        .bind(o.invoice_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(o)
}

/// WHMCS `DeleteOrder`: only cancelled/fraud orders without provisioned
/// services can be removed (invoices keep existing, unlinked).
pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let st: Option<(String,)> =
        sqlx::query_as("SELECT status FROM orders WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    match st {
        None => return Err(AppError::NotFound),
        Some((s,)) if s != "cancelled" && s != "fraud" => {
            return Err(AppError::Conflict("error.order_not_cancelled"))
        }
        _ => {}
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM services s JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1").bind(id).fetch_one(&mut *tx).await.map_err(db_err)?;
    if n > 0 {
        return Err(AppError::Conflict("error.order_has_services"));
    }
    sqlx::query("DELETE FROM orders WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct OrderItem {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub cycle: String,
    pub amount: Decimal,
    pub setup_fee: Decimal,
    pub config: serde_json::Value,
}

pub async fn items(pool: &PgPool, order_id: Uuid) -> AppResult<Vec<OrderItem>> {
    sqlx::query_as(
        "SELECT oi.id, oi.product_id, p.name AS product_name, oi.cycle, oi.amount, oi.setup_fee, oi.config
         FROM order_items oi JOIN products p ON p.id = oi.product_id WHERE oi.order_id = $1 ORDER BY oi.id",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}
