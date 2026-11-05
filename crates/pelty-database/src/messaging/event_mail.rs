//! Lookups behind event-driven customer emails (`pelty_api::event_mail`).
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn user(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<Option<(String, Option<String>, Option<String>)>> {
    sqlx::query_as("SELECT email::text, display_name, locale FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// (customer, number, "total currency")
pub async fn order(pool: &PgPool, id: Uuid) -> AppResult<Option<(Uuid, i64, String)>> {
    sqlx::query_as(
        "SELECT customer_id, number, total::text || ' ' || currency FROM orders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// (customer, number, "total currency", status, due date) for real invoices only.
pub async fn invoice(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<Option<(Uuid, Option<String>, String, String, Option<String>)>> {
    sqlx::query_as("SELECT customer_id, number, total::text || ' ' || currency, status, to_char(due_at, 'YYYY-MM-DD') FROM invoices WHERE id = $1 AND kind = 'invoice'")
        .bind(id).fetch_optional(pool).await.map_err(db_err)
}

/// (customer, number, subject)
pub async fn ticket(pool: &PgPool, id: Uuid) -> AppResult<Option<(Uuid, i64, String)>> {
    sqlx::query_as("SELECT customer_id, number, subject FROM tickets WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// (customer, product name, remote id, cancel/next due date)
pub async fn service(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<Option<(Uuid, String, Option<String>, Option<String>)>> {
    sqlx::query_as("SELECT s.customer_id, p.name, s.remote_id, to_char(COALESCE(s.cancel_at, s.next_due_date), 'YYYY-MM-DD') FROM services s JOIN products p ON p.id = s.product_id WHERE s.id = $1")
        .bind(id).fetch_optional(pool).await.map_err(db_err)
}
