//! `ClientService`: Client (customer) records and their contacts.
//!
//! An object-style entry point that owns the pool and groups the `customers` repository
//! functions as methods. Each method delegates to the free function of the same name,
//! so behaviour is identical and old callers (`pelty_database::customers::...`) keep working.
//! Covered by the same integration tests as `customers`.

use super::customers;
use super::customers::*;
#[allow(unused_imports)]
use pelty_core::AppResult;
use sqlx::PgPool;
#[allow(unused_imports)]
use uuid::Uuid;

/// Groups the `customers` repository behind one value that holds the pool.
#[derive(Clone)]
pub struct ClientService {
    pool: PgPool,
}

impl ClientService {
    /// Build the service around a pool (cheap: `PgPool` is reference counted).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool, for calls that still need it directly (transactions).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Same as [`customers::get`].
    pub async fn get(&self, id: Uuid) -> AppResult<Option<Customer>> {
        customers::get(&self.pool, id).await
    }

    /// Same as [`customers::by_owner`].
    pub async fn by_owner(&self, user_id: Uuid) -> AppResult<Option<Customer>> {
        customers::by_owner(&self.pool, user_id).await
    }

    /// Same as [`customers::id_for_user`].
    pub async fn id_for_user(&self, user_id: Uuid) -> AppResult<Uuid> {
        customers::id_for_user(&self.pool, user_id).await
    }

    /// Same as [`customers::create`].
    pub async fn create(&self, email: &str, p: &Profile) -> AppResult<Customer> {
        customers::create(&self.pool, email, p).await
    }

    /// Same as [`customers::update_profile`].
    pub async fn update_profile(&self, id: Uuid, p: &Profile) -> AppResult<Option<Customer>> {
        customers::update_profile(&self.pool, id, p).await
    }

    /// Same as [`customers::contacts`].
    pub async fn contacts(&self, customer_id: Uuid) -> AppResult<Vec<Contact>> {
        customers::contacts(&self.pool, customer_id).await
    }

    /// Same as [`customers::delete_contact`].
    pub async fn delete_contact(&self, customer_id: Uuid, id: Uuid) -> AppResult<bool> {
        customers::delete_contact(&self.pool, customer_id, id).await
    }

    /// Same as [`customers::update_admin`].
    pub async fn update_admin(&self, id: Uuid, status: Option<&str>, notes: Option<&str>, tax_exempt: Option<bool>) -> AppResult<Option<Customer>> {
        customers::update_admin(&self.pool, id, status, notes, tax_exempt).await
    }

    /// Same as [`customers::list`].
    pub async fn list(&self, q: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Customer>, i64)> {
        customers::list(&self.pool, q, limit, offset).await
    }

    /// Same as [`customers::add_contact`].
    pub async fn add_contact(&self, customer_id: Uuid, name: &str, email: &str, phone: &str, is_billing: bool) -> AppResult<Contact> {
        customers::add_contact(&self.pool, customer_id, name, email, phone, is_billing).await
    }

    /// Same as [`customers::update_contact`].
    pub async fn update_contact(&self, customer_id: Uuid, id: Uuid, name: &str, email: &str, phone: &str, is_billing: bool) -> AppResult<Option<Contact>> {
        customers::update_contact(&self.pool, customer_id, id, name, email, phone, is_billing).await
    }
}
