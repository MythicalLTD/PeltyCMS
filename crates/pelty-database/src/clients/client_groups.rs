//! Client groups. A customer belongs to at most one group.
use crate::db_err;
use pelty_core::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub const COLORS: &[&str] = &[
    "slate", "red", "orange", "amber", "green", "teal", "blue", "violet", "pink",
];

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ClientGroup {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub discount_percent: Decimal,
    pub late_fee_exempt: bool,
    pub description: String,
    pub member_count: i64,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Member {
    pub id: Uuid,
    pub name: String,
    pub email: String,
}

pub struct GroupInput<'a> {
    pub name: &'a str,
    pub color: &'a str,
    pub discount_percent: Decimal,
    pub late_fee_exempt: bool,
    pub description: &'a str,
}

const COLS: &str = "g.id, g.name, g.color, g.discount_percent, g.late_fee_exempt, g.description, \
    (SELECT count(*) FROM customers c WHERE c.client_group_id = g.id) AS member_count";

pub async fn list(pool: &PgPool) -> AppResult<Vec<ClientGroup>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM client_groups g ORDER BY g.name"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<ClientGroup>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM client_groups g WHERE g.id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn create(pool: &PgPool, g: &GroupInput<'_>) -> AppResult<Uuid> {
    sqlx::query_scalar("INSERT INTO client_groups (name, color, discount_percent, late_fee_exempt, description) VALUES ($1,$2,$3,$4,$5) RETURNING id")
        .bind(g.name)
        .bind(g.color)
        .bind(g.discount_percent)
        .bind(g.late_fee_exempt)
        .bind(g.description)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, g: &GroupInput<'_>) -> AppResult<bool> {
    let r = sqlx::query("UPDATE client_groups SET name=$2, color=$3, discount_percent=$4, late_fee_exempt=$5, description=$6 WHERE id=$1")
        .bind(id)
        .bind(g.name)
        .bind(g.color)
        .bind(g.discount_percent)
        .bind(g.late_fee_exempt)
        .bind(g.description)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Members are detached (group set NULL) by the FK.
pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM client_groups WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn members(pool: &PgPool, id: Uuid) -> AppResult<Vec<Member>> {
    sqlx::query_as(
        "SELECT id, COALESCE(NULLIF(trim(first_name || ' ' || last_name), ''), organization) AS name, email::text AS email
         FROM customers WHERE client_group_id = $1 ORDER BY email",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Set (or clear with `None`) a customer's group. Returns false if no such customer.
pub async fn assign(pool: &PgPool, customer_id: Uuid, group: Option<Uuid>) -> AppResult<bool> {
    let r =
        sqlx::query("UPDATE customers SET client_group_id = $2, updated_at = now() WHERE id = $1")
            .bind(customer_id)
            .bind(group)
            .execute(pool)
            .await
            .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn customer_by_email(pool: &PgPool, email: &str) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM customers WHERE email = $1::citext ORDER BY created_at LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Group of a customer, for pricing/automation code that applies group rules.
pub async fn of_customer(pool: &PgPool, customer_id: Uuid) -> AppResult<Option<ClientGroup>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM client_groups g JOIN customers c ON c.client_group_id = g.id WHERE c.id = $1"))
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}
