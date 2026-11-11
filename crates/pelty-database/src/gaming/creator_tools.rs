//! Creator tools for the marketplace (module `creator_marketplace`):
//! creator-owned discount codes and bundles, wishlists, buyer <-> creator
//! conversations and per-listing analytics (views, sales, revenue).
use crate::bundles::{self, Bundle, BundleInput, ItemInput};
use crate::coupons::{self, Coupon, CouponInput};
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// Ids of every product the creator lists (any review status).
pub async fn creator_products(pool: &PgPool, creator: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar("SELECT product_id FROM creator_products WHERE creator_id = $1")
        .bind(creator)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// 422 unless `ids` is a non-empty set of the creator's own listings.
async fn own_products(pool: &PgPool, creator: Uuid, ids: &[Uuid]) -> AppResult<()> {
    let mine = creator_products(pool, creator).await?;
    if ids.is_empty() || ids.len() > 200 || ids.iter().any(|p| !mine.contains(p)) {
        return Err(AppError::Validation("error.creator_products_invalid"));
    }
    Ok(())
}

// ------------------------------------------------------------- discount codes

/// What a creator may set on a discount code. Always limited to their own
/// listings; never recurring, never for domains, addons or specific customers.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatorCouponInput {
    /// Without the creator prefix; `NOVA-` is added for creator `nova`.
    pub code: String,
    #[serde(default)]
    pub description: String,
    /// `percent` or `fixed`.
    pub kind: String,
    #[schema(value_type = String)]
    pub value: Decimal,
    #[serde(default)]
    pub currency: Option<String>,
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
    /// Listings it works on (at least one of the creator's own).
    pub product_ids: Vec<Uuid>,
}

fn yes() -> bool {
    true
}

/// `NOVA-SPRING` for slug `nova` and code `spring` (prefix not doubled).
pub fn prefixed_code(slug: &str, code: &str) -> String {
    let prefix = format!("{}-", slug.to_ascii_uppercase());
    let code = coupons::normalize(code);
    if code.starts_with(&prefix) {
        code
    } else {
        format!("{prefix}{code}")
    }
}

impl CreatorCouponInput {
    fn to_coupon(&self, slug: &str) -> CouponInput {
        CouponInput {
            code: prefixed_code(slug, &self.code),
            description: self.description.clone(),
            kind: self.kind.clone(),
            value: self.value,
            currency: self.currency.clone(),
            recurring: false,
            max_uses: self.max_uses,
            max_uses_per_customer: self.max_uses_per_customer,
            starts_at: self.starts_at,
            expires_at: self.expires_at,
            active: self.active,
            product_ids: self.product_ids.clone(),
            recurring_cycles: None,
            applies_addons: false,
            applies_domains: false,
            tlds: vec![],
            customer_ids: vec![],
            client_group_ids: vec![],
        }
    }
}

pub async fn coupons(pool: &PgPool, creator: Uuid) -> AppResult<Vec<Coupon>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM coupons WHERE creator_id = $1 ORDER BY created_at DESC LIMIT 500",
    )
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(c) = coupons::get(pool, id).await? {
            out.push(c);
        }
    }
    Ok(out)
}

async fn coupon_owner(pool: &PgPool, id: Uuid) -> AppResult<Option<Uuid>> {
    Ok(
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT creator_id FROM coupons WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .flatten(),
    )
}

/// Create (`id = None`) or update one of the creator's codes.
pub async fn save_coupon(
    pool: &PgPool,
    creator: Uuid,
    slug: &str,
    id: Option<Uuid>,
    i: &CreatorCouponInput,
) -> AppResult<Coupon> {
    own_products(pool, creator, &i.product_ids).await?;
    if let Some(id) = id {
        if coupon_owner(pool, id).await? != Some(creator) {
            return Err(AppError::NotFound);
        }
    }
    let c = coupons::save(pool, id, &i.to_coupon(slug))
        .await
        .map_err(|e| match e {
            AppError::Conflict("error.conflict") => AppError::Conflict("error.coupon_code_taken"),
            e => e,
        })?;
    if id.is_none() {
        sqlx::query("UPDATE coupons SET creator_id = $2 WHERE id = $1")
            .bind(c.id)
            .bind(creator)
            .execute(pool)
            .await
            .map_err(db_err)?;
    }
    Ok(c)
}

/// Delete an unused code; a used one is switched off instead.
pub async fn delete_coupon(pool: &PgPool, creator: Uuid, id: Uuid) -> AppResult<bool> {
    if coupon_owner(pool, id).await? != Some(creator) {
        return Err(AppError::NotFound);
    }
    match coupons::delete(pool, id).await {
        Ok(()) => Ok(true),
        Err(AppError::Conflict(_)) => {
            sqlx::query("UPDATE coupons SET active = false WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .map_err(db_err)?;
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

// ------------------------------------------------------------------- bundles

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatorBundleInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "yes")]
    pub active: bool,
    #[schema(value_type = String)]
    pub discount_percent: Decimal,
    /// 2 to 10 of the creator's own listings.
    pub product_ids: Vec<Uuid>,
}

/// Bundle input with each listing's own billing cycle (first price).
async fn bundle_input(
    pool: &PgPool,
    creator: Uuid,
    i: &CreatorBundleInput,
) -> AppResult<BundleInput> {
    own_products(pool, creator, &i.product_ids).await?;
    let mut items = Vec::with_capacity(i.product_ids.len());
    for p in &i.product_ids {
        let cycle: Option<String> = sqlx::query_scalar(
            "SELECT cycle FROM product_prices WHERE product_id = $1 ORDER BY (cycle = 'one_time') DESC, cycle LIMIT 1",
        )
        .bind(p)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        items.push(ItemInput {
            product_id: *p,
            cycle: cycle.ok_or(AppError::Validation("error.creator_products_invalid"))?,
        });
    }
    Ok(BundleInput {
        name: i.name.clone(),
        description: i.description.clone(),
        active: i.active,
        discount_percent: i.discount_percent,
        items,
    })
}

pub async fn bundles(pool: &PgPool, creator: Uuid) -> AppResult<Vec<Bundle>> {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM bundles WHERE creator_id = $1 ORDER BY name")
            .bind(creator)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(b) = bundles::get(pool, id, false).await? {
            out.push(b);
        }
    }
    Ok(out)
}

async fn bundle_owner(pool: &PgPool, id: Uuid) -> AppResult<Option<Uuid>> {
    Ok(
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT creator_id FROM bundles WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .flatten(),
    )
}

pub async fn save_bundle(
    pool: &PgPool,
    creator: Uuid,
    id: Option<Uuid>,
    i: &CreatorBundleInput,
) -> AppResult<Bundle> {
    if i.product_ids.len() < 2 {
        return Err(AppError::Validation("error.bundle_invalid"));
    }
    let input = bundle_input(pool, creator, i).await?;
    let id = match id {
        Some(id) => {
            if bundle_owner(pool, id).await? != Some(creator) {
                return Err(AppError::NotFound);
            }
            bundles::update(pool, id, &input).await?;
            id
        }
        None => {
            let id = bundles::create(pool, &input).await?;
            sqlx::query("UPDATE bundles SET creator_id = $2 WHERE id = $1")
                .bind(id)
                .bind(creator)
                .execute(pool)
                .await
                .map_err(db_err)?;
            id
        }
    };
    bundles::get(pool, id, false)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn delete_bundle(pool: &PgPool, creator: Uuid, id: Uuid) -> AppResult<()> {
    if bundle_owner(pool, id).await? != Some(creator) {
        return Err(AppError::NotFound);
    }
    bundles::delete(pool, id).await
}

/// Active creator bundles that contain `product` (resource page sidebar).
pub async fn bundles_with(pool: &PgPool, product: Uuid) -> AppResult<Vec<Bundle>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT b.id FROM bundles b JOIN bundle_items bi ON bi.bundle_id = b.id
          WHERE bi.product_id = $1 AND b.active AND b.creator_id IS NOT NULL ORDER BY b.discount_percent DESC LIMIT 5",
    )
    .bind(product)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(b) = bundles::get(pool, id, true).await? {
            if b.items.len() >= 2 {
                out.push(b);
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ wishlist

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct WishlistItem {
    pub product_id: Uuid,
    pub name: String,
    pub image_url: String,
    pub creator: Option<String>,
    pub creator_slug: Option<String>,
    /// False once the product is retired or hidden.
    pub available: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

pub const WISHLIST_MAX: i64 = 200;

pub async fn wishlist(pool: &PgPool, customer: Uuid) -> AppResult<Vec<WishlistItem>> {
    sqlx::query_as(
        "SELECT w.product_id, p.name, p.image_url, c.display_name AS creator, c.slug AS creator_slug,
                (p.status = 'active') AS available, w.created_at
           FROM wishlist_items w JOIN products p ON p.id = w.product_id
           LEFT JOIN creator_products cp ON cp.product_id = p.id AND cp.status = 'approved'
           LEFT JOIN creators c ON c.id = cp.creator_id
          WHERE w.customer_id = $1 ORDER BY w.created_at DESC",
    )
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Save an active product; 404 for hidden or unknown products.
pub async fn wish(pool: &PgPool, customer: Uuid, product: Uuid) -> AppResult<()> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM products WHERE id = $1 AND status = 'active')",
    )
    .bind(product)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if !ok {
        return Err(AppError::NotFound);
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM wishlist_items WHERE customer_id = $1")
        .bind(customer)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if n >= WISHLIST_MAX {
        return Err(AppError::Validation("error.wishlist_full"));
    }
    sqlx::query("INSERT INTO wishlist_items (customer_id, product_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(customer)
        .bind(product)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn unwish(pool: &PgPool, customer: Uuid, product: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM wishlist_items WHERE customer_id = $1 AND product_id = $2")
        .bind(customer)
        .bind(product)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// How many customers saved each of these products.
pub async fn wish_counts(pool: &PgPool, products: &[Uuid]) -> AppResult<Vec<(Uuid, i64)>> {
    sqlx::query_as("SELECT product_id, count(*) FROM wishlist_items WHERE product_id = ANY($1) GROUP BY product_id")
        .bind(products)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

// ------------------------------------------------------------- conversations

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Conversation {
    pub id: Uuid,
    pub creator_id: Uuid,
    pub creator: String,
    pub creator_slug: String,
    pub customer_id: Uuid,
    /// Buyer's display name (first name + last initial).
    pub buyer: String,
    pub product_id: Option<Uuid>,
    pub product: Option<String>,
    /// The buyer had paid for the product when the conversation started.
    pub verified_buyer: bool,
    pub subject: String,
    pub status: String,
    pub buyer_unread: i32,
    pub creator_unread: i32,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub last_message_at: OffsetDateTime,
}

const CVCOLS: &str = "m.id, m.creator_id, c.display_name AS creator, c.slug AS creator_slug, m.customer_id,
    trim(cu.first_name || ' ' || left(cu.last_name, 1)) AS buyer, m.product_id, p.name AS product,
    (m.invoice_id IS NOT NULL) AS verified_buyer, m.subject, m.status, m.buyer_unread, m.creator_unread, m.last_message_at";
const CVFROM: &str = "market_conversations m JOIN creators c ON c.id = m.creator_id JOIN customers cu ON cu.id = m.customer_id
    LEFT JOIN products p ON p.id = m.product_id";

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Message {
    pub id: Uuid,
    pub from_creator: bool,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

/// Conversations where the user is the buyer (`customer`) or the creator.
pub async fn conversations(
    pool: &PgPool,
    customer: Option<Uuid>,
    creator: Option<Uuid>,
) -> AppResult<Vec<Conversation>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CVCOLS} FROM {CVFROM} WHERE m.customer_id = $1 OR m.creator_id = $2 ORDER BY m.last_message_at DESC LIMIT 200"
    )))
    .bind(customer)
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn conversation(pool: &PgPool, id: Uuid) -> AppResult<Conversation> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CVCOLS} FROM {CVFROM} WHERE m.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn messages(pool: &PgPool, conversation: Uuid) -> AppResult<Vec<Message>> {
    sqlx::query_as("SELECT id, from_creator, body, created_at FROM market_messages WHERE conversation_id = $1 ORDER BY created_at, id LIMIT 1000")
        .bind(conversation)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Unread conversations for the buyer side and the creator side.
pub async fn unread(
    pool: &PgPool,
    customer: Option<Uuid>,
    creator: Option<Uuid>,
) -> AppResult<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM market_conversations
          WHERE (customer_id = $1 AND buyer_unread > 0) OR (creator_id = $2 AND creator_unread > 0)",
    )
    .bind(customer)
    .bind(creator)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub const NEW_PER_DAY: i64 = 20;

fn clean_body(body: &str) -> AppResult<&str> {
    let b = body.trim();
    if b.is_empty() || b.chars().count() > 5000 {
        return Err(AppError::Validation("error.message_invalid"));
    }
    Ok(b)
}

/// A buyer opens a conversation about an approved listing.
pub async fn start(
    pool: &PgPool,
    customer: Uuid,
    author: Uuid,
    product: Uuid,
    subject: &str,
    body: &str,
) -> AppResult<Uuid> {
    let body = clean_body(body)?;
    let subject = subject.trim();
    if subject.is_empty() || subject.chars().count() > 150 {
        return Err(AppError::Validation("error.message_invalid"));
    }
    let creator: (Uuid, Uuid) = sqlx::query_as(
        "SELECT c.id, c.customer_id FROM creator_products cp JOIN creators c ON c.id = cp.creator_id
          WHERE cp.product_id = $1 AND cp.status = 'approved' AND c.status = 'approved'",
    )
    .bind(product)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)?;
    if creator.1 == customer {
        return Err(AppError::Validation("error.message_self"));
    }
    let today: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM market_conversations WHERE customer_id = $1 AND created_at > now() - interval '1 day'",
    )
    .bind(customer)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if today >= NEW_PER_DAY {
        return Err(AppError::RateLimited);
    }
    let invoice: Option<Uuid> = sqlx::query_scalar(
        "SELECT i.id FROM invoices i JOIN order_items oi ON oi.order_id = i.order_id
          WHERE i.customer_id = $1 AND oi.product_id = $2 AND i.status = 'paid' ORDER BY i.paid_at DESC NULLS LAST LIMIT 1",
    )
    .bind(customer)
    .bind(product)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("INSERT INTO market_conversations (id, creator_id, customer_id, product_id, invoice_id, subject, creator_unread) VALUES ($1,$2,$3,$4,$5,$6,1)")
        .bind(id)
        .bind(creator.0)
        .bind(customer)
        .bind(product)
        .bind(invoice)
        .bind(subject)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO market_messages (id, conversation_id, author_user_id, from_creator, body) VALUES ($1,$2,$3,false,$4)")
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(author)
        .bind(body)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

/// Reply as the buyer or the creator. Returns the other side's unread count
/// before this message (0 means: notify them).
pub async fn reply(
    pool: &PgPool,
    conversation: Uuid,
    author: Uuid,
    from_creator: bool,
    body: &str,
) -> AppResult<i32> {
    let body = clean_body(body)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let (status, other_unread): (String, i32) = sqlx::query_as(if from_creator {
        "SELECT status, buyer_unread FROM market_conversations WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT status, creator_unread FROM market_conversations WHERE id = $1 FOR UPDATE"
    })
    .bind(conversation)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)?;
    if status != "open" {
        return Err(AppError::Conflict("error.conversation_closed"));
    }
    sqlx::query("INSERT INTO market_messages (id, conversation_id, author_user_id, from_creator, body) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(conversation)
        .bind(author)
        .bind(from_creator)
        .bind(body)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query(if from_creator {
        "UPDATE market_conversations SET buyer_unread = buyer_unread + 1, creator_unread = 0, last_message_at = now() WHERE id = $1"
    } else {
        "UPDATE market_conversations SET creator_unread = creator_unread + 1, buyer_unread = 0, last_message_at = now() WHERE id = $1"
    })
    .bind(conversation)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(other_unread)
}

pub async fn mark_read(pool: &PgPool, conversation: Uuid, creator_side: bool) -> AppResult<()> {
    sqlx::query(if creator_side {
        "UPDATE market_conversations SET creator_unread = 0 WHERE id = $1"
    } else {
        "UPDATE market_conversations SET buyer_unread = 0 WHERE id = $1"
    })
    .bind(conversation)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn set_status(pool: &PgPool, conversation: Uuid, open: bool) -> AppResult<()> {
    sqlx::query("UPDATE market_conversations SET status = $2 WHERE id = $1")
        .bind(conversation)
        .bind(if open { "open" } else { "closed" })
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Who to mail about a new message: (email, name, locale) of the buyer or
/// of the creator's user account.
pub async fn recipient(
    pool: &PgPool,
    conversation: Uuid,
    to_creator: bool,
) -> AppResult<Option<(String, String, Option<String>)>> {
    sqlx::query_as(if to_creator {
        "SELECT u.email::text, c.display_name, u.locale FROM market_conversations m JOIN creators c ON c.id = m.creator_id
           JOIN users u ON u.id = c.user_id WHERE m.id = $1 AND u.is_active"
    } else {
        "SELECT COALESCE(NULLIF(c.email::text, ''), u.email::text), COALESCE(NULLIF(trim(c.first_name), ''), u.display_name, ''), u.locale
           FROM market_conversations m JOIN customers c ON c.id = m.customer_id LEFT JOIN users u ON u.id = c.owner_user_id
          WHERE m.id = $1 AND c.closed_at IS NULL"
    })
    .bind(conversation)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

// ----------------------------------------------------------------- analytics

/// Count one resource page view (today, server time).
pub async fn record_view(pool: &PgPool, product: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO product_views (product_id, day, views) VALUES ($1, CURRENT_DATE, 1)
                 ON CONFLICT (product_id, day) DO UPDATE SET views = product_views.views + 1",
    )
    .bind(product)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct DayStats {
    #[schema(value_type = String)]
    pub day: Date,
    pub views: i64,
    pub sales: i64,
    /// Gross sales (net of tax, after discounts) in `currency`.
    #[schema(value_type = String)]
    pub revenue: Decimal,
    /// What the creator earned after commission.
    #[schema(value_type = String)]
    pub earnings: Decimal,
}

/// One row per day for the last `days` days (oldest first), sales in `currency`.
pub async fn listing_stats(
    pool: &PgPool,
    product: Uuid,
    currency: &str,
    days: i64,
) -> AppResult<Vec<DayStats>> {
    sqlx::query_as(
        "WITH d AS (SELECT generate_series(CURRENT_DATE - ($3::int - 1), CURRENT_DATE, interval '1 day')::date AS day)
         SELECT d.day,
                COALESCE((SELECT views FROM product_views v WHERE v.product_id = $1 AND v.day = d.day), 0)::BIGINT AS views,
                COALESCE((SELECT count(*) FROM creator_ledger l WHERE l.product_id = $1 AND l.kind = 'sale' AND l.created_at::date = d.day), 0)::BIGINT AS sales,
                COALESCE((SELECT sum(gross) FROM creator_ledger l WHERE l.product_id = $1 AND l.kind IN ('sale','refund') AND l.currency = $2 AND l.created_at::date = d.day), 0) AS revenue,
                COALESCE((SELECT sum(amount) FROM creator_ledger l WHERE l.product_id = $1 AND l.kind IN ('sale','refund') AND l.currency = $2 AND l.created_at::date = d.day), 0) AS earnings
           FROM d ORDER BY d.day",
    )
    .bind(product)
    .bind(currency)
    .bind(days.clamp(1, 365) as i32)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Sales per view in percent, two decimals; 0 without views.
pub fn conversion(views: i64, sales: i64) -> Decimal {
    if views <= 0 {
        return Decimal::ZERO;
    }
    (Decimal::from(sales) * Decimal::ONE_HUNDRED / Decimal::from(views)).round_dp(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_prefix() {
        assert_eq!(prefixed_code("nova", " spring "), "NOVA-SPRING");
        assert_eq!(prefixed_code("nova", "NOVA-SPRING"), "NOVA-SPRING");
        assert_eq!(prefixed_code("kade-dev", "x10"), "KADE-DEV-X10");
    }

    #[test]
    fn conversion_rate() {
        assert_eq!(conversion(0, 3), Decimal::ZERO);
        assert_eq!(conversion(200, 3), "1.5".parse::<Decimal>().unwrap());
        assert_eq!(conversion(3, 1), "33.33".parse::<Decimal>().unwrap());
    }
}
