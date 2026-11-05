//! Order approval rules and the pending-order approval queue.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const KEY: &str = "orders.approval";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ApprovalRules {
    /// Off = every order waits for staff (manual mode).
    pub auto_approve: bool,
    /// Only approve once the order invoice is paid.
    #[serde(default = "yes")]
    pub require_paid: bool,
    /// Hold orders above this total (order currency).
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>)]
    pub max_total: Option<Decimal>,
    /// Hold orders from clients with no previously approved order.
    #[serde(default)]
    pub existing_clients_only: bool,
    /// Products that always need manual approval.
    #[serde(default)]
    pub manual_product_ids: Vec<Uuid>,
    /// Hold orders flagged for fraud review.
    #[serde(default = "yes")]
    pub hold_fraud_review: bool,
}

fn yes() -> bool {
    true
}

impl Default for ApprovalRules {
    fn default() -> Self {
        Self { auto_approve: false, require_paid: true, max_total: None, existing_clients_only: false, manual_product_ids: vec![], hold_fraud_review: true }
    }
}

pub async fn rules(pool: &PgPool) -> AppResult<ApprovalRules> {
    Ok(crate::settings::get(pool, KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn set_rules(pool: &PgPool, mut r: ApprovalRules) -> AppResult<ApprovalRules> {
    if r.max_total.is_some_and(|m| m < Decimal::ZERO) || r.manual_product_ids.len() > 500 {
        return Err(AppError::Validation("error.invalid_approval_rules"));
    }
    r.manual_product_ids.sort();
    r.manual_product_ids.dedup();
    crate::settings::set(pool, KEY, serde_json::to_value(&r).map_err(AppError::internal)?).await?;
    Ok(r)
}

/// Facts about one order that the rules look at.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct Facts {
    pub total: Decimal,
    pub paid: bool,
    pub prior_approved: i64,
    pub manual_products: i64,
    pub fraud_review: bool,
}

/// Why an order is held (empty = may auto-approve).
pub fn hold_reasons(r: &ApprovalRules, f: &Facts) -> Vec<&'static str> {
    let mut v = vec![];
    if !r.auto_approve {
        v.push("manual_mode");
    }
    if r.require_paid && !f.paid {
        v.push("awaiting_payment");
    }
    if r.max_total.is_some_and(|m| f.total > m) {
        v.push("over_limit");
    }
    if r.existing_clients_only && f.prior_approved == 0 {
        v.push("new_client");
    }
    if f.manual_products > 0 {
        v.push("manual_product");
    }
    if r.hold_fraud_review && f.fraud_review {
        v.push("fraud_review");
    }
    v
}

const FACTS: &str = "SELECT o.total, COALESCE(i.status = 'paid', false) AS paid, \
    (SELECT count(*) FROM orders p WHERE p.customer_id = o.customer_id AND p.id <> o.id AND p.approved_at IS NOT NULL) AS prior_approved, \
    (SELECT count(*) FROM order_items oi WHERE oi.order_id = o.id AND oi.product_id = ANY($2)) AS manual_products, \
    EXISTS (SELECT 1 FROM fraud_checks f WHERE f.order_id = o.id AND f.verdict = 'review' AND f.decision IS NULL) AS fraud_review \
    FROM orders o LEFT JOIN invoices i ON i.id = o.invoice_id WHERE o.id = $1";

pub async fn facts(pool: &PgPool, order: Uuid, r: &ApprovalRules) -> AppResult<Option<Facts>> {
    sqlx::query_as(FACTS).bind(order).bind(&r.manual_product_ids).fetch_optional(pool).await.map_err(db_err)
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct QueueItem {
    pub id: Uuid,
    pub number: i64,
    pub customer_id: Uuid,
    pub customer_email: String,
    #[schema(value_type = String)]
    pub total: Decimal,
    pub currency: String,
    pub invoice_status: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub reasons: Vec<&'static str>,
}

type QueueRow = (Uuid, i64, Uuid, String, Decimal, String, Option<String>, OffsetDateTime);

/// Pending orders with the rule reasons holding each one.
pub async fn queue(pool: &PgPool) -> AppResult<Vec<QueueItem>> {
    let r = rules(pool).await?;
    let rows: Vec<QueueRow> = sqlx::query_as(
        "SELECT o.id, o.number, o.customer_id, c.email::text, o.total, o.currency::text, i.status, o.created_at \
         FROM orders o JOIN customers c ON c.id = o.customer_id LEFT JOIN invoices i ON i.id = o.invoice_id \
         WHERE o.status = 'pending' ORDER BY o.created_at LIMIT 500",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, number, customer_id, customer_email, total, currency, invoice_status, created_at) in rows {
        let f = facts(pool, id, &r).await?.unwrap_or_default();
        out.push(QueueItem { id, number, customer_id, customer_email, total, currency, invoice_status, created_at, reasons: hold_reasons(&r, &f) });
    }
    Ok(out)
}

/// Pending order attached to an invoice, if any.
pub async fn pending_for_invoice(pool: &PgPool, invoice: Uuid) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM orders WHERE invoice_id = $1 AND status = 'pending'").bind(invoice).fetch_optional(pool).await.map_err(db_err)
}

/// True if a pending order passes every rule right now.
pub async fn auto_ok(pool: &PgPool, order: Uuid) -> AppResult<bool> {
    let r = rules(pool).await?;
    let st: Option<String> = sqlx::query_scalar("SELECT status FROM orders WHERE id = $1").bind(order).fetch_optional(pool).await.map_err(db_err)?;
    if st.as_deref() != Some("pending") {
        return Ok(false);
    }
    Ok(facts(pool, order, &r).await?.is_some_and(|f| hold_reasons(&r, &f).is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    #[test]
    fn reasons() {
        let f = Facts { total: Decimal::from(50), paid: true, prior_approved: 1, manual_products: 0, fraud_review: false };
        assert_eq!(hold_reasons(&ApprovalRules::default(), &f), vec!["manual_mode"]);
        let auto = ApprovalRules { auto_approve: true, ..Default::default() };
        assert!(hold_reasons(&auto, &f).is_empty());
        assert_eq!(hold_reasons(&auto, &Facts { paid: false, ..f.clone() }), vec!["awaiting_payment"]);
        let lim = ApprovalRules { max_total: Some(Decimal::from_str("49.99").unwrap()), existing_clients_only: true, ..auto.clone() };
        assert_eq!(hold_reasons(&lim, &Facts { prior_approved: 0, ..f.clone() }), vec!["over_limit", "new_client"]);
        assert_eq!(hold_reasons(&auto, &Facts { fraud_review: true, manual_products: 1, ..f }), vec!["manual_product", "fraud_review"]);
    }
}
