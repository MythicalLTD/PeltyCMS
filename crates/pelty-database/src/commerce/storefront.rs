//! Public storefront: persisted carts with a read-only price/tax preview,
//! abandoned-cart lookup, customer contacts (sub-accounts), email
//! preferences, login history and admin order notes.
use crate::orders::{CartItem, MAX_ITEMS};
use crate::{addons, catalog, coupons, db_err, tax};
use pelty_billing::cycle;
use pelty_billing::math::round;
use pelty_billing::tax::{self as engine, FormatOnly, Rule, TaxPart};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

// carts

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Cart {
    pub id: Uuid,
    pub customer_id: Option<Uuid>,
    pub email: Option<String>,
    pub currency: String,
    pub country: String,
    pub items: Value,
    pub coupon: Option<String>,
    /// Creator code entered on the cart (module `creator_codes`).
    pub creator_code: Option<String>,
    pub order_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const CART_COLS: &str =
    "id, customer_id, email, currency::text AS currency, country, items, coupon, creator_code, order_id, updated_at";

#[derive(Debug, Clone)]
pub struct CartInput {
    pub currency: String,
    pub country: String,
    pub items: Vec<CartItem>,
    pub coupon: Option<String>,
    pub creator_code: Option<String>,
    pub email: Option<String>,
    pub locale: String,
}

pub fn parse_items(v: &Value) -> Vec<CartItem> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

pub async fn get_cart(pool: &PgPool, id: Uuid) -> AppResult<Option<Cart>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CART_COLS} FROM carts WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Create or replace a cart. A cart that already became an order is frozen.
pub async fn save_cart(
    pool: &PgPool,
    id: Uuid,
    customer: Option<Uuid>,
    i: &CartInput,
) -> AppResult<Cart> {
    if i.items.len() > MAX_ITEMS {
        return Err(AppError::Validation("error.cart_invalid"));
    }
    if i.items.iter().any(|it| !cycle::is_valid(&it.cycle)) {
        return Err(AppError::Validation("error.invalid_cycle"));
    }
    let country = i.country.trim().to_ascii_uppercase();
    if !(country.is_empty()
        || (country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase())))
    {
        return Err(AppError::Validation("error.invalid_country"));
    }
    let email = i.email.as_deref().map(str::trim).filter(|e| !e.is_empty());
    if email.is_some_and(|e| e.len() > 254 || !e.contains('@')) {
        return Err(AppError::Validation("error.invalid_email"));
    }
    let coupon = i
        .coupon
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| c.chars().take(64).collect::<String>());
    let creator_code = i
        .creator_code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| c.chars().take(24).collect::<String>());
    let items =
        serde_json::to_value(&i.items).map_err(|_| AppError::Validation("error.cart_invalid"))?;
    let enabled: Option<String> =
        sqlx::query_scalar("SELECT code::text FROM currencies WHERE code = $1 AND enabled")
            .bind(i.currency.to_ascii_uppercase())
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let currency = enabled.ok_or(AppError::Validation("error.invalid_currency"))?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO carts (id, customer_id, email, currency, country, items, coupon, locale, creator_code)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT (id) DO UPDATE SET customer_id = COALESCE(EXCLUDED.customer_id, carts.customer_id),
             email = COALESCE(EXCLUDED.email, carts.email), currency = EXCLUDED.currency, country = EXCLUDED.country,
             items = EXCLUDED.items, coupon = EXCLUDED.coupon, creator_code = EXCLUDED.creator_code, locale = EXCLUDED.locale, updated_at = now(), reminded_at = NULL
         WHERE carts.order_id IS NULL
           AND (carts.customer_id IS NULL OR EXCLUDED.customer_id IS NULL OR carts.customer_id = EXCLUDED.customer_id)
         RETURNING {CART_COLS}"
    )))
    .bind(id)
    .bind(customer)
    .bind(email)
    .bind(&currency)
    .bind(&country)
    .bind(items)
    .bind(coupon)
    .bind(i.locale.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect::<String>())
    .bind(creator_code)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::Conflict("error.cart_closed"))
}

pub async fn mark_converted(
    pool: &PgPool,
    cart: Uuid,
    customer: Uuid,
    order: Uuid,
) -> AppResult<()> {
    sqlx::query("UPDATE carts SET order_id = $3, customer_id = $2, updated_at = now() WHERE id = $1 AND order_id IS NULL")
        .bind(cart)
        .bind(customer)
        .bind(order)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Carts with items and a reachable email, idle longer than `idle_minutes`,
/// never reminded and not converted.
#[derive(Debug, Clone, FromRow)]
pub struct Abandoned {
    pub id: Uuid,
    pub email: String,
    pub items: i32,
    pub locale: String,
}

pub async fn abandoned(pool: &PgPool, idle_minutes: i64, limit: i64) -> AppResult<Vec<Abandoned>> {
    sqlx::query_as(
        "SELECT c.id, COALESCE(c.email, cu.email::text) AS email, jsonb_array_length(c.items) AS items, c.locale
         FROM carts c LEFT JOIN customers cu ON cu.id = c.customer_id
         LEFT JOIN customer_email_prefs p ON p.customer_id = c.customer_id
         WHERE c.order_id IS NULL AND c.reminded_at IS NULL AND jsonb_array_length(c.items) > 0
           AND c.updated_at < now() - make_interval(mins => $1::int)
           AND COALESCE(c.email, cu.email::text) IS NOT NULL
           AND COALESCE(p.marketing, true)
         ORDER BY c.updated_at LIMIT $2",
    )
    .bind(idle_minutes as i32)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Claim a cart for reminding; false when another worker got it first.
pub async fn mark_reminded(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("UPDATE carts SET reminded_at = now() WHERE id = $1 AND reminded_at IS NULL AND order_id IS NULL")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewAddon {
    pub addon_id: Uuid,
    pub name: String,
    pub cycle: String,
    pub amount: Decimal,
    pub setup_fee: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewLine {
    pub product_id: Uuid,
    pub product_name: String,
    pub cycle: String,
    pub amount: Decimal,
    pub setup_fee: Decimal,
    pub labels: Vec<String>,
    pub addons: Vec<PreviewAddon>,
    pub discount: Decimal,
    /// Running timed sale on this line (first invoice only).
    pub sale_discount: Decimal,
    pub sale: Option<String>,
    /// Creator code discount on this line (first invoice only).
    pub creator_discount: Decimal,
    /// Share of the bundle discount on this line.
    pub bundle_discount: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub currency: String,
    pub lines: Vec<PreviewLine>,
    pub subtotal: Decimal,
    pub discount: Decimal,
    pub group_discount: Decimal,
    pub sale_discount: Decimal,
    pub creator_discount: Decimal,
    pub tax: Decimal,
    pub taxes: Vec<TaxPart>,
    pub total: Decimal,
    pub inclusive: bool,
    pub coupon: Option<String>,
    pub coupon_error: Option<&'static str>,
    pub creator_code: Option<String>,
    pub creator_error: Option<&'static str>,
    /// Bundle applied automatically because the cart holds all of it.
    pub bundle: Option<String>,
    pub bundle_discount: Decimal,
}

/// The active bundle with the biggest discount that the cart fully holds.
pub async fn best_bundle(
    pool: &PgPool,
    items: &[CartItem],
) -> AppResult<Option<crate::orders::BundleDiscount>> {
    if items.len() < 2 {
        return Ok(None);
    }
    let mut best: Option<crate::orders::BundleDiscount> = None;
    for b in crate::bundles::list(pool, true).await? {
        if b.discount_percent <= Decimal::ZERO
            || best
                .as_ref()
                .is_some_and(|x| x.percent >= b.discount_percent)
        {
            continue;
        }
        if let Some(idx) = crate::bundles::match_cart(&b, items) {
            best = Some(crate::orders::BundleDiscount {
                bundle_id: b.id,
                name: b.name,
                percent: b.discount_percent,
                items: idx,
            });
        }
    }
    Ok(best)
}

#[derive(FromRow)]
struct TaxCust {
    country: String,
    region: String,
    vat_number: String,
    tax_exempt: bool,
}

/// Price a cart exactly like `orders::place` would, without writing
/// anything: catalog prices, addons, promo code, client-group discount and
/// the tax engine. A bad promo or creator code is reported, not fatal.
#[allow(clippy::too_many_arguments)]
pub async fn preview(
    pool: &PgPool,
    customer: Option<Uuid>,
    country: &str,
    currency: &str,
    items: &[CartItem],
    coupon: Option<&str>,
    creator: Option<&str>,
) -> AppResult<Preview> {
    if items.len() > MAX_ITEMS {
        return Err(AppError::Validation("error.cart_invalid"));
    }
    let currency = currency.to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exp = crate::invoices::exponent(&mut tx, &currency).await?;
    let mut coupon_error = None;
    let promo = match coupon.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) => {
            match coupons::check(pool, code, customer.unwrap_or(Uuid::nil()), &currency).await {
                Ok(c) => Some(c),
                Err(AppError::Validation(k)) => {
                    coupon_error = Some(k);
                    None
                }
                Err(e) => return Err(e),
            }
        }
        None => None,
    };
    let mut creator_error = None;
    let creator = match creator.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code)
            if crate::feature_modules::is_enabled(pool, crate::creator_codes::MODULE).await? =>
        {
            match crate::creator_codes::check(pool, code, customer).await {
                Ok(c) => Some(c),
                Err(AppError::Validation(k)) => {
                    creator_error = Some(k);
                    None
                }
                Err(e) => return Err(e),
            }
        }
        _ => None,
    };
    let creator_pct = creator
        .as_ref()
        .map_or(Decimal::ZERO, |c| c.discount_percent);
    let mut creator_total = Decimal::ZERO;
    let (cust, group_pct) = match customer {
        Some(cid) => {
            let c: Option<TaxCust> = sqlx::query_as(
                "SELECT country, region, vat_number, tax_exempt FROM customers WHERE id = $1",
            )
            .bind(cid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
            let pct: Option<Decimal> = sqlx::query_scalar(
                "SELECT g.discount_percent FROM customers c JOIN client_groups g ON g.id = c.client_group_id WHERE c.id = $1 AND g.discount_percent > 0",
            )
            .bind(cid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
            (c, pct)
        }
        None => (None, None),
    };
    let cust = cust.unwrap_or(TaxCust {
        country: country.to_ascii_uppercase(),
        region: String::new(),
        vat_number: String::new(),
        tax_exempt: false,
    });
    let mut lines = Vec::with_capacity(items.len());
    let mut amounts: Vec<Decimal> = vec![];
    let mut discount = Decimal::ZERO;
    let mut group = Decimal::ZERO;
    let mut sale_total = Decimal::ZERO;
    let rules_on = crate::store_rules::enabled(pool).await?;
    let rules = crate::store_rules::for_cart(pool, items).await?;
    crate::store_rules::check_lines(&rules, items, rules_on)?;
    let bundle = best_bundle(pool, items).await?;
    let mut bundle_total = Decimal::ZERO;
    // Timed sales, clamped like `orders::place_full` (after promo and group).
    let sales = if crate::feature_modules::is_enabled(pool, crate::timed_sales::MODULE).await? {
        crate::timed_sales::active(pool).await?
    } else {
        vec![]
    };
    let product_groups: std::collections::HashMap<Uuid, Uuid> = if sales.is_empty() {
        Default::default()
    } else {
        let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
        crate::timed_sales::groups(pool, &ids)
            .await?
            .into_iter()
            .collect()
    };
    for (idx, it) in items.iter().enumerate() {
        let mut q =
            catalog::quote(&mut tx, it.product_id, &it.cycle, &currency, &it.config).await?;
        q.amount = crate::store_rules::line_amount(
            rules.get(&it.product_id),
            &q.cycle,
            q.amount,
            it.pay_amount,
        )?;
        let extras =
            addons::quote(&mut tx, it.product_id, &it.cycle, &currency, &it.addons).await?;
        let d = match &promo {
            Some(c) if c.applies_to(q.product_id) => {
                coupons::line_discount(&c.kind, c.value, q.amount, exp)
            }
            _ => Decimal::ZERO,
        };
        discount += d;
        // Bundle: after the promo code, like `orders::place_full`.
        let bundle_line = match &bundle {
            Some(b) if b.items.contains(&idx) => {
                coupons::line_discount("percent", b.percent, (q.amount - d).max(Decimal::ZERO), exp)
            }
            _ => Decimal::ZERO,
        };
        bundle_total += bundle_line;
        let mut group_line = Decimal::ZERO;
        if let Some(pct) = group_pct {
            let base = q.amount - d + extras.iter().map(|a| a.amount).sum::<Decimal>();
            group_line = coupons::line_discount("percent", pct, base, exp);
            group += group_line;
        }
        let sale = product_groups
            .get(&q.product_id)
            .and_then(|g| crate::timed_sales::best(&sales, q.product_id, *g));
        let sale_discount = sale.map_or(Decimal::ZERO, |x| {
            (q.amount * x.percent / Decimal::from(100))
                .round_dp(exp)
                .min((q.amount - d - bundle_line - group_line).max(Decimal::ZERO))
        });
        sale_total += sale_discount;
        // Creator code: after the sale, same clamp as `orders::place_full`.
        let creator_discount = (q.amount * creator_pct / Decimal::from(100))
            .round_dp(exp)
            .min((q.amount - d - bundle_line - group_line - sale_discount).max(Decimal::ZERO));
        creator_total += creator_discount;
        amounts.push(q.amount);
        if q.setup_fee > Decimal::ZERO {
            amounts.push(q.setup_fee);
        }
        for a in &extras {
            amounts.push(a.amount);
            if a.setup_fee > Decimal::ZERO {
                amounts.push(a.setup_fee);
            }
        }
        lines.push(PreviewLine {
            product_id: q.product_id,
            product_name: q.product_name,
            cycle: q.cycle,
            amount: q.amount,
            setup_fee: q.setup_fee,
            labels: q.labels,
            addons: extras
                .into_iter()
                .map(|a| PreviewAddon {
                    addon_id: a.addon_id,
                    name: a.name,
                    cycle: a.cycle,
                    amount: a.amount,
                    setup_fee: a.setup_fee,
                })
                .collect(),
            discount: d,
            sale_discount,
            sale: sale
                .filter(|_| sale_discount > Decimal::ZERO)
                .map(|x| x.name.clone()),
            creator_discount,
            bundle_discount: bundle_line,
        });
    }
    drop(tx); // read-only; rolled back
    let promo_code = promo.as_ref().map(|c| c.code.clone());
    if promo.is_some() && discount <= Decimal::ZERO && !items.is_empty() {
        coupon_error = Some("error.coupon_not_applicable");
    }
    if discount > Decimal::ZERO {
        amounts.push(-discount);
    }
    if bundle_total > Decimal::ZERO {
        amounts.push(-bundle_total);
    }
    if group > Decimal::ZERO {
        amounts.push(-group);
    }
    if sale_total > Decimal::ZERO {
        amounts.push(-sale_total);
    }
    if creator_total > Decimal::ZERO {
        amounts.push(-creator_total);
    }
    let cfg = tax::get_settings(pool).await?;
    let reverse = cfg.enabled
        && cfg.reverse_charge
        && !cust.tax_exempt
        && engine::reverse_charge(
            &cfg.home_country,
            &cust.country,
            &cust.vat_number,
            &FormatOnly,
        );
    let rules: Vec<Rule> = if cfg.enabled && !cust.tax_exempt {
        let all = tax::list_rules(pool).await?;
        tax::select(&all, &cust.country, &cust.region)
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
    let inclusive = cfg.enabled && cfg.inclusive;
    let (mut net, mut tax_total, mut taxes) = (Decimal::ZERO, Decimal::ZERO, Vec::new());
    for a in amounts {
        let v = round(a, exp);
        let l = if inclusive {
            engine::on_gross(v, &rules, exp)
        } else {
            engine::on_net(v, &rules, exp)
        }
        .map_err(|_| AppError::Validation("error.invalid_amount"))?;
        engine::merge(&mut taxes, &l.parts);
        net += l.net;
        tax_total += l.tax;
    }
    let subtotal = lines
        .iter()
        .map(|l| {
            l.amount
                + l.setup_fee
                + l.addons
                    .iter()
                    .map(|a| a.amount + a.setup_fee)
                    .sum::<Decimal>()
        })
        .sum();
    Ok(Preview {
        currency,
        lines,
        subtotal,
        discount,
        group_discount: group,
        sale_discount: sale_total,
        creator_discount: creator_total,
        tax: tax_total,
        taxes,
        total: (net + tax_total).max(Decimal::ZERO),
        inclusive,
        coupon: promo_code,
        coupon_error,
        creator_code: creator.map(|c| c.code),
        creator_error,
        bundle: bundle
            .filter(|_| bundle_total > Decimal::ZERO)
            .map(|b| b.name),
        bundle_discount: bundle_total,
    })
}

// contacts

pub const CONTACT_PERMS: &[&str] = &["invoices", "tickets", "services", "orders", "domains"];

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Contact {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    pub permissions: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const CONTACT_COLS: &str =
    "c.id, c.customer_id, c.user_id, u.email::text AS email, c.name, c.permissions, c.created_at";

pub fn check_perms(p: &[String]) -> AppResult<Vec<String>> {
    let mut v: Vec<String> = p.iter().map(|s| s.trim().to_string()).collect();
    if v.iter().any(|s| !CONTACT_PERMS.contains(&s.as_str())) {
        return Err(AppError::Validation("error.invalid_permission"));
    }
    v.sort();
    v.dedup();
    Ok(v)
}

pub async fn contacts(pool: &PgPool, customer: Uuid) -> AppResult<Vec<Contact>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CONTACT_COLS} FROM customer_subaccounts c JOIN users u ON u.id = c.user_id WHERE c.customer_id = $1 ORDER BY c.created_at"
    )))
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn mark_invited(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE customer_subaccounts SET invited_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn contact_for_user(pool: &PgPool, user: Uuid) -> AppResult<Option<Contact>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CONTACT_COLS} FROM customer_subaccounts c JOIN users u ON u.id = c.user_id WHERE c.user_id = $1"
    )))
    .bind(user)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn add_contact(
    pool: &PgPool,
    customer: Uuid,
    user: Uuid,
    name: &str,
    perms: &[String],
) -> AppResult<Contact> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO customer_subaccounts (id, customer_id, user_id, name, permissions) VALUES ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(customer)
        .bind(user)
        .bind(name.trim().chars().take(100).collect::<String>())
        .bind(perms)
        .execute(pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.contact_exists"),
            _ => db_err(e),
        })?;
    contact_for_user(pool, user)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn update_contact(
    pool: &PgPool,
    customer: Uuid,
    id: Uuid,
    name: &str,
    perms: &[String],
) -> AppResult<bool> {
    let r = sqlx::query("UPDATE customer_subaccounts SET name = $3, permissions = $4 WHERE id = $1 AND customer_id = $2")
        .bind(id)
        .bind(customer)
        .bind(name.trim().chars().take(100).collect::<String>())
        .bind(perms)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Remove a contact; returns the contact's user id so the caller can
/// revoke its sessions.
pub async fn remove_contact(pool: &PgPool, customer: Uuid, id: Uuid) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "DELETE FROM customer_subaccounts WHERE id = $1 AND customer_id = $2 RETURNING user_id",
    )
    .bind(id)
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

// email prefs

#[derive(Debug, Clone, Copy, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailPrefs {
    pub invoices: bool,
    pub services: bool,
    pub support: bool,
    pub marketing: bool,
}

impl Default for EmailPrefs {
    fn default() -> Self {
        Self {
            invoices: true,
            services: true,
            support: true,
            marketing: true,
        }
    }
}

pub async fn email_prefs(pool: &PgPool, customer: Uuid) -> AppResult<EmailPrefs> {
    Ok(sqlx::query_as("SELECT invoices, services, support, marketing FROM customer_email_prefs WHERE customer_id = $1")
        .bind(customer)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .unwrap_or_default())
}

pub async fn put_email_prefs(pool: &PgPool, customer: Uuid, p: &EmailPrefs) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO customer_email_prefs (customer_id, invoices, services, support, marketing) VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (customer_id) DO UPDATE SET invoices = $2, services = $3, support = $4, marketing = $5, updated_at = now()",
    )
    .bind(customer)
    .bind(p.invoices)
    .bind(p.services)
    .bind(p.support)
    .bind(p.marketing)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

// login history

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct LoginEvent {
    pub action: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn login_history(pool: &PgPool, user: Uuid, limit: i64) -> AppResult<Vec<LoginEvent>> {
    sqlx::query_as(
        "SELECT action, ip,
                COALESCE(metadata->>'user_agent', (SELECT s.user_agent FROM sessions s WHERE s.id::text = audit_logs.target)) AS user_agent,
                created_at FROM audit_logs
         WHERE actor_id = $1 AND action IN ('auth.login','auth.login_failed','auth.login_locked','auth.logout')
         ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(user)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// order notes

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct OrderNote {
    pub id: Uuid,
    pub author_id: Option<Uuid>,
    pub author: Option<String>,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn order_notes(pool: &PgPool, order: Uuid) -> AppResult<Vec<OrderNote>> {
    sqlx::query_as(
        "SELECT n.id, n.author_id, u.display_name AS author, n.body, n.created_at FROM order_notes n
         LEFT JOIN users u ON u.id = n.author_id WHERE n.order_id = $1 ORDER BY n.created_at, n.id",
    )
    .bind(order)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add_order_note(
    pool: &PgPool,
    order: Uuid,
    author: Uuid,
    body: &str,
) -> AppResult<Uuid> {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 5000 {
        return Err(AppError::Validation("error.invalid_note"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO order_notes (id, order_id, author_id, body) SELECT $1, o.id, $3, $4 FROM orders o WHERE o.id = $2")
        .bind(id)
        .bind(order)
        .bind(author)
        .bind(body)
        .execute(pool)
        .await
        .map_err(db_err)
        .and_then(|r| if r.rows_affected() == 1 { Ok(()) } else { Err(AppError::NotFound) })?;
    Ok(id)
}

/// Latest fraud screening for an order, if any.
pub async fn order_fraud(pool: &PgPool, order: Uuid) -> AppResult<Option<Value>> {
    sqlx::query_scalar(
        "SELECT to_jsonb(f) - 'customer_id' FROM fraud_checks f WHERE f.order_id = $1 ORDER BY f.created_at DESC LIMIT 1",
    )
    .bind(order)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Close a paid order after its invoice was credited (refund).
pub async fn mark_order_refunded(pool: &PgPool, order: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE orders SET status = 'cancelled' WHERE id = $1 AND status <> 'fraud'")
        .bind(order)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

// client home widgets

/// Which widgets a caller may see; contacts only get the areas they were
/// granted (announcements and KB are public anyway).
#[derive(Debug, Clone, Copy)]
pub struct HomeScope {
    pub services: bool,
    pub invoices: bool,
    pub tickets: bool,
}

async fn json_rows(pool: &PgPool, sql: &str, customer: Uuid, on: bool) -> AppResult<Value> {
    if !on {
        return Ok(Value::Null);
    }
    sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(format!(
        "SELECT COALESCE(json_agg(t), '[]'::json)::jsonb FROM ({sql}) t"
    )))
    .bind(customer)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Client home: active services, unpaid invoices, open tickets, latest
/// announcements and suggested knowledge-base articles (locale-aware).
pub async fn home(
    pool: &PgPool,
    customer: Uuid,
    scope: HomeScope,
    locale: &str,
) -> AppResult<Value> {
    let services = json_rows(
        pool,
        "SELECT s.id, p.name AS product_name, s.status, s.cycle, s.currency::text AS currency, s.recurring_amount, s.next_due_date
         FROM services s JOIN products p ON p.id = s.product_id
         WHERE s.customer_id = $1 AND s.status IN ('active','suspended','pending') ORDER BY s.next_due_date NULLS LAST, s.id LIMIT 5",
        customer,
        scope.services,
    )
    .await?;
    let invoices = json_rows(
        pool,
        "SELECT id, number, currency::text AS currency, total, amount_paid, due_at, (due_at < now()) AS overdue
         FROM invoices WHERE customer_id = $1 AND status = 'unpaid' ORDER BY due_at, id LIMIT 5",
        customer,
        scope.invoices,
    )
    .await?;
    let tickets = json_rows(
        pool,
        "SELECT id, number, subject, status, priority, last_reply_at FROM tickets
         WHERE customer_id = $1 AND status <> 'closed' ORDER BY last_reply_at DESC, id LIMIT 5",
        customer,
        scope.tickets,
    )
    .await?;
    let announcements: Value = sqlx::query_scalar(
        "SELECT COALESCE(json_agg(t), '[]'::json)::jsonb FROM (
           SELECT id, title, left(body, 240) AS excerpt, published_at FROM announcements
           WHERE is_published AND published_at <= now() AND (locale IS NULL OR locale = $1)
           ORDER BY published_at DESC LIMIT 3) t",
    )
    .bind(locale)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    // Suggest articles matching the names of the customer's products first,
    // then the most helpful/viewed ones.
    let kb: Value = sqlx::query_scalar(
        "SELECT COALESCE(json_agg(t), '[]'::json)::jsonb FROM (
           SELECT a.id, a.slug, a.title FROM kb_articles a
           WHERE a.is_published AND (a.locale IS NULL OR a.locale = $2)
           ORDER BY (SELECT bool_or(a.search @@ plainto_tsquery('simple', p.name)) FROM services s JOIN products p ON p.id = s.product_id
                     WHERE s.customer_id = $1 AND s.status = 'active') IS TRUE DESC,
                    a.helpful_yes - a.helpful_no DESC, a.views DESC, a.id
           LIMIT 5) t",
    )
    .bind(customer)
    .bind(locale)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok(serde_json::json!({
        "services": services, "invoices": invoices, "tickets": tickets,
        "announcements": announcements, "kb": kb,
    }))
}

// helpers

pub async fn set_country_if_empty(pool: &PgPool, customer: Uuid, country: &str) -> AppResult<()> {
    sqlx::query("UPDATE customers SET country = $2 WHERE id = $1 AND country = ''")
        .bind(customer)
        .bind(country)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Services created from an order's lines.
pub async fn order_services(pool: &PgPool, order: Uuid) -> AppResult<Value> {
    sqlx::query_scalar(
        "SELECT COALESCE(json_agg(t), '[]'::json)::jsonb FROM (
           SELECT s.id, p.name AS product_name, s.status FROM services s JOIN order_items oi ON oi.id = s.order_item_id
           JOIN products p ON p.id = s.product_id WHERE oi.order_id = $1 ORDER BY s.created_at) t",
    )
    .bind(order)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn contact_count(pool: &PgPool, customer: Uuid) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM customer_subaccounts WHERE customer_id = $1")
        .bind(customer)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn disable_user(pool: &PgPool, user: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(user)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
