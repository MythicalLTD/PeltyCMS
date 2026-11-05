//! Fraud screening settings, rule inputs and stored results.
use crate::{db_err, settings};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

const KEY: &str = "fraud";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FraudSettings {
    pub enabled: bool,
    /// Score (0-100) at which an order is held for manual review.
    pub review_score: u8,
    /// Score at which an order is rejected (cancelled as fraud).
    pub reject_score: u8,
    /// Orders allowed per IP in 24 h before the velocity rule fires (0 = off).
    pub max_orders_per_ip_day: u32,
    /// Orders allowed per customer in 24 h (0 = off).
    pub max_orders_per_customer_day: u32,
    /// First-order total above which the order is flagged (0 = off).
    #[schema(value_type = String)]
    pub max_first_order_total: Decimal,
    /// ISO 3166-1 alpha-2 billing countries that are always rejected.
    pub blocked_countries: Vec<String>,
    /// Score when the e-mail domain is a disposable provider (0 = off).
    pub disposable_email_score: u8,
    /// Extra disposable domains on top of the built-in list.
    pub disposable_domains: Vec<String>,
    /// Score when the edge proxy's geo country differs from the billing country (0 = off).
    pub country_mismatch_score: u8,
    /// Score for Tor traffic (geo country `T1`; 0 = off).
    pub tor_score: u8,
    /// Score when the request carries client-side proxy headers (0 = off).
    pub proxy_score: u8,
    /// Hosting/VPN ranges (CIDR) whose addresses score `vpn_score`.
    pub vpn_ranges: Vec<String>,
    /// Score for an IP inside `vpn_ranges` (0 = off).
    pub vpn_score: u8,
    /// Other customers that ordered from the same IP in 30 days before the
    /// shared-IP rule fires (0 = off).
    pub max_customers_per_ip: u32,
    /// Score for the shared-IP rule.
    pub shared_ip_score: u8,
}

impl Default for FraudSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            review_score: 50,
            reject_score: 90,
            max_orders_per_ip_day: 5,
            max_orders_per_customer_day: 5,
            max_first_order_total: Decimal::ZERO,
            blocked_countries: vec![],
            disposable_email_score: 40,
            disposable_domains: vec![],
            country_mismatch_score: 30,
            tor_score: 60,
            proxy_score: 25,
            vpn_ranges: vec![],
            vpn_score: 40,
            max_customers_per_ip: 3,
            shared_ip_score: 30,
        }
    }
}

/// A plausible lowercase domain name (`mail.example.com`).
pub fn valid_domain(d: &str) -> bool {
    d.len() <= 253
        && d.contains('.')
        && d.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

impl FraudSettings {
    /// Validates and tidies the settings. `vpn_ranges` are syntax-checked by
    /// the caller (CIDR parsing lives in the API crate).
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        if self.review_score > 100
            || self.reject_score > 100
            || self.review_score == 0
            || self.reject_score < self.review_score
        {
            return Err("error.fraud_thresholds");
        }
        let scores = [
            self.disposable_email_score,
            self.country_mismatch_score,
            self.tor_score,
            self.proxy_score,
            self.vpn_score,
            self.shared_ip_score,
        ];
        if scores.iter().any(|s| *s > 100) {
            return Err("error.fraud_thresholds");
        }
        if self.max_first_order_total < Decimal::ZERO
            || self.max_orders_per_ip_day > 10_000
            || self.max_orders_per_customer_day > 10_000
            || self.max_customers_per_ip > 10_000
        {
            return Err("error.validation");
        }
        let mut c: Vec<String> = self
            .blocked_countries
            .iter()
            .map(|x| x.trim().to_ascii_uppercase())
            .filter(|x| !x.is_empty())
            .collect();
        if c.iter()
            .any(|x| x.len() != 2 || !x.bytes().all(|b| b.is_ascii_uppercase()))
            || c.len() > 250
        {
            return Err("error.invalid_country");
        }
        c.sort();
        c.dedup();
        self.blocked_countries = c;
        let mut d: Vec<String> = self
            .disposable_domains
            .iter()
            .map(|x| x.trim().trim_start_matches('@').to_ascii_lowercase())
            .filter(|x| !x.is_empty())
            .collect();
        if d.len() > 5000 || d.iter().any(|x| !valid_domain(x)) {
            return Err("error.invalid_domain");
        }
        d.sort();
        d.dedup();
        self.disposable_domains = d;
        let mut r: Vec<String> = self
            .vpn_ranges
            .iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if r.len() > 1000 {
            return Err("error.validation");
        }
        r.dedup();
        self.vpn_ranges = r;
        Ok(self)
    }
}

pub async fn get_settings(pool: &PgPool) -> AppResult<FraudSettings> {
    Ok(settings::get(pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put_settings(pool: &PgPool, s: &FraudSettings) -> AppResult<()> {
    settings::set(
        pool,
        KEY,
        serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

/// Facts the built-in rules need about an order that was just placed.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub country: String,
    pub email: String,
    pub orders_from_ip_day: i64,
    pub orders_by_customer_day: i64,
    pub prior_orders: i64,
    /// Other customers screened from the same IP in the last 30 days.
    pub customers_from_ip_30d: i64,
}

pub async fn facts(
    pool: &PgPool,
    customer_id: Uuid,
    order_id: Uuid,
    ip: Option<&str>,
) -> AppResult<Facts> {
    let (country, email, ip_day, cust_day, prior, shared): (String, String, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT c.country, c.email::text,
                (SELECT count(*) FROM fraud_checks f WHERE $3::text IS NOT NULL AND f.ip = $3 AND f.created_at > now() - interval '1 day'),
                (SELECT count(*) FROM orders o WHERE o.customer_id = c.id AND o.created_at > now() - interval '1 day' AND o.id <> $2),
                (SELECT count(*) FROM orders o WHERE o.customer_id = c.id AND o.id <> $2 AND o.status <> 'fraud'),
                (SELECT count(DISTINCT f.customer_id) FROM fraud_checks f WHERE $3::text IS NOT NULL AND f.ip = $3 AND f.customer_id <> c.id AND f.created_at > now() - interval '30 days')
         FROM customers c WHERE c.id = $1",
    )
    .bind(customer_id).bind(order_id).bind(ip).fetch_one(pool).await.map_err(db_err)?;
    Ok(Facts {
        country,
        email,
        orders_from_ip_day: ip_day,
        orders_by_customer_day: cust_day,
        prior_orders: prior,
        customers_from_ip_30d: shared,
    })
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Check {
    pub id: Uuid,
    pub order_id: Option<Uuid>,
    pub order_number: Option<i64>,
    pub order_status: Option<String>,
    pub customer_id: Uuid,
    pub customer_email: String,
    pub ip: Option<String>,
    pub score: i16,
    pub verdict: String,
    pub reasons: Value,
    pub signals: Value,
    pub decision: Option<String>,
    pub reviewed_by: Option<Uuid>,
    pub reviewer_email: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub reviewed_at: Option<OffsetDateTime>,
    pub review_note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const CHECK_COLS: &str = "f.id, f.order_id, o.number AS order_number, o.status AS order_status, f.customer_id, c.email::text AS customer_email, f.ip, f.score, f.verdict, f.reasons, f.signals, \
    f.decision, f.reviewed_by, r.email::text AS reviewer_email, f.reviewed_at, f.review_note, f.created_at \
    FROM fraud_checks f JOIN customers c ON c.id = f.customer_id LEFT JOIN orders o ON o.id = f.order_id LEFT JOIN users r ON r.id = f.reviewed_by";

#[allow(clippy::too_many_arguments)]
pub async fn record(
    pool: &PgPool,
    order_id: Uuid,
    customer_id: Uuid,
    ip: Option<&str>,
    score: u8,
    verdict: &str,
    reasons: &Value,
    signals: &Value,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO fraud_checks (id, order_id, customer_id, ip, score, verdict, reasons, signals) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(id).bind(order_id).bind(customer_id).bind(ip).bind(i16::from(score)).bind(verdict).bind(reasons).bind(signals)
        .execute(pool).await.map_err(db_err)?;
    Ok(id)
}

/// List filter: `open` = review verdicts still awaiting a staff decision.
#[derive(Debug, Clone, Default)]
pub struct ListFilter<'a> {
    pub verdict: Option<&'a str>,
    pub open: bool,
    pub customer_id: Option<Uuid>,
    pub order_id: Option<Uuid>,
}

const WHERE: &str = "($1::text IS NULL OR f.verdict = $1) AND (NOT $2 OR (f.verdict = 'review' AND f.decision IS NULL AND o.status = 'pending')) \
    AND ($3::uuid IS NULL OR f.customer_id = $3) AND ($4::uuid IS NULL OR f.order_id = $4)";

pub async fn list(
    pool: &PgPool,
    q: &ListFilter<'_>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Check>, i64)> {
    let items = sqlx::query_as(&format!(
        "SELECT {CHECK_COLS} WHERE {WHERE} ORDER BY f.created_at DESC, f.id LIMIT $5 OFFSET $6"
    ))
    .bind(q.verdict)
    .bind(q.open)
    .bind(q.customer_id)
    .bind(q.order_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM fraud_checks f LEFT JOIN orders o ON o.id = f.order_id WHERE {WHERE}"
    ))
    .bind(q.verdict)
    .bind(q.open)
    .bind(q.customer_id)
    .bind(q.order_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((items, total))
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Check> {
    sqlx::query_as(&format!("SELECT {CHECK_COLS} WHERE f.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

/// Reviews awaiting a decision whose order is still pending.
pub async fn open_reviews(pool: &PgPool) -> AppResult<i64> {
    sqlx::query_scalar("SELECT count(*) FROM fraud_checks f JOIN orders o ON o.id = f.order_id WHERE f.verdict = 'review' AND f.decision IS NULL AND o.status = 'pending'")
        .fetch_one(pool).await.map_err(db_err)
}

/// Record a staff decision on an open review. Conflict when already decided
/// or not a review.
pub async fn decide(
    pool: &PgPool,
    id: Uuid,
    decision: &str,
    actor: Uuid,
    note: &str,
) -> AppResult<Check> {
    let n = sqlx::query("UPDATE fraud_checks SET decision = $2, reviewed_by = $3, reviewed_at = now(), review_note = $4 WHERE id = $1 AND verdict = 'review' AND decision IS NULL")
        .bind(id).bind(decision).bind(actor).bind(note)
        .execute(pool).await.map_err(db_err)?.rows_affected();
    if n == 0 {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.fraud_already_reviewed"));
    }
    get(pool, id).await
}
