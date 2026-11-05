//! `OrderService`: Orders: placing, listing and changing order state.
//!
//! Pool-owning service that groups the `orders` repository functions as methods. Each
//! method delegates to the free function of the same name, so behaviour is identical
//! and `pelty_database::orders::...` keeps working. Covered by the `orders` integration tests.

#![allow(unused_imports)]
use super::orders;
use super::orders::*;
use crate::addons;
use crate::catalog;
use crate::coupons;
use crate::db_err;
use crate::invoices::{self, Invoice, NewItem};
use pelty_billing::cycle;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

/// Groups the `orders` repository behind one value that holds the pool.
#[derive(Clone)]
pub struct OrderService {
    pool: PgPool,
}

impl OrderService {
    /// Build the service around a pool (cheap clone).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool, for transactional calls.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Same as [`orders::place`].
    pub async fn place(&self, customer_id: Uuid, currency: &str, items: &[CartItem], coupon: Option<&str>, due_days: i64) -> AppResult<(Order, Invoice)> {
        orders::place(&self.pool, customer_id, currency, items, coupon, due_days).await
    }

    /// Same as [`orders::place_with`].
    pub async fn place_with(&self, customer_id: Uuid, currency: &str, items: &[CartItem], coupon: Option<&str>, due_days: i64, bundle: Option<&BundleDiscount>) -> AppResult<(Order, Invoice)> {
        orders::place_with(&self.pool, customer_id, currency, items, coupon, due_days, bundle).await
    }

    /// Same as [`orders::line_amounts`].
    pub async fn line_amounts(&self, currency: &str, items: &[CartItem]) -> AppResult<Vec<catalog::Quote>> {
        orders::line_amounts(&self.pool, currency, items).await
    }

    /// Same as [`orders::place_full`].
    #[allow(clippy::too_many_arguments)]
    pub async fn place_full(&self, customer_id: Uuid, currency: &str, items: &[CartItem], coupon: Option<&str>, due_days: i64, bundle: Option<&BundleDiscount>, addon_discounts: &[AddonDiscount]) -> AppResult<(Order, Invoice)> {
        orders::place_full(&self.pool, customer_id, currency, items, coupon, due_days, bundle, addon_discounts).await
    }

    /// Same as [`orders::get`].
    pub async fn get(&self, id: Uuid) -> AppResult<Option<Order>> {
        orders::get(&self.pool, id).await
    }

    /// Same as [`orders::get_by_number`].
    pub async fn get_by_number(&self, number: i64) -> AppResult<Option<Order>> {
        orders::get_by_number(&self.pool, number).await
    }

    /// Same as [`orders::list`].
    pub async fn list(&self, customer: Option<Uuid>, status: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Order>, i64)> {
        orders::list(&self.pool, customer, status, limit, offset).await
    }

    /// Same as [`orders::approve`].
    pub async fn approve(&self, id: Uuid, actor: impl Into<Option<Uuid>>) -> AppResult<Vec<Uuid>> {
        orders::approve(&self.pool, id, actor).await
    }

    /// Same as [`orders::cancel`].
    pub async fn cancel(&self, id: Uuid, fraud: bool) -> AppResult<Order> {
        orders::cancel(&self.pool, id, fraud).await
    }

    /// Same as [`orders::reopen`].
    pub async fn reopen(&self, id: Uuid) -> AppResult<Order> {
        orders::reopen(&self.pool, id).await
    }

    /// Same as [`orders::delete`].
    pub async fn delete(&self, id: Uuid) -> AppResult<()> {
        orders::delete(&self.pool, id).await
    }

    /// Same as [`orders::items`].
    pub async fn items(&self, order_id: Uuid) -> AppResult<Vec<OrderItem>> {
        orders::items(&self.pool, order_id).await
    }
}
