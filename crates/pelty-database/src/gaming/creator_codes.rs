//! Creator codes (module `creator_codes`): an affiliate's vanity code that
//! buyers enter at checkout. Credits the order to the affiliate and may give
//! an admin-set percent off the first invoice.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "creator_codes";
/// Highest discount an admin may attach to a code.
pub const MAX_DISCOUNT: u32 = 50;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CreatorCode {
    pub id: Uuid,
    pub affiliate_id: Uuid,
    pub code: String,
    #[schema(value_type = String)]
    pub discount_percent: Decimal,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CreatorCodeRow {
    pub id: Uuid,
    pub affiliate_id: Uuid,
    pub code: String,
    #[schema(value_type = String)]
    pub discount_percent: Decimal,
    pub enabled: bool,
    pub email: String,
    pub affiliate_status: String,
    pub orders: i64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatorCodeUpdate {
    #[schema(value_type = String)]
    pub discount_percent: Decimal,
    pub enabled: bool,
}

const COLS: &str = "id, affiliate_id, code, discount_percent, enabled, created_at";

/// Trimmed, lower-cased code if it has the allowed shape (3–24 of a-z 0-9 _ -).
pub fn normalize(code: &str) -> Option<String> {
    let c = code.trim().to_ascii_lowercase();
    let ok = (3..=24).contains(&c.len())
        && c.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    ok.then_some(c)
}

pub async fn for_affiliate(pool: &PgPool, affiliate_id: Uuid) -> AppResult<Option<CreatorCode>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM creator_codes WHERE affiliate_id = $1"
    )))
    .bind(affiliate_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Create or rename the affiliate's code. Taken codes are a conflict.
pub async fn set_code(pool: &PgPool, affiliate_id: Uuid, code: &str) -> AppResult<CreatorCode> {
    let code = normalize(code).ok_or(AppError::Validation("error.creator_code_format"))?;
    let r = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO creator_codes (affiliate_id, code) VALUES ($1, $2)
         ON CONFLICT (affiliate_id) DO UPDATE SET code = EXCLUDED.code, updated_at = now()
         RETURNING {COLS}"
    )))
    .bind(affiliate_id)
    .bind(&code)
    .fetch_one(pool)
    .await;
    match r {
        Ok(c) => Ok(c),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(AppError::Conflict("error.creator_code_taken"))
        }
        Err(e) => Err(db_err(e)),
    }
}

/// A usable code for this buyer: exists, enabled, affiliate active, and not
/// the buyer's own.
pub async fn check(pool: &PgPool, code: &str, customer: Option<Uuid>) -> AppResult<CreatorCode> {
    let code = normalize(code).ok_or(AppError::Validation("error.creator_code_invalid"))?;
    let row: Option<(Uuid, Uuid, String, Decimal, bool, OffsetDateTime, Uuid)> = sqlx::query_as(
        "SELECT cc.id, cc.affiliate_id, cc.code, cc.discount_percent, cc.enabled, cc.created_at, af.user_id
           FROM creator_codes cc JOIN affiliates af ON af.id = cc.affiliate_id
          WHERE cc.code = $1 AND cc.enabled AND af.status = 'active'",
    )
    .bind(&code)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (id, affiliate_id, code, discount_percent, enabled, created_at, owner) =
        row.ok_or(AppError::Validation("error.creator_code_invalid"))?;
    if let Some(cid) = customer {
        let mine: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM customers WHERE id = $1 AND owner_user_id = $2)",
        )
        .bind(cid)
        .bind(owner)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
        if mine {
            return Err(AppError::Validation("error.creator_code_own"));
        }
    }
    Ok(CreatorCode {
        id,
        affiliate_id,
        code,
        discount_percent,
        enabled,
        created_at,
    })
}

/// Remember which code an order used (credited when its invoice is paid).
pub async fn attach(pool: &PgPool, order_id: Uuid, code_id: Uuid) -> AppResult<()> {
    sqlx::query("INSERT INTO creator_code_orders (order_id, code_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(order_id)
        .bind(code_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Orders placed with this code.
pub async fn uses(pool: &PgPool, code_id: Uuid) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM creator_code_orders WHERE code_id = $1")
        .bind(code_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<CreatorCodeRow>> {
    sqlx::query_as(
        "SELECT cc.id, cc.affiliate_id, cc.code, cc.discount_percent, cc.enabled, u.email,
                af.status AS affiliate_status,
                (SELECT count(*) FROM creator_code_orders o WHERE o.code_id = cc.id) AS orders
           FROM creator_codes cc
           JOIN affiliates af ON af.id = cc.affiliate_id
           JOIN users u ON u.id = af.user_id
          ORDER BY cc.code LIMIT 1000",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, b: &CreatorCodeUpdate) -> AppResult<()> {
    if b.discount_percent < Decimal::ZERO
        || b.discount_percent > Decimal::from(MAX_DISCOUNT)
        || b.discount_percent.scale() > 2
    {
        return Err(AppError::Validation("error.creator_code_discount"));
    }
    let n = sqlx::query(
        "UPDATE creator_codes SET discount_percent = $2, enabled = $3, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(b.discount_percent)
    .bind(b.enabled)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_normalized() {
        assert_eq!(normalize("  CoolGuy_99 ").as_deref(), Some("coolguy_99"));
        assert_eq!(normalize("ab"), None);
        assert_eq!(normalize("has space"), None);
        assert_eq!(normalize("émoji"), None);
        assert_eq!(normalize(&"x".repeat(25)), None);
        assert_eq!(normalize("a-b"), Some("a-b".into()));
    }
}
