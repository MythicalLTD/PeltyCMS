//! Affiliate extras: per-product commission rates, referral-count tiers and
//! promotional materials. Accrual itself stays in the `affiliate_accrue` trigger.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ProductRate {
    pub product_id: Uuid,
    #[serde(default)]
    #[sqlx(default)]
    pub product_name: String,
    /// `percent` | `fixed`
    pub kind: String,
    #[schema(value_type = String)]
    pub value: Decimal,
    /// Required for `fixed`.
    pub currency: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Tier {
    pub id: Uuid,
    pub name: String,
    pub min_referrals: i32,
    #[schema(value_type = String)]
    pub commission_percent: Decimal,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TierInput {
    pub name: String,
    pub min_referrals: i32,
    #[schema(value_type = String)]
    pub commission_percent: Decimal,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Material {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub image_url: String,
    pub sort_order: i32,
    pub active: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialInput {
    /// `banner` | `text`
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "yes")]
    pub active: bool,
}
fn yes() -> bool {
    true
}

fn pct_ok(v: Decimal) -> bool {
    v >= Decimal::ZERO && v <= Decimal::ONE_HUNDRED
}

pub async fn rates(pool: &PgPool) -> AppResult<Vec<ProductRate>> {
    sqlx::query_as(
        "SELECT r.product_id, p.name AS product_name, r.kind, r.value, r.currency::text AS currency
         FROM affiliate_product_rates r JOIN products p ON p.id = r.product_id ORDER BY p.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_rate(pool: &PgPool, r: &ProductRate) -> AppResult<()> {
    let currency = match r.kind.as_str() {
        "percent" if pct_ok(r.value) => None,
        "fixed" if r.value >= Decimal::ZERO => Some(
            r.currency
                .clone()
                .filter(|c| c.len() == 3)
                .ok_or(AppError::Validation("error.invalid_currency"))?,
        ),
        "percent" | "fixed" => return Err(AppError::Validation("error.invalid_amount")),
        _ => return Err(AppError::Validation("error.validation")),
    };
    if r.value.scale() > 4 {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    sqlx::query("INSERT INTO affiliate_product_rates (product_id, kind, value, currency) VALUES ($1, $2, $3, $4::char(3))
         ON CONFLICT (product_id) DO UPDATE SET kind = EXCLUDED.kind, value = EXCLUDED.value, currency = EXCLUDED.currency")
        .bind(r.product_id).bind(&r.kind).bind(r.value).bind(currency)
        .execute(pool).await.map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
                if d.constraint().is_some_and(|c| c.contains("currency")) { AppError::Validation("error.invalid_currency") } else { AppError::NotFound }
            }
            _ => db_err(e),
        })?;
    Ok(())
}

pub async fn delete_rate(pool: &PgPool, product_id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM affiliate_product_rates WHERE product_id = $1")
        .bind(product_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

pub async fn tiers(pool: &PgPool) -> AppResult<Vec<Tier>> {
    sqlx::query_as("SELECT id, name, min_referrals, commission_percent FROM affiliate_tiers ORDER BY min_referrals")
        .fetch_all(pool).await.map_err(db_err)
}

pub async fn create_tier(pool: &PgPool, t: &TierInput) -> AppResult<Tier> {
    let name = t.name.trim();
    if name.is_empty()
        || name.chars().count() > 100
        || t.min_referrals < 1
        || !pct_ok(t.commission_percent)
        || t.commission_percent.scale() > 4
    {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as("INSERT INTO affiliate_tiers (id, name, min_referrals, commission_percent) VALUES ($1, $2, $3, $4)
         RETURNING id, name, min_referrals, commission_percent")
        .bind(Uuid::new_v4()).bind(name).bind(t.min_referrals).bind(t.commission_percent)
        .fetch_one(pool).await.map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.tier_exists"),
            _ => db_err(e),
        })
}

pub async fn delete_tier(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM affiliate_tiers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

/// Effective base percent for an affiliate (own rate or best tier reached).
pub async fn effective_percent(pool: &PgPool, affiliate_id: Uuid) -> AppResult<Decimal> {
    let v: Option<Decimal> = sqlx::query_scalar("SELECT affiliate_percent($1)")
        .bind(affiliate_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok(v.unwrap_or_default())
}

const MCOLS: &str = "id, kind, title, body, image_url, sort_order, active, created_at";

pub async fn materials(pool: &PgPool, only_active: bool) -> AppResult<Vec<Material>> {
    sqlx::query_as(&format!("SELECT {MCOLS} FROM affiliate_materials WHERE active OR NOT $1 ORDER BY sort_order, created_at"))
        .bind(only_active).fetch_all(pool).await.map_err(db_err)
}

fn check_material(m: &MaterialInput) -> AppResult<()> {
    let url_ok = m.image_url.is_empty()
        || ((m.image_url.starts_with("https://") || m.image_url.starts_with("http://"))
            && m.image_url.len() <= 1000);
    let title = m.title.trim();
    if !matches!(m.kind.as_str(), "banner" | "text")
        || title.is_empty()
        || title.chars().count() > 200
        || m.body.chars().count() > 5000
        || !url_ok
    {
        return Err(AppError::Validation("error.validation"));
    }
    if m.kind == "banner" && m.image_url.is_empty() {
        return Err(AppError::Validation("error.image_url_required"));
    }
    Ok(())
}

pub async fn save_material(
    pool: &PgPool,
    id: Option<Uuid>,
    m: &MaterialInput,
) -> AppResult<Material> {
    check_material(m)?;
    let q = match id {
        None => format!("INSERT INTO affiliate_materials (id, kind, title, body, image_url, sort_order, active) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {MCOLS}"),
        Some(_) => format!("UPDATE affiliate_materials SET kind=$2, title=$3, body=$4, image_url=$5, sort_order=$6, active=$7 WHERE id=$1 RETURNING {MCOLS}"),
    };
    sqlx::query_as(&q)
        .bind(id.unwrap_or_else(Uuid::new_v4))
        .bind(&m.kind)
        .bind(m.title.trim())
        .bind(&m.body)
        .bind(&m.image_url)
        .bind(m.sort_order)
        .bind(m.active)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn delete_material(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM affiliate_materials WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}
