//! Creator marketplace (module `creator_marketplace`): creators apply and are
//! approved, list products (catalog products owned by the creator) for admin
//! review, earn each sale minus commission into a ledger held for a refund
//! window, and request payouts. The `mode` setting turns creator sign-ups and
//! listings off (`admin_only`) without losing data.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "creator_marketplace";
pub const PAYOUT_METHODS: &[&str] = &["manual", "stripe_connect", "paypal"];
/// When automatic payouts are created: never (creators request them), every
/// week on `payout_weekday` or every month on `payout_monthday`.
pub const PAYOUT_SCHEDULES: &[&str] = &["manual", "weekly", "monthly"];

/// Marketplace settings (stored in `feature_modules.settings`).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[schema(as = MarketplaceConfig)]
pub struct Config {
    /// `multi_vendor` (creators can sell) or `admin_only` (store only).
    pub mode: String,
    #[schema(value_type = String)]
    pub commission_percent: Decimal,
    /// Days a sale is held before it can be paid out (refund window).
    pub hold_days: i64,
    #[schema(value_type = String)]
    pub min_payout: Decimal,
    pub auto_approve_creators: bool,
    pub auto_approve_listings: bool,
    /// One of [`PAYOUT_SCHEDULES`].
    pub payout_schedule: String,
    /// ISO weekday (1 = Monday … 7 = Sunday) for weekly payouts.
    pub payout_weekday: u8,
    /// Day of month (1–28) for monthly payouts.
    pub payout_monthday: u8,
    /// Payouts wait in `requested` until staff send them. Off: payouts to
    /// Stripe Connect / PayPal are sent automatically.
    pub payout_approval: bool,
    /// Payout methods creators may pick (staff can still assign any).
    pub payout_methods: Vec<String>,
    /// Pull money back from paid Stripe transfers when a refund or
    /// chargeback leaves the creator's balance negative.
    pub reverse_transfers_on_refund: bool,
}

impl Config {
    /// Lenient parse with defaults and clamping (settings are admin JSON).
    pub fn from_value(v: &Value) -> Self {
        let dec = |k: &str, d: Decimal| {
            v.get(k)
                .and_then(|x| match x {
                    Value::String(s) => Decimal::from_str(s).ok(),
                    Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
                    _ => None,
                })
                .unwrap_or(d)
        };
        let mode = match v.get("mode").and_then(Value::as_str) {
            Some("admin_only") => "admin_only",
            _ => "multi_vendor",
        };
        Self {
            mode: mode.into(),
            commission_percent: dec("commission_percent", Decimal::from(10))
                .clamp(Decimal::ZERO, Decimal::from(100)),
            hold_days: v
                .get("hold_days")
                .and_then(Value::as_i64)
                .unwrap_or(14)
                .clamp(0, 365),
            min_payout: dec("min_payout", Decimal::from(10)).max(Decimal::ZERO),
            auto_approve_creators: v
                .get("auto_approve_creators")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            auto_approve_listings: v
                .get("auto_approve_listings")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            payout_schedule: v
                .get("payout_schedule")
                .and_then(Value::as_str)
                .filter(|x| PAYOUT_SCHEDULES.contains(x))
                .unwrap_or("manual")
                .into(),
            payout_weekday: v
                .get("payout_weekday")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 7) as u8,
            payout_monthday: v
                .get("payout_monthday")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 28) as u8,
            payout_approval: v
                .get("payout_approval")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            payout_methods: match v.get("payout_methods").and_then(Value::as_array) {
                Some(a) => PAYOUT_METHODS
                    .iter()
                    .filter(|m| a.iter().any(|x| x.as_str() == Some(m)))
                    .map(|m| (*m).to_string())
                    .collect(),
                None => PAYOUT_METHODS.iter().map(|m| (*m).to_string()).collect(),
            },
            reverse_transfers_on_refund: v
                .get("reverse_transfers_on_refund")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        }
    }
    pub fn multi_vendor(&self) -> bool {
        self.mode == "multi_vendor"
    }
}

pub async fn config(pool: &PgPool) -> AppResult<Config> {
    Ok(Config::from_value(
        &crate::feature_modules::settings(pool, MODULE).await?,
    ))
}

pub fn slug_ok(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=40).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn url_ok(u: &str) -> bool {
    u.is_empty()
        || (u.len() <= 300
            && (u.starts_with("https://") || u.starts_with("http://"))
            && !u.chars().any(|c| c.is_control() || c.is_whitespace()))
}

// ---------------------------------------------------------------- creators

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Creator {
    pub id: Uuid,
    pub user_id: Uuid,
    pub customer_id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub bio: String,
    pub website: String,
    pub avatar_url: String,
    pub status: String,
    #[schema(value_type = Option<String>)]
    pub commission_percent: Option<Decimal>,
    pub payout_method: String,
    pub payout_details: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Stripe Connect (Express) account, empty until onboarding starts.
    pub stripe_account_id: String,
    /// `none`, `onboarding`, `restricted`, `enabled` or `disconnected`.
    pub stripe_status: String,
    pub stripe_status_detail: String,
    pub paypal_email: String,
    /// Staff fixed the payout method; the creator cannot change it.
    pub payout_method_locked: bool,
}

const CCOLS: &str = "id, user_id, customer_id, slug, display_name, bio, website, avatar_url, status, commission_percent, payout_method, payout_details, created_at,
    stripe_account_id, stripe_status, stripe_status_detail, paypal_email, payout_method_locked";

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[schema(as = CreatorProfile)]
pub struct Profile {
    pub slug: String,
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub payout_method: Option<String>,
    /// e.g. `{"paypal_email": "…"}` or `{"note": "IBAN …"}`; never shown publicly.
    #[serde(default)]
    pub payout_details: Option<Value>,
}

impl Profile {
    /// PayPal e-mail given in `payout_details` (older clients send it there).
    pub fn paypal_email(&self) -> Option<&str> {
        self.payout_details
            .as_ref()
            .and_then(|d| d.get("paypal_email"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|e| !e.is_empty())
    }

    pub fn validate(&self) -> AppResult<()> {
        if !slug_ok(&self.slug)
            || self.display_name.trim().is_empty()
            || self.display_name.len() > 80
            || self.bio.len() > 5000
            || !url_ok(&self.website)
            || !(self.avatar_url.is_empty() || crate::catalog::valid_image_url(&self.avatar_url))
            || self
                .payout_method
                .as_deref()
                .is_some_and(|m| !PAYOUT_METHODS.contains(&m))
            || self
                .payout_details
                .as_ref()
                .is_some_and(|d| !d.is_object() || d.to_string().len() > 2000)
            || self
                .paypal_email()
                .is_some_and(|e| !crate::creator_payouts::email_ok(e))
        {
            return Err(AppError::Validation("error.creator_profile_invalid"));
        }
        Ok(())
    }
}

fn conflict(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            AppError::Conflict("error.creator_slug_taken")
        }
        _ => db_err(e),
    }
}

pub async fn by_user(pool: &PgPool, user: Uuid) -> AppResult<Option<Creator>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CCOLS} FROM creators WHERE user_id = $1"
    )))
    .bind(user)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Creator> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CCOLS} FROM creators WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn by_slug(pool: &PgPool, slug: &str) -> AppResult<Creator> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CCOLS} FROM creators WHERE slug = $1 AND status = 'approved'"
    )))
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn apply(
    pool: &PgPool,
    user: Uuid,
    customer: Uuid,
    p: &Profile,
    approve: bool,
) -> AppResult<Creator> {
    p.validate()?;
    if by_user(pool, user).await?.is_some() {
        return Err(AppError::Conflict("error.creator_exists"));
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO creators (id, user_id, customer_id, slug, display_name, bio, website, avatar_url, status, payout_method, payout_details, approved_at, paypal_email)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,COALESCE($10,'manual'),COALESCE($11,'{{}}'::jsonb), CASE WHEN $12 THEN now() END, COALESCE($13, '')) RETURNING {CCOLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(customer)
    .bind(&p.slug)
    .bind(p.display_name.trim())
    .bind(&p.bio)
    .bind(&p.website)
    .bind(&p.avatar_url)
    .bind(if approve { "approved" } else { "pending" })
    .bind(&p.payout_method)
    .bind(&p.payout_details)
    .bind(approve)
    .bind(p.paypal_email())
    .fetch_one(pool)
    .await
    .map_err(conflict)
}

pub async fn update_profile(pool: &PgPool, id: Uuid, p: &Profile) -> AppResult<Creator> {
    p.validate()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE creators SET slug = $2, display_name = $3, bio = $4, website = $5, avatar_url = $6,
           payout_method = COALESCE($7, payout_method), payout_details = COALESCE($8, payout_details),
           paypal_email = COALESCE($9, paypal_email)
         WHERE id = $1 RETURNING {CCOLS}"
    )))
    .bind(id)
    .bind(&p.slug)
    .bind(p.display_name.trim())
    .bind(&p.bio)
    .bind(&p.website)
    .bind(&p.avatar_url)
    .bind(&p.payout_method)
    .bind(&p.payout_details)
    .bind(p.paypal_email())
    .fetch_one(pool)
    .await
    .map_err(conflict)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CreatorRow {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub creator: Creator,
    pub listings: i64,
    pub sales: i64,
}

pub async fn list(pool: &PgPool, status: Option<&str>, public: bool) -> AppResult<Vec<CreatorRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {CCOLS},
           (SELECT count(*) FROM creator_products cp WHERE cp.creator_id = c.id AND cp.status = 'approved') AS listings,
           (SELECT count(*) FROM creator_ledger l WHERE l.creator_id = c.id AND l.kind = 'sale') AS sales
         FROM creators c WHERE ($1::text IS NULL OR c.status = $1) AND (NOT $2 OR c.status = 'approved')
         ORDER BY c.created_at DESC LIMIT 500"
    )))
    .bind(status)
    .bind(public)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    commission: Option<Option<Decimal>>,
) -> AppResult<Creator> {
    if !["pending", "approved", "suspended", "rejected"].contains(&status)
        || commission
            .flatten()
            .is_some_and(|c| c < Decimal::ZERO || c > Decimal::from(100))
    {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE creators SET status = $2, approved_at = CASE WHEN $2 = 'approved' THEN COALESCE(approved_at, now()) ELSE approved_at END,
           commission_percent = CASE WHEN $3 THEN $4 ELSE commission_percent END
         WHERE id = $1 RETURNING {CCOLS}"
    )))
    .bind(id)
    .bind(status)
    .bind(commission.is_some())
    .bind(commission.flatten())
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

// ---------------------------------------------------------------- listings

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Listing {
    pub product_id: Uuid,
    pub creator_id: Uuid,
    pub creator: String,
    pub creator_slug: String,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub image_url: String,
    pub product_status: String,
    pub status: String,
    pub reject_reason: String,
    #[serde(with = "time::serde::rfc3339")]
    pub submitted_at: OffsetDateTime,
    pub sales: i64,
}

const LCOLS: &str = "cp.product_id, cp.creator_id, c.display_name AS creator, c.slug AS creator_slug, p.name, p.slug, p.description, p.image_url,
    p.status AS product_status, cp.status, cp.reject_reason, cp.submitted_at,
    (SELECT count(*) FROM creator_ledger l WHERE l.product_id = cp.product_id AND l.kind = 'sale') AS sales";
const LFROM: &str = "creator_products cp JOIN creators c ON c.id = cp.creator_id JOIN products p ON p.id = cp.product_id";

pub async fn listings(
    pool: &PgPool,
    creator: Option<Uuid>,
    status: Option<&str>,
) -> AppResult<Vec<Listing>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LCOLS} FROM {LFROM} WHERE ($1::uuid IS NULL OR cp.creator_id = $1) AND ($2::text IS NULL OR cp.status = $2)
         ORDER BY cp.submitted_at DESC LIMIT 500"
    )))
    .bind(creator)
    .bind(status)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn listing(pool: &PgPool, product: Uuid) -> AppResult<Listing> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LCOLS} FROM {LFROM} WHERE cp.product_id = $1"
    )))
    .bind(product)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Links a freshly created (hidden) catalog product to its creator.
pub async fn add_listing(
    pool: &PgPool,
    product: Uuid,
    creator: Uuid,
    approved: bool,
) -> AppResult<()> {
    sqlx::query("INSERT INTO creator_products (product_id, creator_id, status, reviewed_at) VALUES ($1, $2, $3, CASE WHEN $4 THEN now() END)")
        .bind(product)
        .bind(creator)
        .bind(if approved { "approved" } else { "pending" })
        .bind(approved)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Admin decision. Approved listings become visible (`products.status = active`).
pub async fn review(
    pool: &PgPool,
    product: Uuid,
    approve: bool,
    reason: &str,
    by: Uuid,
) -> AppResult<()> {
    if reason.len() > 1000 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("UPDATE creator_products SET status = $2, reject_reason = $3, reviewed_at = now(), reviewed_by = $4 WHERE product_id = $1")
        .bind(product)
        .bind(if approve { "approved" } else { "rejected" })
        .bind(if approve { "" } else { reason })
        .bind(by)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("UPDATE products SET status = $2 WHERE id = $1")
        .bind(product)
        .bind(if approve { "active" } else { "hidden" })
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

/// Creator hides/re-lists an approved listing.
pub async fn set_listed(
    pool: &PgPool,
    product: Uuid,
    creator: Uuid,
    listed: bool,
) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query(
        "UPDATE creator_products SET status = $3 WHERE product_id = $1 AND creator_id = $2 AND status IN ('approved', 'unlisted')",
    )
    .bind(product)
    .bind(creator)
    .bind(if listed { "approved" } else { "unlisted" })
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.listing_state"));
    }
    sqlx::query("UPDATE products SET status = $2 WHERE id = $1")
        .bind(product)
        .bind(if listed { "active" } else { "hidden" })
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

pub async fn owner_of(pool: &PgPool, product: Uuid) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT creator_id FROM creator_products WHERE product_id = $1")
        .bind(product)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

// ---------------------------------------------------------------- ledger

/// Accrues every creator line of a paid invoice (idempotent per line).
/// Lines are invoice items linked to a service, or, for order invoices, the
/// order items of products not already counted that way. Gross is net of tax.
pub async fn accrue_invoice(pool: &PgPool, invoice: Uuid, cfg: &Config) -> AppResult<u64> {
    let r = sqlx::query(
        "WITH lines AS (
            SELECT ii.id AS line, s.product_id, ii.amount AS gross
              FROM invoice_items ii JOIN services s ON s.id = ii.service_id WHERE ii.invoice_id = $1
            UNION ALL
            SELECT oi.id, oi.product_id, oi.amount + oi.setup_fee - oi.discount
              FROM invoices i JOIN order_items oi ON oi.order_id = i.order_id
             WHERE i.id = $1 AND NOT EXISTS (
               SELECT 1 FROM invoice_items ii JOIN services s ON s.id = ii.service_id
                WHERE ii.invoice_id = $1 AND s.product_id = oi.product_id)
         )
         INSERT INTO creator_ledger (id, creator_id, kind, source_id, invoice_id, product_id, currency, gross, commission, amount, available_at)
         SELECT gen_random_uuid(), c.id, 'sale', l.line, i.id, l.product_id, i.currency, l.gross,
                round(l.gross * COALESCE(c.commission_percent, $2) / 100, cur.exponent),
                l.gross - round(l.gross * COALESCE(c.commission_percent, $2) / 100, cur.exponent),
                now() + make_interval(days => $3::int)
           FROM lines l
           JOIN creator_products cp ON cp.product_id = l.product_id
           JOIN creators c ON c.id = cp.creator_id
           JOIN invoices i ON i.id = $1 AND i.kind = 'invoice'
           JOIN currencies cur ON cur.code = i.currency
          WHERE l.gross > 0
         ON CONFLICT (kind, source_id) DO NOTHING",
    )
    .bind(invoice)
    .bind(cfg.commission_percent)
    .bind(cfg.hold_days as i32)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Reverses the sales of the given invoices (refund / chargeback).
pub async fn reverse_invoices(pool: &PgPool, invoices: &[Uuid], note: &str) -> AppResult<u64> {
    let r = sqlx::query(
        "INSERT INTO creator_ledger (id, creator_id, kind, source_id, invoice_id, product_id, currency, gross, commission, amount, note)
         SELECT gen_random_uuid(), l.creator_id, 'refund', l.id, l.invoice_id, l.product_id, l.currency, -l.gross, -l.commission, -l.amount, $2
           FROM creator_ledger l WHERE l.kind = 'sale' AND l.invoice_id = ANY($1)
         ON CONFLICT (kind, source_id) DO NOTHING",
    )
    .bind(invoices)
    .bind(note)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

pub async fn invoices_for(pool: &PgPool, kind: &str, id: Uuid) -> AppResult<Vec<Uuid>> {
    let sql = match kind {
        "order" => "SELECT id FROM invoices WHERE order_id = $1 AND kind = 'invoice'",
        "invoice" => "SELECT id FROM invoices WHERE id = $1",
        "payment" => "SELECT invoice_id FROM payments WHERE id = $1 AND invoice_id IS NOT NULL",
        _ => return Ok(vec![]),
    };
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = CreatorBalance)]
pub struct Balance {
    pub currency: String,
    /// Can be paid out now.
    #[schema(value_type = String)]
    pub available: Decimal,
    /// Still inside the refund hold.
    #[schema(value_type = String)]
    pub pending: Decimal,
    /// Lifetime sales, before commission.
    #[schema(value_type = String)]
    pub gross: Decimal,
    #[schema(value_type = String)]
    pub commission: Decimal,
}

pub async fn balances(pool: &PgPool, creator: Uuid) -> AppResult<Vec<Balance>> {
    sqlx::query_as(
        "SELECT currency::text AS currency,
                COALESCE(sum(amount) FILTER (WHERE available_at <= now()), 0) AS available,
                COALESCE(sum(amount) FILTER (WHERE available_at > now()), 0) AS pending,
                COALESCE(sum(gross) FILTER (WHERE kind IN ('sale', 'refund')), 0) AS gross,
                COALESCE(sum(commission) FILTER (WHERE kind IN ('sale', 'refund')), 0) AS commission
           FROM creator_ledger WHERE creator_id = $1 GROUP BY currency ORDER BY currency",
    )
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = CreatorLedgerEntry)]
pub struct Entry {
    pub id: Uuid,
    pub kind: String,
    pub product_id: Option<Uuid>,
    pub product: Option<String>,
    pub currency: String,
    #[schema(value_type = String)]
    pub gross: Decimal,
    #[schema(value_type = String)]
    pub commission: Decimal,
    #[schema(value_type = String)]
    pub amount: Decimal,
    #[serde(with = "time::serde::rfc3339")]
    pub available_at: OffsetDateTime,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn ledger(pool: &PgPool, creator: Uuid, limit: i64) -> AppResult<Vec<Entry>> {
    sqlx::query_as(
        "SELECT l.id, l.kind, l.product_id, p.name AS product, l.currency::text AS currency, l.gross, l.commission, l.amount, l.available_at, l.note, l.created_at
           FROM creator_ledger l LEFT JOIN products p ON p.id = l.product_id WHERE l.creator_id = $1 ORDER BY l.created_at DESC LIMIT $2",
    )
    .bind(creator)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Per-product sales for the seller dashboard.
pub async fn product_stats(pool: &PgPool, creator: Uuid) -> AppResult<Vec<Value>> {
    let rows: Vec<(Uuid, String, String, i64, Decimal)> = sqlx::query_as(
        "SELECT l.product_id, p.name, l.currency::text, count(*) FILTER (WHERE l.kind = 'sale'), COALESCE(sum(l.amount), 0)
           FROM creator_ledger l JOIN products p ON p.id = l.product_id
          WHERE l.creator_id = $1 AND l.kind IN ('sale', 'refund') GROUP BY l.product_id, p.name, l.currency ORDER BY 5 DESC",
    )
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, name, cur, n, net)| serde_json::json!({ "product_id": id, "name": name, "currency": cur, "sales": n, "net": net.to_string() }))
        .collect())
}

// ---------------------------------------------------------------- payouts

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Payout {
    pub id: Uuid,
    pub creator_id: Uuid,
    pub creator: String,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub method: String,
    pub status: String,
    pub provider_ref: String,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub processed_at: Option<OffsetDateTime>,
    /// PayPal batch (`pending:<sender_batch_id>` until PayPal accepted it).
    pub batch_id: String,
    /// Amount pulled back from a Stripe transfer after refunds.
    #[schema(value_type = String)]
    pub reversed: Decimal,
    /// Created by the payout schedule.
    pub auto: bool,
}

const PCOLS: &str = "po.id, po.creator_id, c.display_name AS creator, po.currency::text AS currency, po.amount, po.method, po.status, po.provider_ref, po.note, po.created_at, po.processed_at,
    po.batch_id, po.reversed, po.auto";

pub async fn payouts(
    pool: &PgPool,
    creator: Option<Uuid>,
    status: Option<&str>,
) -> AppResult<Vec<Payout>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {PCOLS} FROM creator_payouts po JOIN creators c ON c.id = po.creator_id
          WHERE ($1::uuid IS NULL OR po.creator_id = $1) AND ($2::text IS NULL OR po.status = $2) ORDER BY po.created_at DESC LIMIT 500"
    )))
    .bind(creator)
    .bind(status)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn payout(pool: &PgPool, id: Uuid) -> AppResult<Payout> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {PCOLS} FROM creator_payouts po JOIN creators c ON c.id = po.creator_id WHERE po.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Requests a payout of the available balance; the amount is debited from the
/// ledger at once (reversed if the payout is rejected or fails).
pub async fn request_payout(
    pool: &PgPool,
    c: &Creator,
    currency: &str,
    amount: Decimal,
    cfg: &Config,
    auto: bool,
) -> AppResult<Payout> {
    if amount <= Decimal::ZERO || amount.scale() > 4 {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    if amount < cfg.min_payout {
        return Err(AppError::Validation("error.payout_below_minimum"));
    }
    let currency = currency.to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('creator_payout:' || $1::text))")
        .bind(c.id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let available: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(sum(amount), 0) FROM creator_ledger WHERE creator_id = $1 AND currency = $2 AND available_at <= now()",
    )
    .bind(c.id)
    .bind(&currency)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if amount > available {
        return Err(AppError::Conflict("error.payout_insufficient"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO creator_payouts (id, creator_id, currency, amount, method, auto) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(c.id)
        .bind(&currency)
        .bind(amount)
        .bind(&c.payout_method)
        .bind(auto)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO creator_ledger (id, creator_id, kind, source_id, currency, amount) VALUES ($1,$2,'payout',$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(c.id)
        .bind(id)
        .bind(&currency)
        .bind(-amount)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    payout(pool, id).await
}

/// Moves a payout to `processing`, `paid`, `rejected` or `failed`. Rejected or
/// failed payouts credit the amount back.
/// After a provider error on a claimed payout, put it back to `requested`
/// (only while nothing was recorded from a provider).
pub async fn unclaim_payout(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE creator_payouts SET status = 'requested' WHERE id = $1 AND status = 'processing' AND provider_ref = ''")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Store the provider reference on a payout that is processing.
pub async fn payout_ref(pool: &PgPool, id: Uuid, provider_ref: &str) -> AppResult<Payout> {
    if provider_ref.len() > 200 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    sqlx::query(
        "UPDATE creator_payouts SET provider_ref = $2 WHERE id = $1 AND status = 'processing'",
    )
    .bind(id)
    .bind(provider_ref)
    .execute(pool)
    .await
    .map_err(db_err)?;
    payout(pool, id).await
}

/// Publish a creator's product (after its listing row exists).
pub async fn activate_product(pool: &PgPool, product: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE products SET status = 'active' WHERE id = $1")
        .bind(product)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_payout(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    provider_ref: &str,
    note: &str,
) -> AppResult<Payout> {
    let allowed_from: &[&str] = match status {
        "processing" => &["requested"],
        "paid" => &["requested", "processing"],
        "rejected" | "failed" => &["requested", "processing"],
        _ => return Err(AppError::Validation("error.validation")),
    };
    if provider_ref.len() > 200 || note.len() > 1000 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(Uuid, String, Decimal, String)> =
        sqlx::query_as("SELECT creator_id, currency::text, amount, status FROM creator_payouts WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let (creator, currency, amount, cur) = row.ok_or(AppError::NotFound)?;
    if !allowed_from.contains(&cur.as_str()) {
        return Err(AppError::Conflict("error.payout_state"));
    }
    sqlx::query(
        "UPDATE creator_payouts SET status = $2, provider_ref = CASE WHEN $3 = '' THEN provider_ref ELSE $3 END,
           note = CASE WHEN $4 = '' THEN note ELSE $4 END, processed_at = CASE WHEN $2 = 'processing' THEN processed_at ELSE now() END WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(provider_ref)
    .bind(note)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if matches!(status, "rejected" | "failed") {
        sqlx::query("INSERT INTO creator_ledger (id, creator_id, kind, source_id, currency, amount, note) VALUES ($1,$2,'payout_reversal',$3,$4,$5,$6) ON CONFLICT (kind, source_id) DO NOTHING")
            .bind(Uuid::new_v4())
            .bind(creator)
            .bind(id)
            .bind(&currency)
            .bind(amount)
            .bind(status)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    payout(pool, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_defaults_and_clamps() {
        let c = Config::from_value(&json!({}));
        assert!(c.multi_vendor());
        assert_eq!(c.commission_percent, Decimal::from(10));
        assert_eq!(c.hold_days, 14);
        let c = Config::from_value(
            &json!({"mode": "admin_only", "commission_percent": "150", "hold_days": -3, "min_payout": 5}),
        );
        assert!(!c.multi_vendor());
        assert_eq!(c.commission_percent, Decimal::from(100));
        assert_eq!(c.hold_days, 0);
        assert_eq!(c.min_payout, Decimal::from(5));
    }

    #[test]
    fn slugs_and_profiles() {
        assert!(slug_ok("cool-dev"));
        assert!(!slug_ok("Cool"));
        assert!(!slug_ok("a"));
        let p = Profile {
            slug: "dev".into(),
            display_name: "Dev".into(),
            bio: String::new(),
            website: "javascript:x".into(),
            avatar_url: String::new(),
            payout_method: None,
            payout_details: None,
        };
        assert!(p.validate().is_err());
        let p = Profile {
            website: "https://dev.example".into(),
            payout_method: Some("paypal".into()),
            ..p
        };
        assert!(p.validate().is_ok());
    }
}

/// One approved listing on the public marketplace (BuiltByBit-style browse).
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = MarketplaceListing)]
pub struct PublicListing {
    pub product_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub image_url: String,
    pub creator: String,
    pub creator_slug: String,
    pub creator_avatar: String,
    pub category: String,
    pub category_slug: String,
    /// All-time paid sales.
    pub sales: i64,
    /// Paid sales in the last 30 days (the "trending" order).
    pub recent_sales: i64,
    /// Average approved review rating (1–5), `None` without reviews or when
    /// `product_reviews` is off.
    pub rating: Option<f64>,
    pub reviews: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub listed_at: OffsetDateTime,
}

/// Browse order of [`public_listings`].
pub const SORTS: &[&str] = &["trending", "new", "top_rated", "popular"];

const PUBLIC_LISTING_SQL: &str = "SELECT cp.product_id, p.name, p.slug, p.description, p.image_url,
        c.display_name AS creator, c.slug AS creator_slug, c.avatar_url AS creator_avatar,
        g.name AS category, g.slug AS category_slug,
        (SELECT count(*) FROM creator_ledger l WHERE l.product_id = cp.product_id AND l.kind = 'sale') AS sales,
        (SELECT count(*) FROM creator_ledger l WHERE l.product_id = cp.product_id AND l.kind = 'sale' AND l.created_at > now() - interval '30 days') AS recent_sales,
        CASE WHEN $1 THEN (SELECT avg(r.rating)::float8 FROM product_reviews r WHERE r.product_id = cp.product_id AND r.status = 'approved') END AS rating,
        CASE WHEN $1 THEN (SELECT count(*) FROM product_reviews r WHERE r.product_id = cp.product_id AND r.status = 'approved') ELSE 0 END AS reviews,
        COALESCE(cp.reviewed_at, cp.submitted_at) AS listed_at
   FROM creator_products cp JOIN creators c ON c.id = cp.creator_id JOIN products p ON p.id = cp.product_id
   JOIN product_groups g ON g.id = p.group_id
  WHERE cp.status = 'approved' AND c.status = 'approved' AND p.status = 'active' AND NOT g.hidden";

/// Approved, active listings matching `q` (name/description/creator) in a
/// category slug, sorted by one of [`SORTS`] (unknown = trending).
pub async fn public_listings(
    pool: &PgPool,
    ratings: bool,
    q: &str,
    category: Option<&str>,
    sort: &str,
    limit: i64,
) -> AppResult<Vec<PublicListing>> {
    let order = match sort {
        "new" => "listed_at DESC",
        "top_rated" => "rating DESC NULLS LAST, reviews DESC, sales DESC",
        "popular" => "sales DESC, listed_at DESC",
        _ => "recent_sales DESC, sales DESC, listed_at DESC",
    };
    let pattern = format!(
        "%{}%",
        q.trim()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT * FROM ({PUBLIC_LISTING_SQL}
            AND ($2 = '%%' OR p.name ILIKE $2 OR p.description ILIKE $2 OR c.display_name ILIKE $2)
            AND ($3::text IS NULL OR g.slug = $3)) x
          ORDER BY {order}, name LIMIT $4"
    )))
    .bind(ratings)
    .bind(pattern)
    .bind(category)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// One approved listing, or `NotFound`.
pub async fn public_listing(
    pool: &PgPool,
    ratings: bool,
    product: Uuid,
) -> AppResult<PublicListing> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{PUBLIC_LISTING_SQL} AND cp.product_id = $2"
    )))
    .bind(ratings)
    .bind(product)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = MarketplaceCategory)]
pub struct Category {
    pub name: String,
    pub slug: String,
    pub listings: i64,
}

/// Visible product groups that hold at least one approved listing.
pub async fn public_categories(pool: &PgPool) -> AppResult<Vec<Category>> {
    sqlx::query_as(
        "SELECT g.name, g.slug, count(*) AS listings
           FROM creator_products cp JOIN creators c ON c.id = cp.creator_id JOIN products p ON p.id = cp.product_id
           JOIN product_groups g ON g.id = p.group_id
          WHERE cp.status = 'approved' AND c.status = 'approved' AND p.status = 'active' AND NOT g.hidden
          GROUP BY g.id ORDER BY g.sort_order, g.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}
