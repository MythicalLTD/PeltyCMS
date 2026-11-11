//! Live creator payouts (module `creator_marketplace`): payout settings per
//! creator (method, PayPal email, Stripe Connect account and status),
//! provider batches and transfer reversals, webhook dedupe, scheduled payout
//! candidates, resource galleries and buyer lists for update mails.
//!
//! Money model: buyers pay the platform through the normal gateways; each
//! sale is accrued to `creator_ledger` minus commission and held for
//! `hold_days`. A payout debits the ledger when it is requested and is then
//! sent as a Stripe transfer (platform balance → connected account) or a
//! PayPal Payouts item. Failed or returned payouts and transfer reversals
//! credit the ledger again.
use crate::db_err;
use crate::marketplace::{payout, Creator, Payout, PAYOUT_METHODS};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

pub const STRIPE_STATUSES: &[&str] = &[
    "none",
    "onboarding",
    "restricted",
    "enabled",
    "disconnected",
];

/// Loose e-mail check for PayPal receivers (PayPal validates for real).
pub fn email_ok(e: &str) -> bool {
    let e = e.trim();
    let Some((local, domain)) = e.split_once('@') else {
        return false;
    };
    (3..=254).contains(&e.len())
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !e
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == ',' || c == ';')
        && !domain.contains('@')
}

/// Stripe connected account ids look like `acct_…`.
pub fn stripe_account_ok(a: &str) -> bool {
    a.len() <= 64
        && a.strip_prefix("acct_")
            .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn ccols() -> &'static str {
    "id, user_id, customer_id, slug, display_name, bio, website, avatar_url, banner_url, status, commission_percent, payout_method, payout_details, created_at,
     stripe_account_id, stripe_status, stripe_status_detail, paypal_email, payout_method_locked"
}

/// Creator-chosen method and PayPal email. `allowed` is the marketplace list.
pub async fn save_settings(
    pool: &PgPool,
    c: &Creator,
    method: &str,
    paypal_email: &str,
    allowed: &[String],
) -> AppResult<Creator> {
    let email = paypal_email.trim();
    if !PAYOUT_METHODS.contains(&method) || (!email.is_empty() && !email_ok(email)) {
        return Err(AppError::Validation("error.payout_settings_invalid"));
    }
    if method != c.payout_method {
        if c.payout_method_locked {
            return Err(AppError::Conflict("error.payout_method_locked"));
        }
        if !allowed.iter().any(|m| m == method) {
            return Err(AppError::Validation("error.payout_method_unavailable"));
        }
    }
    if method == "paypal" && email.is_empty() {
        return Err(AppError::Validation("error.paypal_email_required"));
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE creators SET payout_method = $2, paypal_email = $3 WHERE id = $1 RETURNING {}",
        ccols()
    )))
    .bind(c.id)
    .bind(method)
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Staff assign a creator's payout method (and optionally lock it).
pub async fn admin_set_method(
    pool: &PgPool,
    id: Uuid,
    method: &str,
    locked: bool,
) -> AppResult<Creator> {
    if !PAYOUT_METHODS.contains(&method) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE creators SET payout_method = $2, payout_method_locked = $3 WHERE id = $1 RETURNING {}",
        ccols()
    )))
    .bind(id)
    .bind(method)
    .bind(locked)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Remember the connected account created for a creator.
pub async fn set_stripe_account(pool: &PgPool, id: Uuid, account: &str) -> AppResult<()> {
    if !stripe_account_ok(account) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query(
        "UPDATE creators SET stripe_account_id = $2, stripe_status = 'onboarding', stripe_status_detail = '', stripe_updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(account)
    .execute(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.stripe_account_taken"),
        _ => db_err(e),
    })?;
    Ok(())
}

/// Status from Stripe (`account.updated` or a sync). Returns the creator id.
pub async fn set_stripe_status(
    pool: &PgPool,
    account: &str,
    status: &str,
    detail: &str,
) -> AppResult<Option<Uuid>> {
    if !STRIPE_STATUSES.contains(&status) {
        return Err(AppError::Validation("error.validation"));
    }
    let detail: String = detail.chars().take(500).collect();
    sqlx::query_scalar(
        "UPDATE creators SET stripe_status = $2, stripe_status_detail = $3, stripe_updated_at = now()
          WHERE stripe_account_id = $1 AND stripe_account_id <> '' RETURNING id",
    )
    .bind(account)
    .bind(status)
    .bind(detail)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Note shown to the creator (e.g. a failed bank payout on their Stripe account).
pub async fn set_stripe_detail(
    pool: &PgPool,
    account: &str,
    detail: &str,
) -> AppResult<Option<Uuid>> {
    let detail: String = detail.chars().take(500).collect();
    sqlx::query_scalar(
        "UPDATE creators SET stripe_status_detail = $2, stripe_updated_at = now() WHERE stripe_account_id = $1 AND stripe_account_id <> '' RETURNING id",
    )
    .bind(account)
    .bind(detail)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// First time an event is seen: `true` (apply it); repeats: `false`.
pub async fn first_event(pool: &PgPool, provider: &str, id: &str, kind: &str) -> AppResult<bool> {
    if id.is_empty() || id.len() > 200 {
        return Err(AppError::Validation("error.validation"));
    }
    let r = sqlx::query(
        "INSERT INTO creator_payout_events (provider, event_id, kind) VALUES ($1, $2, left($3, 100)) ON CONFLICT DO NOTHING",
    )
    .bind(provider)
    .bind(id)
    .bind(kind)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Forget an event so a provider retry applies it again (after a failure).
pub async fn forget_event(pool: &PgPool, provider: &str, id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM creator_payout_events WHERE provider = $1 AND event_id = $2")
        .bind(provider)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Payouts of a method in a status, oldest first.
pub async fn open_payouts(
    pool: &PgPool,
    method: &str,
    status: &str,
    limit: i64,
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM creator_payouts WHERE method = $1 AND status = $2 ORDER BY created_at LIMIT $3",
    )
    .bind(method)
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Claim requested payouts (requested → processing) under row locks; returns
/// the ones this caller now owns.
pub async fn claim(pool: &PgPool, ids: &[Uuid]) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "UPDATE creator_payouts SET status = 'processing' WHERE id = ANY($1) AND status = 'requested' RETURNING id",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Give claimed payouts back (requested) after a definite provider refusal
/// of a request that created nothing.
pub async fn unclaim(pool: &PgPool, ids: &[Uuid]) -> AppResult<()> {
    sqlx::query("UPDATE creator_payouts SET status = 'requested', batch_id = '' WHERE id = ANY($1) AND status = 'processing' AND provider_ref = ''")
        .bind(ids)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Note on a payout (last provider error, shown to staff).
pub async fn note(pool: &PgPool, id: Uuid, note: &str) -> AppResult<()> {
    sqlx::query("UPDATE creator_payouts SET note = left($2, 1000) WHERE id = $1")
        .bind(id)
        .bind(note)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Login e-mail, display name and locale of a creator's user.
pub async fn contact(
    pool: &PgPool,
    creator: Uuid,
) -> AppResult<Option<(String, String, Option<String>)>> {
    sqlx::query_as(
        "SELECT u.email, c.display_name, u.locale FROM creators c JOIN users u ON u.id = c.user_id WHERE c.id = $1 AND u.is_active",
    )
    .bind(creator)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Batch reference on processing payouts.
pub async fn set_batch(pool: &PgPool, ids: &[Uuid], batch: &str) -> AppResult<()> {
    if batch.len() > 120 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    sqlx::query(
        "UPDATE creator_payouts SET batch_id = $2 WHERE id = ANY($1) AND status = 'processing'",
    )
    .bind(ids)
    .bind(batch)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Distinct batch ids of processing PayPal payouts.
pub async fn open_batches(pool: &PgPool) -> AppResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT batch_id FROM creator_payouts WHERE method = 'paypal' AND status = 'processing' AND batch_id <> '' LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn by_batch(pool: &PgPool, batch: &str) -> AppResult<Vec<Payout>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM creator_payouts WHERE batch_id = $1 AND status = 'processing' ORDER BY id",
    )
    .bind(batch)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(payout(pool, id).await?);
    }
    Ok(out)
}

/// A provider reports the money did not arrive or came back (failed,
/// returned, reversed…): the payout becomes `failed` and the amount is
/// credited back, also after it was `paid`. Idempotent.
pub async fn provider_failed(pool: &PgPool, id: Uuid, note: &str) -> AppResult<Option<Payout>> {
    let note: String = note.chars().take(1000).collect();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(Uuid, String, Decimal, Decimal)> = sqlx::query_as(
        "UPDATE creator_payouts SET status = 'failed', note = $2, processed_at = now()
          WHERE id = $1 AND status IN ('requested', 'processing', 'paid')
          RETURNING creator_id, currency::text, amount, reversed",
    )
    .bind(id)
    .bind(&note)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some((creator, currency, amount, reversed)) = row else {
        return Ok(None);
    };
    // Only what actually left the platform comes back (minus reversals already credited).
    sqlx::query(
        "INSERT INTO creator_ledger (id, creator_id, kind, source_id, currency, amount, note) VALUES ($1,$2,'payout_reversal',$3,$4,$5,left($6, 500))
         ON CONFLICT (kind, source_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(creator)
    .bind(id)
    .bind(&currency)
    .bind(amount - reversed)
    .bind(&note)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(payout(pool, id).await?))
}

/// Provider confirmed delivery: processing → paid with its reference.
pub async fn provider_paid(
    pool: &PgPool,
    id: Uuid,
    provider_ref: &str,
) -> AppResult<Option<Payout>> {
    let r = sqlx::query(
        "UPDATE creator_payouts SET status = 'paid', provider_ref = left($2, 200), processed_at = now() WHERE id = $1 AND status IN ('requested', 'processing')",
    )
    .bind(id)
    .bind(provider_ref)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(payout(pool, id).await?))
}

/// Book a Stripe transfer reversal (+amount to the creator ledger). `false`
/// when this reversal id was already booked.
pub async fn record_reversal(
    pool: &PgPool,
    payout_id: Uuid,
    reversal_id: &str,
    amount: Decimal,
    note: &str,
) -> AppResult<bool> {
    if reversal_id.is_empty() || reversal_id.len() > 100 || amount <= Decimal::ZERO {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query(
        "INSERT INTO creator_transfer_reversals (reversal_id, payout_id, amount) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
    )
    .bind(reversal_id)
    .bind(payout_id)
    .bind(amount)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Ok(false);
    }
    let (creator, currency): (Uuid, String) = sqlx::query_as(
        "UPDATE creator_payouts SET reversed = reversed + $2 WHERE id = $1 RETURNING creator_id, currency::text",
    )
    .bind(payout_id)
    .bind(amount)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query(
        "INSERT INTO creator_ledger (id, creator_id, kind, source_id, currency, amount, note) VALUES ($1,$2,'transfer_reversal',$3,$4,$5,left($6, 500))",
    )
    .bind(Uuid::new_v4())
    .bind(creator)
    .bind(Uuid::new_v4())
    .bind(&currency)
    .bind(amount)
    .bind(note)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(true)
}

/// Total of reversals already booked for a payout.
pub async fn reversal_ids(pool: &PgPool, payout_id: Uuid) -> AppResult<Vec<String>> {
    sqlx::query_scalar("SELECT reversal_id FROM creator_transfer_reversals WHERE payout_id = $1")
        .bind(payout_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Stripe-paid payout by transfer id.
pub async fn by_transfer(pool: &PgPool, transfer: &str) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM creator_payouts WHERE method = 'stripe_connect' AND provider_ref = $1 AND provider_ref <> '' LIMIT 1",
    )
    .bind(transfer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Whole-ledger balance (available and pending) per currency for creators
/// touched by these invoices, when it is below zero.
pub async fn negative_balances(
    pool: &PgPool,
    invoices: &[Uuid],
) -> AppResult<Vec<(Uuid, String, Decimal)>> {
    sqlx::query_as(
        "SELECT l.creator_id, l.currency::text, sum(l.amount)
           FROM creator_ledger l
          WHERE l.creator_id IN (SELECT creator_id FROM creator_ledger WHERE invoice_id = ANY($1))
          GROUP BY l.creator_id, l.currency HAVING sum(l.amount) < 0",
    )
    .bind(invoices)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Paid Stripe payouts with money left to reverse, newest first:
/// `(payout, transfer id, amount - reversed)`.
pub async fn reversible(
    pool: &PgPool,
    creator: Uuid,
    currency: &str,
) -> AppResult<Vec<(Uuid, String, Decimal)>> {
    sqlx::query_as(
        "SELECT id, provider_ref, amount - reversed FROM creator_payouts
          WHERE creator_id = $1 AND currency = $2 AND method = 'stripe_connect' AND status = 'paid'
            AND provider_ref LIKE 'tr\\_%' AND amount > reversed
          ORDER BY processed_at DESC NULLS LAST LIMIT 20",
    )
    .bind(creator)
    .bind(currency)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Minor-unit exponent of a currency (2 when unknown).
pub async fn exponent(pool: &PgPool, currency: &str) -> AppResult<u32> {
    let e: Option<i16> = sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1")
        .bind(currency)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(e.map_or(2, |x| x.clamp(0, 4) as u32))
}

/// Creators and currencies whose available balance reaches the minimum and
/// that have no payout still open in that currency (scheduled payouts).
pub async fn due_creators(pool: &PgPool, min: Decimal) -> AppResult<Vec<(Uuid, String, Decimal)>> {
    sqlx::query_as(
        "SELECT l.creator_id, l.currency::text, sum(l.amount) AS available
           FROM creator_ledger l JOIN creators c ON c.id = l.creator_id
          WHERE c.status = 'approved' AND l.available_at <= now()
            AND NOT EXISTS (SELECT 1 FROM creator_payouts po WHERE po.creator_id = l.creator_id AND po.currency = l.currency AND po.status IN ('requested', 'processing'))
          GROUP BY l.creator_id, l.currency
         HAVING sum(l.amount) > 0 AND sum(l.amount) >= $1
          ORDER BY l.creator_id LIMIT 1000",
    )
    .bind(min)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Money in payouts not settled yet, per currency (dashboard "in transit").
pub async fn in_transit(pool: &PgPool, creator: Uuid) -> AppResult<Vec<(String, Decimal)>> {
    sqlx::query_as(
        "SELECT currency::text, sum(amount) FROM creator_payouts WHERE creator_id = $1 AND status IN ('requested', 'processing') GROUP BY currency ORDER BY currency",
    )
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ---------------------------------------------------------------- gallery

pub const GALLERY_MAX: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct GalleryImage {
    pub url: String,
    #[serde(default)]
    pub caption: String,
}

pub async fn gallery(pool: &PgPool, product: Uuid) -> AppResult<Vec<GalleryImage>> {
    sqlx::query_as(
        "SELECT url, caption FROM product_gallery WHERE product_id = $1 ORDER BY sort_order, id",
    )
    .bind(product)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Replace a product's gallery (order kept, at most [`GALLERY_MAX`]).
pub async fn set_gallery(
    pool: &PgPool,
    product: Uuid,
    images: &[GalleryImage],
) -> AppResult<Vec<GalleryImage>> {
    if images.len() > GALLERY_MAX
        || images
            .iter()
            .any(|i| !crate::catalog::valid_image_url(&i.url) || i.caption.chars().count() > 200)
    {
        return Err(AppError::Validation("error.gallery_invalid"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM product_gallery WHERE product_id = $1")
        .bind(product)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for (i, img) in images.iter().enumerate() {
        sqlx::query("INSERT INTO product_gallery (id, product_id, url, caption, sort_order) VALUES ($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4())
            .bind(product)
            .bind(&img.url)
            .bind(img.caption.trim())
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    gallery(pool, product).await
}

// ---------------------------------------------------------------- update mails

/// Someone to tell about a new version.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Buyer {
    pub email: String,
    pub name: String,
    pub locale: Option<String>,
}

/// Customers entitled to download a product (active service or license),
/// skipping closed accounts and marketing opt-outs.
pub async fn buyers(pool: &PgPool, product: Uuid, limit: i64) -> AppResult<Vec<Buyer>> {
    sqlx::query_as(
        "SELECT DISTINCT ON (lower(COALESCE(NULLIF(c.email, ''), u.email)))
                COALESCE(NULLIF(c.email, ''), u.email) AS email,
                COALESCE(NULLIF(trim(c.first_name), ''), u.display_name, '') AS name, u.locale
           FROM customers c LEFT JOIN users u ON u.id = c.owner_user_id
          WHERE c.closed_at IS NULL AND NOT c.marketing_opt_out
            AND COALESCE(NULLIF(c.email, ''), u.email) IS NOT NULL
            AND (EXISTS (SELECT 1 FROM services s WHERE s.customer_id = c.id AND s.product_id = $1 AND s.status = 'active')
              OR EXISTS (SELECT 1 FROM software_licenses l WHERE l.customer_id = c.id AND l.product_id = $1 AND l.status = 'active'
                          AND (l.expires_at IS NULL OR l.expires_at > now())))
          LIMIT $2",
    )
    .bind(product)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_and_accounts() {
        assert!(email_ok("dev@example.com"));
        assert!(!email_ok("dev@example"));
        assert!(!email_ok("a b@example.com"));
        assert!(!email_ok("a@b.com,c@d.com"));
        assert!(stripe_account_ok("acct_1Nv0FGQ9RKHgCVdK"));
        assert!(!stripe_account_ok("acct_"));
        assert!(!stripe_account_ok("acct_../x"));
        assert!(!stripe_account_ok("cus_123"));
    }
}
