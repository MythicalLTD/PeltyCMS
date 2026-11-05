//! `TicketService`: Tickets, their messages and support departments.
//!
//! An object-style entry point that owns the pool and groups the `tickets` repository
//! functions as methods. Each method delegates to the free function of the same name,
//! so behaviour is identical and old callers (`pelty_database::tickets::...`) keep working.
//! Covered by the same integration tests as `tickets`.

use super::tickets;
use super::tickets::*;
#[allow(unused_imports)]
use pelty_core::AppResult;
use sqlx::PgPool;
#[allow(unused_imports)]
use uuid::Uuid;

/// Groups the `tickets` repository behind one value that holds the pool.
#[derive(Clone)]
pub struct TicketService {
    pool: PgPool,
}

impl TicketService {
    /// Build the service around a pool (cheap: `PgPool` is reference counted).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool, for calls that still need it directly (transactions).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Same as [`tickets::departments`].
    pub async fn departments(&self, only_active: bool) -> AppResult<Vec<Department>> {
        tickets::departments(&self.pool, only_active).await
    }

    /// Same as [`tickets::open`].
    pub async fn open(&self, t: NewTicket<'_>) -> AppResult<Ticket> {
        tickets::open(&self.pool, t).await
    }

    /// Same as [`tickets::get`].
    pub async fn get(&self, id: Uuid, customer: Option<Uuid>) -> AppResult<Option<Ticket>> {
        tickets::get(&self.pool, id, customer).await
    }

    /// Same as [`tickets::awaiting_staff`].
    pub async fn awaiting_staff(&self) -> AppResult<i64> {
        tickets::awaiting_staff(&self.pool).await
    }

    /// Same as [`tickets::edit_message`].
    pub async fn edit_message(&self, ticket: Uuid, message: Uuid, body: &str) -> AppResult<String> {
        tickets::edit_message(&self.pool, ticket, message, body).await
    }

    /// Same as [`tickets::delete_message`].
    pub async fn delete_message(&self, ticket: Uuid, message: Uuid) -> AppResult<()> {
        tickets::delete_message(&self.pool, ticket, message).await
    }

    /// Same as [`tickets::upsert_department`].
    pub async fn upsert_department(
        &self,
        id: Option<Uuid>,
        name: &str,
        description: &str,
        is_active: bool,
        sort_order: i32,
    ) -> AppResult<Department> {
        tickets::upsert_department(&self.pool, id, name, description, is_active, sort_order).await
    }

    /// Same as [`tickets::list`].
    pub async fn list(
        &self,
        customer: Option<Uuid>,
        status: Option<&str>,
        q: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> AppResult<(Vec<Ticket>, i64)> {
        tickets::list(&self.pool, customer, status, q, limit, offset).await
    }

    /// Same as [`tickets::messages`].
    pub async fn messages(&self, ticket: Uuid, include_internal: bool) -> AppResult<Vec<Message>> {
        tickets::messages(&self.pool, ticket, include_internal).await
    }

    /// Same as [`tickets::reply`].
    pub async fn reply(
        &self,
        ticket: Uuid,
        author: Uuid,
        is_staff: bool,
        internal: bool,
        body: &str,
        attachments: &[Uuid],
    ) -> AppResult<()> {
        tickets::reply(
            &self.pool,
            ticket,
            author,
            is_staff,
            internal,
            body,
            attachments,
        )
        .await
    }

    /// Same as [`tickets::update`].
    pub async fn update(
        &self,
        id: Uuid,
        status: Option<&str>,
        priority: Option<&str>,
        department: Option<Uuid>,
        assigned_to: Option<Option<Uuid>>,
    ) -> AppResult<()> {
        tickets::update(&self.pool, id, status, priority, department, assigned_to).await
    }
}
