//! Add funds: a client-requested invoice whose payment is credited to the
//! account balance (see migration 0272 trigger). Limits live in settings.
use crate::{db_err, invoices, settings};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

pub const KEY: &str = "billing.add_funds";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AddFundsSettings {
    pub enabled: bool,
    /// Smallest single deposit.
    #[schema(value_type = String)]
    pub min: Decimal,
    /// Largest single deposit.
    #[schema(value_type = String)]
    pub max: Decimal,
    /// Highest credit balance (existing + unpaid deposits + this one). 0 = no cap.
    #[schema(value_type = String)]
    pub max_balance: Decimal,
    /// Only customers with at least one active service may deposit.
    pub require_active_service: bool,
    /// Apply available credit to new invoices automatically (WHMCS
    /// "Credit Auto-Apply"). Off by default.
    pub auto_apply_credit: bool,
}

impl Default for AddFundsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            min: Decimal::from(10),
            max: Decimal::from(500),
            max_balance: Decimal::from(1000),
            require_active_service: false,
            auto_apply_credit: false,
        }
    }
}

impl AddFundsSettings {
    pub fn normalized(self) -> Result<Self, &'static str> {
        let cap = Decimal::from(1_000_000_000);
        if self.min <= Decimal::ZERO
            || self.max < self.min
            || self.max > cap
            || self.max_balance < Decimal::ZERO
            || self.max_balance > cap
        {
            return Err("error.invalid_amount");
        }
        if self.max_balance > Decimal::ZERO && self.max_balance < self.min {
            return Err("error.invalid_amount");
        }
        Ok(self)
    }

    /// Validate one deposit against the limits (amounts in invoice currency).
    pub fn check(
        &self,
        amount: Decimal,
        exp: u32,
        balance: Decimal,
        pending: Decimal,
    ) -> Result<(), &'static str> {
        if !self.enabled {
            return Err("error.add_funds_disabled");
        }
        if amount <= Decimal::ZERO || amount.normalize().scale() > exp {
            return Err("error.invalid_amount");
        }
        if amount < self.min {
            return Err("error.add_funds_below_min");
        }
        if amount > self.max {
            return Err("error.add_funds_above_max");
        }
        if self.max_balance > Decimal::ZERO && balance + pending + amount > self.max_balance {
            return Err("error.add_funds_balance_cap");
        }
        Ok(())
    }
}

pub async fn get_settings(pool: &PgPool) -> AppResult<AddFundsSettings> {
    Ok(settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put_settings(pool: &PgPool, s: &AddFundsSettings) -> AppResult<()> {
    settings::set(pool, KEY, serde_json::to_value(s).unwrap_or_default()).await
}

pub async fn is_add_funds(pool: &PgPool, invoice: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM add_funds WHERE invoice_id = $1)")
        .bind(invoice)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Create and finalize the deposit invoice. Returns its id.
pub async fn create(
    pool: &PgPool,
    customer: Uuid,
    currency: &str,
    amount: Decimal,
    description: &str,
) -> AppResult<Uuid> {
    let cfg = get_settings(pool).await?;
    let currency = currency.trim().to_ascii_uppercase();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exp = invoices::exponent(&mut tx, &currency).await?;
    // Serialize deposits per customer so the balance cap holds under races.
    sqlx::query("SELECT 1 FROM customers WHERE id = $1 FOR UPDATE")
        .bind(customer)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    if cfg.require_active_service {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM services WHERE customer_id = $1 AND status = 'active')",
        )
        .bind(customer)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        if !ok {
            return Err(AppError::Conflict("error.add_funds_needs_service"));
        }
    }
    let balance: Decimal = sqlx::query_scalar("SELECT COALESCE(sum(amount),0) FROM credit_ledger WHERE customer_id = $1 AND currency = $2")
        .bind(customer).bind(&currency).fetch_one(&mut *tx).await.map_err(db_err)?;
    let pending: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(sum(a.amount),0) FROM add_funds a JOIN invoices i ON i.id = a.invoice_id
         WHERE a.customer_id = $1 AND a.currency = $2 AND a.credited_at IS NULL AND i.status = 'unpaid'",
    )
    .bind(customer).bind(&currency).fetch_one(&mut *tx).await.map_err(db_err)?;
    cfg.check(amount, exp, balance, pending).map_err(|e| {
        if e == "error.add_funds_disabled" {
            AppError::Forbidden
        } else {
            AppError::Validation(e)
        }
    })?;
    let id = invoices::create_draft(&mut tx, customer, &currency, None).await?;
    invoices::add_items(
        &mut tx,
        id,
        &currency,
        &[invoices::NewItem {
            description: description.to_string(),
            quantity: Decimal::ONE,
            unit_price: amount,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: false,
        }],
    )
    .await?;
    sqlx::query(
        "INSERT INTO add_funds (invoice_id, customer_id, currency, amount) VALUES ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(customer)
    .bind(&currency)
    .bind(amount)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    invoices::finalize(&mut tx, id, 7).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }
    #[test]
    fn limits() {
        let c = AddFundsSettings {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(c.check(d("10"), 2, d("0"), d("0")), Ok(()));
        assert_eq!(
            c.check(d("9.99"), 2, d("0"), d("0")),
            Err("error.add_funds_below_min")
        );
        assert_eq!(
            c.check(d("500.01"), 2, d("0"), d("0")),
            Err("error.add_funds_above_max")
        );
        assert_eq!(
            c.check(d("10.001"), 2, d("0"), d("0")),
            Err("error.invalid_amount")
        );
        assert_eq!(
            c.check(d("100"), 2, d("850"), d("60")),
            Err("error.add_funds_balance_cap")
        );
        assert_eq!(c.check(d("90"), 2, d("850"), d("60")), Ok(()));
        assert_eq!(
            AddFundsSettings::default().check(d("10"), 2, d("0"), d("0")),
            Err("error.add_funds_disabled")
        );
        assert!(AddFundsSettings {
            max: d("5"),
            ..Default::default()
        }
        .normalized()
        .is_err());
    }
}
