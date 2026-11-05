//! Key/value settings repository used by every typed settings module.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;

pub async fn get(pool: &PgPool, key: &str) -> AppResult<Option<serde_json::Value>> {
    sqlx::query_scalar("SELECT value FROM system_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn set(pool: &PgPool, key: &str, value: serde_json::Value) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO system_settings (key, value) VALUES ($1,$2)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub const BANK_TRANSFER_KEY: &str = "billing.bank_transfer";

/// Operator bank details shown with manual-transfer instructions. Not secret.
#[derive(
    Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct BankTransfer {
    pub beneficiary: String,
    pub iban: String,
    pub bic: String,
    pub bank_name: String,
    /// Free-form note appended to the instructions.
    pub note: String,
}

impl BankTransfer {
    pub fn is_configured(&self) -> bool {
        !self.iban.is_empty() || !self.note.trim().is_empty()
    }
    /// Normalize (uppercase, strip spaces) and validate. Returns an error code.
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        self.iban = self
            .iban
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_uppercase();
        self.bic = self.bic.trim().to_uppercase();
        for (v, max) in [
            (&self.beneficiary, 140),
            (&self.bank_name, 140),
            (&self.note, 2000),
        ] {
            if v.chars().count() > max {
                return Err("error.validation");
            }
        }
        if !self.iban.is_empty() && !iban_valid(&self.iban) {
            return Err("error.invalid_iban");
        }
        if !(self.bic.is_empty() || self.bic.len() == 8 || self.bic.len() == 11)
            || !self.bic.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err("error.invalid_bic");
        }
        Ok(self)
    }
}

/// ISO 13616 mod-97 check.
pub fn iban_valid(iban: &str) -> bool {
    let b = iban.as_bytes();
    if !(15..=34).contains(&b.len())
        || !b[..2].iter().all(u8::is_ascii_uppercase)
        || !b[2..4].iter().all(u8::is_ascii_digit)
        || !b.iter().all(u8::is_ascii_alphanumeric)
    {
        return false;
    }
    let mut rem: u32 = 0;
    for &c in b[4..].iter().chain(&b[..4]) {
        let v = if c.is_ascii_digit() {
            (c - b'0') as u32
        } else {
            (c - b'A') as u32 + 10
        };
        rem = if v >= 10 {
            (rem * 100 + v) % 97
        } else {
            (rem * 10 + v) % 97
        };
    }
    rem == 1
}

pub async fn bank_transfer(pool: &PgPool) -> AppResult<BankTransfer> {
    Ok(get(pool, BANK_TRANSFER_KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iban_and_bic() {
        assert!(iban_valid("RO49AAAA1B31007593840000"));
        assert!(iban_valid("DE89370400440532013000"));
        assert!(!iban_valid("RO49AAAA1B31007593840001"));
        assert!(!iban_valid("RO00"));
        let b = BankTransfer {
            iban: "de89 3704 0044 0532 0130 00".into(),
            bic: "cobadeffxxx".into(),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(
            (b.iban.as_str(), b.bic.as_str()),
            ("DE89370400440532013000", "COBADEFFXXX")
        );
        assert_eq!(
            BankTransfer {
                bic: "X".into(),
                ..Default::default()
            }
            .normalized(),
            Err("error.invalid_bic")
        );
        assert!(!BankTransfer::default().is_configured());
    }
}
