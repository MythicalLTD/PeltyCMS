//! Saved-card expiry notices (WHMCS `--CreditCardExpiryNotices`).
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

pub struct Expiring {
    pub payment_method_id: Uuid,
    pub customer_id: Uuid,
    pub brand: String,
    pub last4: String,
    pub exp_month: i16,
    pub exp_year: i16,
}

/// Claim cards expiring in `today`'s month or the next one (not yet
/// expired, not yet notified). Each card is returned once, ever.
pub async fn claim(pool: &PgPool, today: Date) -> AppResult<Vec<Expiring>> {
    let first = today.replace_day(1).unwrap_or(today);
    let rows: Vec<(Uuid, Uuid, String, String, i16, i16)> = sqlx::query_as(
        "UPDATE payment_methods SET expiry_notified_at = now()
         WHERE deleted_at IS NULL AND expiry_notified_at IS NULL
           AND make_date(exp_year::int, exp_month::int, 1) BETWEEN $1 AND ($1 + interval '1 month')::date
         RETURNING id, customer_id, brand, last4, exp_month, exp_year",
    )
    .bind(first)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(payment_method_id, customer_id, brand, last4, exp_month, exp_year)| Expiring { payment_method_id, customer_id, brand, last4, exp_month, exp_year })
        .collect())
}
