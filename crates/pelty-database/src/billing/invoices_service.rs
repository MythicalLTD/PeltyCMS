//! `InvoiceService`: Invoice reads and pool-level invoice actions (void, credit/debit notes, payments).
//!
//! An object-style entry point that owns the pool and groups the `invoices` repository
//! functions as methods. Each method delegates to the free function of the same name,
//! so behaviour is identical and old callers (`pelty_database::invoices::...`) keep working.
//! Covered by the same integration tests as `invoices`.

use super::invoices;
use super::invoices::*;
#[allow(unused_imports)]
use pelty_core::AppResult;
use sqlx::PgPool;
#[allow(unused_imports)]
use rust_decimal::Decimal;
#[allow(unused_imports)]
use uuid::Uuid;

/// Groups the `invoices` repository behind one value that holds the pool.
#[derive(Clone)]
pub struct InvoiceService {
    pool: PgPool,
}

impl InvoiceService {
    /// Build the service around a pool (cheap: `PgPool` is reference counted).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool, for calls that still need it directly (transactions).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Same as [`invoices::get`].
    pub async fn get(&self, id: Uuid) -> AppResult<Option<Invoice>> {
        invoices::get(&self.pool, id).await
    }

    /// Same as [`invoices::get_owned`].
    pub async fn get_owned(&self, id: Uuid, customer_id: Uuid) -> AppResult<Option<Invoice>> {
        invoices::get_owned(&self.pool, id, customer_id).await
    }

    /// Same as [`invoices::items`].
    pub async fn items(&self, invoice_id: Uuid) -> AppResult<Vec<Item>> {
        invoices::items(&self.pool, invoice_id).await
    }

    /// Same as [`invoices::void`].
    pub async fn void(&self, id: Uuid) -> AppResult<Invoice> {
        invoices::void(&self.pool, id).await
    }

    /// Same as [`invoices::credit_note`].
    pub async fn credit_note(&self, invoice_id: Uuid) -> AppResult<Invoice> {
        invoices::credit_note(&self.pool, invoice_id).await
    }

    /// Same as [`invoices::payments`].
    pub async fn payments(&self, invoice_id: Uuid) -> AppResult<Vec<Payment>> {
        invoices::payments(&self.pool, invoice_id).await
    }

    /// Same as [`invoices::debit_note`].
    pub async fn debit_note(&self, invoice_id: Uuid, lines: &[DebitLine], reason: &str, due_days: i64) -> AppResult<Invoice> {
        invoices::debit_note(&self.pool, invoice_id, lines, reason, due_days).await
    }

    /// Same as [`invoices::debit_notes_for`].
    pub async fn debit_notes_for(&self, invoice_id: Uuid) -> AppResult<Vec<Invoice>> {
        invoices::debit_notes_for(&self.pool, invoice_id).await
    }

    /// Same as [`invoices::note_reason`].
    pub async fn note_reason(&self, id: Uuid) -> AppResult<String> {
        invoices::note_reason(&self.pool, id).await
    }

    /// Same as [`invoices::credit_notes_for`].
    pub async fn credit_notes_for(&self, invoice_id: Uuid) -> AppResult<Vec<Invoice>> {
        invoices::credit_notes_for(&self.pool, invoice_id).await
    }

    /// Same as [`invoices::list`].
    pub async fn list(&self, customer: Option<Uuid>, status: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Invoice>, i64)> {
        invoices::list(&self.pool, customer, status, limit, offset).await
    }

    /// Same as [`invoices::create_pending`].
    pub async fn create_pending(&self, inv: &Invoice, gateway: &str, user: Option<Uuid>, key: &str) -> AppResult<Payment> {
        invoices::create_pending(&self.pool, inv, gateway, user, key).await
    }

    /// Same as [`invoices::create_pending_multi`].
    pub async fn create_pending_multi(&self, invs: &[Invoice], gateway: &str, user: Option<Uuid>, key: &str) -> AppResult<Payment> {
        invoices::create_pending_multi(&self.pool, invs, gateway, user, key).await
    }

    /// Same as [`invoices::set_provider_ref`].
    pub async fn set_provider_ref(&self, payment_id: Uuid, provider_ref: &str) -> AppResult<()> {
        invoices::set_provider_ref(&self.pool, payment_id, provider_ref).await
    }

    /// Same as [`invoices::record_payment`].
    pub async fn record_payment(&self, invoice_id: Uuid, gateway: &str, provider_ref: Option<&str>, amount: Decimal, key: &str, actor: Option<Uuid>) -> AppResult<Applied> {
        invoices::record_payment(&self.pool, invoice_id, gateway, provider_ref, amount, key, actor).await
    }

    /// Same as [`invoices::apply_by_ref`].
    pub async fn apply_by_ref(&self, gateway: &str, provider_ref: &str) -> AppResult<Option<Applied>> {
        invoices::apply_by_ref(&self.pool, gateway, provider_ref).await
    }
}
