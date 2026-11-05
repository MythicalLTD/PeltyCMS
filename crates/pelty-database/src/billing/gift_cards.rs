//! Gift cards / prepaid credit codes. Codes are shown once at creation and
//! stored as SHA-256; redeeming moves the full balance to account credit.
use crate::{db_err, invoices};
use pelty_core::{AppError, AppResult};
use rand::Rng;
use rust_decimal::Decimal;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GiftCard {
    pub id: Uuid,
    pub code_last4: String,
    pub currency: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub balance: Decimal,
    pub status: String,
    pub note: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub redeemed_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub redeemed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, code_last4, currency::text AS currency, amount, balance, status, note, expires_at, redeemed_by, redeemed_at, created_at";
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Uppercase, strip spaces/dashes.
pub fn normalize(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

pub fn hash(code: &str) -> String {
    hex::encode(Sha256::digest(normalize(code).as_bytes()))
}

/// `XXXX-XXXX-XXXX-XXXX` from an unambiguous alphabet (80 bits).
pub fn generate() -> String {
    let mut r = rand::thread_rng();
    (0..4)
        .map(|_| (0..4).map(|_| ALPHABET[r.gen_range(0..ALPHABET.len())] as char).collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Create `count` cards; returns (card, plaintext code) once.
pub async fn create(
    pool: &PgPool,
    count: u32,
    currency: &str,
    amount: Decimal,
    note: &str,
    expires_at: Option<OffsetDateTime>,
    actor: Option<Uuid>,
) -> AppResult<Vec<(GiftCard, String)>> {
    if count == 0 || count > 500 || note.chars().count() > 500 {
        return Err(AppError::Validation("error.validation"));
    }
    let currency = currency.trim().to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exp = invoices::exponent(&mut tx, &currency).await?;
    if amount <= Decimal::ZERO || amount.normalize().scale() > exp || amount > Decimal::from(1_000_000) {
        return Err(AppError::Validation("error.invalid_amount"));
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let code = generate();
        let n = normalize(&code);
        let card: GiftCard = sqlx::query_as(&format!(
            "INSERT INTO gift_cards (id, code_hash, code_last4, currency, amount, balance, note, expires_at, created_by)
             VALUES ($1,$2,$3,$4,$5,$5,$6,$7,$8) RETURNING {COLS}"
        ))
        .bind(Uuid::new_v4())
        .bind(hash(&n))
        .bind(&n[n.len() - 4..])
        .bind(&currency)
        .bind(amount)
        .bind(note.trim())
        .bind(expires_at)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        out.push((card, code));
    }
    tx.commit().await.map_err(db_err)?;
    Ok(out)
}

pub async fn list(pool: &PgPool, limit: i64) -> AppResult<Vec<GiftCard>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM gift_cards ORDER BY created_at DESC, id LIMIT $1"))
        .bind(limit.clamp(1, 1000))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn disable(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("UPDATE gift_cards SET status = 'disabled' WHERE id = $1 AND status = 'active'")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.gift_card_not_active"));
    }
    Ok(())
}

/// Redeem the full balance into the customer's credit. Unknown, used,
/// disabled and expired codes all answer the same error.
pub async fn redeem(pool: &PgPool, customer: Uuid, code: &str) -> AppResult<(String, Decimal)> {
    let n = normalize(code);
    if n.len() != 16 {
        return Err(AppError::Validation("error.gift_card_invalid"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(Uuid, String, Decimal, String)> = sqlx::query_as(
        "SELECT id, currency::text, balance, code_last4 FROM gift_cards
         WHERE code_hash = $1 AND status = 'active' AND balance > 0 AND (expires_at IS NULL OR expires_at > now()) FOR UPDATE",
    )
    .bind(hash(&n))
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (id, currency, bal, last4) = row.ok_or(AppError::Validation("error.gift_card_invalid"))?;
    sqlx::query("UPDATE gift_cards SET balance = 0, status = 'redeemed', redeemed_by = $2, redeemed_at = now() WHERE id = $1")
        .bind(id)
        .bind(customer)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, note) VALUES ($1,$2,$3,$4,'gift_card',$5)")
        .bind(Uuid::new_v4())
        .bind(customer)
        .bind(&currency)
        .bind(bal)
        .bind(format!("Gift card ****{last4}"))
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((currency, bal))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes() {
        let c = generate();
        assert_eq!(c.len(), 19);
        assert_eq!(normalize(&c).len(), 16);
        assert_eq!(hash(&c.to_lowercase()), hash(&c.replace('-', " ")));
        assert!(!normalize(&c).contains('O') && !normalize(&c).contains('0'));
    }
}
