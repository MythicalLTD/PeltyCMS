//! Affiliates: referral codes, signup attribution, commissions (accrued by a
//! DB trigger when a referred customer's invoice is paid) and payouts.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Affiliate {
    pub id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub code: String,
    #[schema(value_type = String)]
    pub commission_percent: Decimal,
    pub status: String,
    pub referrals: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "a.id, a.user_id, u.email::text AS email, a.code, a.commission_percent, a.status,
    (SELECT count(*) FROM affiliate_referrals r WHERE r.affiliate_id = a.id) AS referrals, a.created_at";

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Commission {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub invoice_number: Option<String>,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Payout {
    pub id: Uuid,
    pub affiliate_id: Uuid,
    pub email: String,
    pub currency: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    pub status: String,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Balance {
    pub currency: String,
    #[schema(value_type = String)]
    pub earned: Decimal,
    #[schema(value_type = String)]
    pub withdrawn: Decimal,
    #[schema(value_type = String)]
    pub available: Decimal,
}

pub async fn for_user(pool: &PgPool, user_id: Uuid) -> AppResult<Option<Affiliate>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM affiliates a JOIN users u ON u.id = a.user_id WHERE a.user_id = $1"
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Join the program (idempotent).
pub async fn enroll(pool: &PgPool, user_id: Uuid) -> AppResult<Affiliate> {
    let code = Uuid::new_v4().simple().to_string()[..10].to_string();
    sqlx::query("INSERT INTO affiliates (id, user_id, code) VALUES ($1,$2,$3) ON CONFLICT (user_id) DO NOTHING")
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(code)
        .execute(pool)
        .await
        .map_err(db_err)?;
    for_user(pool, user_id).await?.ok_or(AppError::NotFound)
}

/// Is `code` a valid, active referral code?
pub async fn code_exists(pool: &PgPool, code: &str) -> AppResult<bool> {
    if !valid_code(code) {
        return Ok(false);
    }
    let n: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM affiliates WHERE code = $1 AND status = 'active'")
            .bind(code)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(n.is_some())
}

pub fn valid_code(code: &str) -> bool {
    (6..=32).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Attribute a new user to the affiliate owning `code`. First touch wins;
/// self-referrals and unknown codes are ignored.
pub async fn attribute(pool: &PgPool, user_id: Uuid, code: &str) -> AppResult<bool> {
    if !valid_code(code) {
        return Ok(false);
    }
    let r = sqlx::query(
        "INSERT INTO affiliate_referrals (user_id, affiliate_id)
         SELECT $1, id FROM affiliates WHERE code = $2 AND status = 'active' AND user_id <> $1
         ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(code)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn balances(pool: &PgPool, affiliate_id: Uuid) -> AppResult<Vec<Balance>> {
    sqlx::query_as(
        "WITH e AS (SELECT currency, sum(amount) s FROM affiliate_commissions WHERE affiliate_id = $1 AND status = 'available' GROUP BY currency),
              w AS (SELECT currency, sum(amount) s FROM affiliate_payouts WHERE affiliate_id = $1 AND status <> 'rejected' GROUP BY currency)
         SELECT COALESCE(e.currency, w.currency)::text AS currency, COALESCE(e.s, 0) AS earned, COALESCE(w.s, 0) AS withdrawn,
                COALESCE(e.s, 0) - COALESCE(w.s, 0) AS available
           FROM e FULL JOIN w ON w.currency = e.currency ORDER BY 1",
    )
    .bind(affiliate_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn commissions(pool: &PgPool, affiliate_id: Uuid) -> AppResult<Vec<Commission>> {
    sqlx::query_as(
        "SELECT c.id, c.invoice_id, i.number AS invoice_number, c.currency::text AS currency, c.amount, c.status, c.created_at
           FROM affiliate_commissions c JOIN invoices i ON i.id = c.invoice_id
          WHERE c.affiliate_id = $1 ORDER BY c.created_at DESC LIMIT 200",
    )
    .bind(affiliate_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

const PCOLS: &str = "p.id, p.affiliate_id, u.email::text AS email, p.currency::text AS currency, p.amount, p.status, p.note, p.created_at";

pub async fn payouts(pool: &PgPool, affiliate_id: Option<Uuid>) -> AppResult<Vec<Payout>> {
    sqlx::query_as(&format!(
        "SELECT {PCOLS} FROM affiliate_payouts p JOIN affiliates a ON a.id = p.affiliate_id JOIN users u ON u.id = a.user_id
          WHERE ($1::uuid IS NULL OR p.affiliate_id = $1) ORDER BY p.created_at DESC LIMIT 200"
    ))
    .bind(affiliate_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Request a payout. The affiliate row is locked so two concurrent requests
/// cannot both spend the same balance.
pub async fn request_payout(
    pool: &PgPool,
    affiliate_id: Uuid,
    currency: &str,
    amount: Decimal,
) -> AppResult<Uuid> {
    if amount <= Decimal::ZERO {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let st: Option<String> =
        sqlx::query_scalar("SELECT status FROM affiliates WHERE id = $1 FOR UPDATE")
            .bind(affiliate_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    if st.as_deref() != Some("active") {
        return Err(AppError::Forbidden);
    }
    let exp: Option<i16> = sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1")
        .bind(currency)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let exp = exp.ok_or(AppError::Validation("error.invalid_currency"))?;
    if amount.normalize().scale() > exp.max(0) as u32 {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    let avail: Decimal = sqlx::query_scalar(
        "SELECT COALESCE((SELECT sum(amount) FROM affiliate_commissions WHERE affiliate_id = $1 AND currency = $2 AND status = 'available'), 0)
              - COALESCE((SELECT sum(amount) FROM affiliate_payouts WHERE affiliate_id = $1 AND currency = $2 AND status <> 'rejected'), 0)",
    )
    .bind(affiliate_id)
    .bind(currency)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if amount > avail {
        return Err(AppError::Validation("error.affiliate.insufficient_balance"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO affiliate_payouts (id, affiliate_id, currency, amount) VALUES ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(affiliate_id)
    .bind(currency)
    .bind(amount)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

/// Admin: settle (`paid`) or reject a requested payout.
pub async fn process_payout(pool: &PgPool, id: Uuid, status: &str, note: &str) -> AppResult<()> {
    if !matches!(status, "paid" | "rejected") || note.len() > 1000 {
        return Err(AppError::Validation("error.validation"));
    }
    let r = sqlx::query(
        "UPDATE affiliate_payouts SET status = $2, note = $3, processed_at = now() WHERE id = $1 AND status = 'requested'",
    )
    .bind(id)
    .bind(status)
    .bind(note)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.affiliate.payout_processed"));
    }
    Ok(())
}

/// (email, locale) of active users that hold `perm` through a role.
pub async fn user_email(pool: &PgPool, user_id: Uuid) -> AppResult<String> {
    sqlx::query_scalar("SELECT email::text FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn staff_recipients(
    pool: &PgPool,
    perm: &str,
) -> AppResult<Vec<(String, Option<String>)>> {
    sqlx::query_as(
        "SELECT DISTINCT u.email::text, u.locale FROM users u
           JOIN user_roles ur ON ur.user_id = u.id
           JOIN role_permissions rp ON rp.role_id = ur.role_id
           JOIN permissions p ON p.id = rp.permission_id
          WHERE p.name = $1 AND u.is_active",
    )
    .bind(perm)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn list(pool: &PgPool, limit: i64, offset: i64) -> AppResult<(Vec<Affiliate>, i64)> {
    let items = sqlx::query_as(&format!(
        "SELECT {COLS} FROM affiliates a JOIN users u ON u.id = a.user_id ORDER BY a.created_at DESC LIMIT $1 OFFSET $2"
    ))
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM affiliates")
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok((items, total))
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AffiliateUpdate {
    #[schema(value_type = String)]
    pub commission_percent: Decimal,
    pub status: String,
}

pub async fn update(pool: &PgPool, id: Uuid, b: &AffiliateUpdate) -> AppResult<()> {
    if b.commission_percent < Decimal::ZERO
        || b.commission_percent > Decimal::ONE_HUNDRED
        || !matches!(b.status.as_str(), "active" | "suspended")
    {
        return Err(AppError::Validation("error.validation"));
    }
    let r = sqlx::query("UPDATE affiliates SET commission_percent = $2, status = $3 WHERE id = $1")
        .bind(id)
        .bind(b.commission_percent)
        .bind(&b.status)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn codes() {
        assert!(super::valid_code("abc123def0"));
        assert!(!super::valid_code("ABC123"));
        assert!(!super::valid_code("abc"));
        assert!(!super::valid_code("abc-123-x"));
    }
}
