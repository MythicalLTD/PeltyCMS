//! `DomainService`: Registered domains and their lifecycle.
//!
//! Pool-owning service that groups the `domains` repository functions as methods. Each
//! method delegates to the free function of the same name, so behaviour is identical
//! and `pelty_database::domains::...` keeps working. Covered by the `domains` integration tests.

#![allow(unused_imports)]
use super::domains;
use super::domains::*;
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::{Date, Month, OffsetDateTime};
use uuid::Uuid;

/// Groups the `domains` repository behind one value that holds the pool.
#[derive(Clone)]
pub struct DomainService {
    pool: PgPool,
}

impl DomainService {
    /// Build the service around a pool (cheap clone).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool, for transactional calls.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Same as [`domains::tlds`].
    pub async fn tlds(&self, active_only: bool, currency: Option<&str>) -> AppResult<Vec<Tld>> {
        domains::tlds(&self.pool, active_only, currency).await
    }

    /// Same as [`domains::tld`].
    pub async fn tld(&self, tld: &str, currency: &str) -> AppResult<Option<Tld>> {
        domains::tld(&self.pool, tld, currency).await
    }

    /// Same as [`domains::upsert_tld`].
    pub async fn upsert_tld(&self, t: &Tld) -> AppResult<Tld> {
        domains::upsert_tld(&self.pool, t).await
    }

    /// Same as [`domains::delete_tld`].
    pub async fn delete_tld(&self, tld: &str, currency: &str) -> AppResult<()> {
        domains::delete_tld(&self.pool, tld, currency).await
    }

    /// Same as [`domains::create_order`].
    pub async fn create_order(&self, o: NewOrder<'_>) -> AppResult<(Domain, invoices::Invoice)> {
        domains::create_order(&self.pool, o).await
    }

    /// Same as [`domains::get`].
    pub async fn get(&self, id: Uuid) -> AppResult<Option<Domain>> {
        domains::get(&self.pool, id).await
    }

    /// Same as [`domains::get_owned`].
    pub async fn get_owned(&self, id: Uuid, customer_id: Uuid) -> AppResult<Domain> {
        domains::get_owned(&self.pool, id, customer_id).await
    }

    /// Same as [`domains::name_taken`].
    pub async fn name_taken(&self, name: &str) -> AppResult<bool> {
        domains::name_taken(&self.pool, name).await
    }

    /// Same as [`domains::list`].
    pub async fn list(&self, customer: Option<Uuid>, status: Option<&str>, q: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Domain>, i64)> {
        domains::list(&self.pool, customer, status, q, limit, offset).await
    }

    /// Same as [`domains::set_nameservers`].
    pub async fn set_nameservers(&self, id: Uuid, ns: &[String]) -> AppResult<()> {
        domains::set_nameservers(&self.pool, id, ns).await
    }

    /// Same as [`domains::set_contact`].
    pub async fn set_contact(&self, id: Uuid, contact: &Value, privacy: bool) -> AppResult<()> {
        domains::set_contact(&self.pool, id, contact, privacy).await
    }

    /// Same as [`domains::set_auto_renew`].
    pub async fn set_auto_renew(&self, id: Uuid, on: bool) -> AppResult<()> {
        domains::set_auto_renew(&self.pool, id, on).await
    }

    /// Same as [`domains::admin_update`].
    pub async fn admin_update(&self, id: Uuid, status: Option<&str>, expires_on: Option<Date>, auto_renew: Option<bool>) -> AppResult<Domain> {
        domains::admin_update(&self.pool, id, status, expires_on, auto_renew).await
    }

    /// Same as [`domains::paid_pending`].
    pub async fn paid_pending(&self) -> AppResult<Vec<(Domain, Option<Vec<u8>>)>> {
        domains::paid_pending(&self.pool).await
    }

    /// Same as [`domains::mark_active`].
    pub async fn mark_active(&self, id: Uuid, registered_on: Date, expires_on: Date) -> AppResult<()> {
        domains::mark_active(&self.pool, id, registered_on, expires_on).await
    }

    /// Same as [`domains::mark_error`].
    pub async fn mark_error(&self, id: Uuid, err: &str, fatal: bool) -> AppResult<()> {
        domains::mark_error(&self.pool, id, err, fatal).await
    }

    /// Same as [`domains::create_renewal`].
    pub async fn create_renewal(&self, d: &Domain, t: &Tld, years: i32) -> AppResult<Option<invoices::Invoice>> {
        domains::create_renewal(&self.pool, d, t, years).await
    }

    /// Same as [`domains::due_auto_renew`].
    pub async fn due_auto_renew(&self, today: Date, days: i64) -> AppResult<Vec<Domain>> {
        domains::due_auto_renew(&self.pool, today, days).await
    }

    /// Same as [`domains::paid_renewals`].
    pub async fn paid_renewals(&self) -> AppResult<Vec<PaidRenewal>> {
        domains::paid_renewals(&self.pool).await
    }

    /// Same as [`domains::apply_renewal`].
    pub async fn apply_renewal(&self, r: &PaidRenewal, new_expiry: Date) -> AppResult<()> {
        domains::apply_renewal(&self.pool, r, new_expiry).await
    }

    /// Same as [`domains::renewals`].
    pub async fn renewals(&self, domain_id: Uuid) -> AppResult<Vec<Value>> {
        domains::renewals(&self.pool, domain_id).await
    }

    /// Same as [`domains::expire_overdue`].
    pub async fn expire_overdue(&self, today: Date) -> AppResult<u64> {
        domains::expire_overdue(&self.pool, today).await
    }

    /// Same as [`domains::dns_records`].
    pub async fn dns_records(&self, domain_id: Uuid) -> AppResult<Vec<DnsRecord>> {
        domains::dns_records(&self.pool, domain_id).await
    }

    /// Same as [`domains::replace_dns`].
    pub async fn replace_dns(&self, domain_id: Uuid, records: &[DnsRecord]) -> AppResult<Vec<DnsRecord>> {
        domains::replace_dns(&self.pool, domain_id, records).await
    }

    /// Same as [`domains::set_flags`].
    pub async fn set_flags(&self, id: Uuid, dns_management: Option<bool>, registrar_lock: Option<bool>, privacy: Option<bool>) -> AppResult<()> {
        domains::set_flags(&self.pool, id, dns_management, registrar_lock, privacy).await
    }

    /// Same as [`domains::set_admin_notes`].
    pub async fn set_admin_notes(&self, id: Uuid, notes: &str) -> AppResult<()> {
        domains::set_admin_notes(&self.pool, id, notes).await
    }

    /// Same as [`domains::mark_synced`].
    pub async fn mark_synced(&self, id: Uuid, expires_on: Date, status: Option<&str>, err: Option<&str>) -> AppResult<()> {
        domains::mark_synced(&self.pool, id, expires_on, status, err).await
    }

    /// Same as [`domains::sync_batch`].
    pub async fn sync_batch(&self, limit: i64) -> AppResult<Vec<Domain>> {
        domains::sync_batch(&self.pool, limit).await
    }

    /// Same as [`domains::bulk_adjust_prices`].
    pub async fn bulk_adjust_prices(&self, tlds: &[String], currency: Option<&str>, percent: Decimal) -> AppResult<u64> {
        domains::bulk_adjust_prices(&self.pool, tlds, currency, percent).await
    }

    /// Same as [`domains::copy_currency`].
    pub async fn copy_currency(&self, from: &str, to: &str, rate: Decimal, overwrite: bool) -> AppResult<u64> {
        domains::copy_currency(&self.pool, from, to, rate, overwrite).await
    }

    /// Same as [`domains::claim_reminder`].
    pub async fn claim_reminder(&self, d: &Domain, days_before: i64) -> AppResult<bool> {
        domains::claim_reminder(&self.pool, d, days_before).await
    }

    /// Same as [`domains::expiring`].
    pub async fn expiring(&self, today: Date, within: i64) -> AppResult<Vec<Domain>> {
        domains::expiring(&self.pool, today, within).await
    }
}
