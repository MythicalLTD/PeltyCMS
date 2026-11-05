//! `HostingService`: Active customer services (hosting accounts, products) and their state.
//!
//! Pool-owning service that groups the `services` repository functions as methods. Each
//! method delegates to the free function of the same name, so behaviour is identical
//! and `pelty_database::services::...` keeps working. Covered by the `services` integration tests.

#![allow(unused_imports)]
use super::services;
use super::services::*;
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_billing::{cycle, lifecycle};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// Groups the `services` repository behind one value that holds the pool.
#[derive(Clone)]
pub struct HostingService {
    pool: PgPool,
}

impl HostingService {
    /// Build the service around a pool (cheap clone).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool, for transactional calls.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Same as [`services::get`].
    pub async fn get(&self, id: Uuid) -> AppResult<Option<Service>> {
        services::get(&self.pool, id).await
    }

    /// Same as [`services::get_owned`].
    pub async fn get_owned(&self, id: Uuid, customer_id: Uuid) -> AppResult<Option<Service>> {
        services::get_owned(&self.pool, id, customer_id).await
    }

    /// Same as [`services::list`].
    pub async fn list(&self, customer: Option<Uuid>, status: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Service>, i64)> {
        services::list(&self.pool, customer, status, limit, offset).await
    }

    /// Same as [`services::transition`].
    pub async fn transition(&self, id: Uuid, action: &str, reason: &str) -> AppResult<(String, Service)> {
        services::transition(&self.pool, id, action, reason).await
    }

    /// Same as [`services::bill_due`].
    pub async fn bill_due(&self, horizon: Date, batch: i64, due_days: i64) -> AppResult<Vec<Renewal>> {
        services::bill_due(&self.pool, horizon, batch, due_days).await
    }

    /// Same as [`services::bill_due_ahead`].
    pub async fn bill_due_ahead(&self, today: Date, default_days: i64, batch: i64, due_days: i64) -> AppResult<Vec<Renewal>> {
        services::bill_due_ahead(&self.pool, today, default_days, batch, due_days).await
    }

    /// Same as [`services::renew_now`].
    pub async fn renew_now(&self, sid: Uuid, customer: Option<Uuid>, max_days_ahead: i64, today: Date) -> AppResult<EarlyRenewal> {
        services::renew_now(&self.pool, sid, customer, max_days_ahead, today).await
    }
}
