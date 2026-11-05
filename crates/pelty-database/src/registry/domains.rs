//! Domains: TLD price list, orders (register/transfer), renewals and
//! expiry reminders. Registrar calls happen in the API worker; this module
//! only holds state. Prices are exact decimals.
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::{Date, Month, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Tld {
    pub tld: String,
    pub currency: String,
    pub register_price: Decimal,
    pub renew_price: Decimal,
    pub transfer_price: Decimal,
    pub min_years: i32,
    pub max_years: i32,
    pub registrar: String,
    pub active: bool,
    /// Days after expiry the domain can still be renewed (plus `grace_fee`).
    #[serde(default = "d30")]
    pub grace_days: i32,
    #[serde(default)]
    pub grace_fee: Decimal,
    /// Days after the grace period the domain can be restored (plus `redemption_fee`).
    #[serde(default = "d30")]
    pub redemption_days: i32,
    #[serde(default)]
    pub redemption_fee: Decimal,
    /// Yearly ID protection (WHOIS privacy) price; 0 = free.
    #[serde(default)]
    pub id_protection_price: Decimal,
    /// Yearly DNS management price; 0 = free.
    #[serde(default)]
    pub dns_management_price: Decimal,
    #[serde(default = "yes")]
    pub epp_required: bool,
    /// Storefront badge: '' | hot | new | sale.
    #[serde(default)]
    pub tld_group: String,
    #[serde(default)]
    pub sort_order: i32,
}

fn d30() -> i32 {
    30
}
fn yes() -> bool {
    true
}

const TLD_COLS: &str = "tld, currency::text AS currency, register_price, renew_price, transfer_price, min_years, max_years, registrar, active, \
    grace_days, grace_fee, redemption_days, redemption_fee, id_protection_price, dns_management_price, epp_required, tld_group, sort_order";

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Domain {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub name: String,
    pub tld: String,
    pub kind: String,
    pub status: String,
    pub registrar: String,
    pub years: i32,
    pub registered_on: Option<Date>,
    pub expires_on: Option<Date>,
    pub auto_renew: bool,
    pub nameservers: Vec<String>,
    pub contact: Value,
    pub privacy: bool,
    pub invoice_id: Option<Uuid>,
    pub last_error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub dns_management: bool,
    pub registrar_lock: bool,
    pub admin_notes: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub synced_at: Option<OffsetDateTime>,
    /// Registry premium name; renewals bill `premium_renew_price`.
    pub premium: bool,
    pub premium_renew_price: Option<Decimal>,
    /// Per-TLD additional fields (see `domain_pricing::tld_fields`).
    pub extra_fields: Value,
}

const COLS: &str = "id, customer_id, name, tld, kind, status, registrar, years, registered_on, expires_on, auto_renew, \
    nameservers, contact, privacy, invoice_id, last_error, created_at, dns_management, registrar_lock, admin_notes, synced_at, \
    premium, premium_renew_price, extra_fields";

pub const STATUSES: &[&str] = &[
    "pending",
    "active",
    "grace",
    "redemption",
    "expired",
    "transferring",
    "transferred_away",
    "cancelled",
    "failed",
    "fraud",
];
/// Statuses in which the owner can still renew (late fees apply after expiry).
pub const RENEWABLE: &[&str] = &["active", "grace", "redemption", "expired"];

/// Validate and split a domain name into (name, tld). The TLD is everything
/// after the first label, so `example.co.uk` → `co.uk`.
pub fn normalize(name: &str) -> AppResult<(String, String)> {
    let n = name.trim().trim_end_matches('.').to_ascii_lowercase();
    let bad = || AppError::Validation("error.invalid_domain");
    if n.len() > 253 || !n.contains('.') {
        return Err(bad());
    }
    for label in n.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(bad());
        }
    }
    let (_, tld) = n.split_once('.').ok_or_else(bad)?;
    let tld = tld.to_string();
    Ok((n, tld))
}

/// Nameservers: 0 or 2-13 valid host names, deduplicated, order kept.
pub fn normalize_ns(ns: &[String]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for s in ns {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let (h, _) = normalize(s).map_err(|_| AppError::Validation("error.invalid_nameserver"))?;
        if !out.contains(&h) {
            out.push(h);
        }
    }
    if out.len() == 1 || out.len() > 13 {
        return Err(AppError::Validation("error.nameserver_count"));
    }
    Ok(out)
}

/// `d` plus `years`, clamping Feb 29 to Feb 28.
pub fn add_years(d: Date, years: i32) -> Date {
    let y = d.year() + years;
    Date::from_calendar_date(y, d.month(), d.day())
        .unwrap_or_else(|_| Date::from_calendar_date(y, Month::February, 28).unwrap_or(d))
}

pub fn price_for(t: &Tld, kind: &str) -> Decimal {
    match kind {
        "transfer" => t.transfer_price,
        "renew" => t.renew_price,
        _ => t.register_price,
    }
}

/// Transfers include one year; registrations and renewals are per year.
pub fn order_total(t: &Tld, kind: &str, years: i32) -> Decimal {
    if kind == "transfer" {
        t.transfer_price
    } else {
        price_for(t, kind) * Decimal::from(years)
    }
}

// ---- TLD pricing ----

pub async fn tlds(pool: &PgPool, active_only: bool, currency: Option<&str>) -> AppResult<Vec<Tld>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TLD_COLS} FROM tld_pricing WHERE (NOT $1 OR active) AND ($2::text IS NULL OR currency = $2) ORDER BY sort_order, tld, currency"
    )))
    .bind(active_only)
    .bind(currency)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn tld(pool: &PgPool, tld: &str, currency: &str) -> AppResult<Option<Tld>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TLD_COLS} FROM tld_pricing WHERE tld = $1 AND currency = $2"
    )))
    .bind(tld)
    .bind(currency)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn upsert_tld(pool: &PgPool, t: &Tld) -> AppResult<Tld> {
    let tl = t.tld.trim().trim_start_matches('.').to_ascii_lowercase();
    let fees = [
        t.grace_fee,
        t.redemption_fee,
        t.id_protection_price,
        t.dns_management_price,
    ];
    if t.register_price.is_sign_negative()
        || t.renew_price.is_sign_negative()
        || t.transfer_price.is_sign_negative()
        || t.min_years < 1
        || t.max_years > 10
        || t.min_years > t.max_years
        || fees.iter().any(|f| f.is_sign_negative())
    {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO tld_pricing (tld, currency, register_price, renew_price, transfer_price, min_years, max_years, registrar, active,
           grace_days, grace_fee, redemption_days, redemption_fee, id_protection_price, dns_management_price, epp_required, tld_group, sort_order)
         VALUES ($1, upper($2), $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
         ON CONFLICT (tld, currency) DO UPDATE SET register_price = EXCLUDED.register_price, renew_price = EXCLUDED.renew_price,
           transfer_price = EXCLUDED.transfer_price, min_years = EXCLUDED.min_years, max_years = EXCLUDED.max_years,
           registrar = EXCLUDED.registrar, active = EXCLUDED.active, grace_days = EXCLUDED.grace_days, grace_fee = EXCLUDED.grace_fee,
           redemption_days = EXCLUDED.redemption_days, redemption_fee = EXCLUDED.redemption_fee, id_protection_price = EXCLUDED.id_protection_price,
           dns_management_price = EXCLUDED.dns_management_price, epp_required = EXCLUDED.epp_required, tld_group = EXCLUDED.tld_group,
           sort_order = EXCLUDED.sort_order
         RETURNING {TLD_COLS}"
    )))
    .bind(&tl)
    .bind(&t.currency)
    .bind(t.register_price)
    .bind(t.renew_price)
    .bind(t.transfer_price)
    .bind(t.min_years)
    .bind(t.max_years)
    .bind(&t.registrar)
    .bind(t.active)
    .bind(t.grace_days)
    .bind(t.grace_fee)
    .bind(t.redemption_days)
    .bind(t.redemption_fee)
    .bind(t.id_protection_price)
    .bind(t.dns_management_price)
    .bind(t.epp_required)
    .bind(&t.tld_group)
    .bind(t.sort_order)
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_check_violation() || d.is_foreign_key_violation() => AppError::Validation("error.validation"),
        _ => db_err(e),
    })
}

pub async fn delete_tld(pool: &PgPool, tld: &str, currency: &str) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM tld_pricing WHERE tld = $1 AND currency = $2")
        .bind(tld)
        .bind(currency)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ---- Domains ----

pub struct NewOrder<'a> {
    pub customer_id: Uuid,
    pub name: &'a str,
    pub tld: &'a Tld,
    pub kind: &'a str,
    pub years: i32,
    pub nameservers: &'a [String],
    pub contact: &'a Value,
    pub privacy: bool,
    pub dns_management: bool,
    pub auth_code_sealed: Option<Vec<u8>>,
    /// Promo code (coupons with `applies_domains`).
    pub coupon: Option<&'a str>,
    /// Claim the product's free domain via this service or pending order item.
    pub free_service: Option<Uuid>,
    pub free_order_item: Option<Uuid>,
    /// Premium name: `tld` prices already hold the premium sell price; this
    /// is the yearly renewal sell price stored on the domain.
    pub premium_renew: Option<Decimal>,
    /// Validated per-TLD additional fields.
    pub extra_fields: Value,
}

/// Product free-domain rule reached through a service or an order item the
/// customer owns: (rule, eligible TLDs, eligible cycles, item cycle).
async fn free_rule(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    customer: Uuid,
    service: Option<Uuid>,
    item: Option<Uuid>,
) -> AppResult<Option<(String, Vec<String>, Vec<String>, String)>> {
    let row = if let Some(sid) = service {
        sqlx::query_as("SELECT p.free_domain, p.free_domain_tlds, p.free_domain_cycles, s.cycle FROM services s JOIN products p ON p.id = s.product_id
                        WHERE s.id = $1 AND s.customer_id = $2 AND s.status IN ('pending','active')")
            .bind(sid).bind(customer).fetch_optional(&mut **tx).await.map_err(db_err)?
    } else if let Some(iid) = item {
        sqlx::query_as("SELECT p.free_domain, p.free_domain_tlds, p.free_domain_cycles, oi.cycle FROM order_items oi JOIN orders o ON o.id = oi.order_id JOIN products p ON p.id = oi.product_id
                        WHERE oi.id = $1 AND o.customer_id = $2 AND o.status IN ('pending','active','completed')")
            .bind(iid).bind(customer).fetch_optional(&mut **tx).await.map_err(db_err)?
    } else {
        None
    };
    Ok(row)
}

/// Whether a product's free-domain rule covers this TLD and billing cycle.
pub fn free_domain_eligible(
    rule: &str,
    tlds: &[String],
    cycles: &[String],
    tld: &str,
    cycle: &str,
) -> bool {
    rule != "none"
        && (tlds.is_empty() || tlds.iter().any(|t| t.trim_start_matches('.') == tld))
        && (cycles.is_empty() || cycles.iter().any(|c| c == cycle))
}

/// Create a pending domain plus its finalized invoice in one transaction.
pub async fn create_order(
    pool: &PgPool,
    o: NewOrder<'_>,
) -> AppResult<(Domain, invoices::Invoice)> {
    if o.years < o.tld.min_years || o.years > o.tld.max_years {
        return Err(AppError::Validation("error.invalid_years"));
    }
    let years = if o.kind == "transfer" { 1 } else { o.years };
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO domains (id, customer_id, name, tld, kind, status, registrar, years, nameservers, contact, privacy, auth_code_sealed, dns_management,
                              premium, premium_renew_price, extra_fields)
         VALUES ($1,$2,$3,$4,$5,'pending',$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(id)
    .bind(o.customer_id)
    .bind(o.name)
    .bind(&o.tld.tld)
    .bind(o.kind)
    .bind(&o.tld.registrar)
    .bind(years)
    .bind(o.nameservers)
    .bind(o.contact)
    .bind(o.privacy)
    .bind(o.auth_code_sealed)
    .bind(o.dns_management)
    .bind(o.premium_renew.is_some())
    .bind(o.premium_renew)
    .bind(&o.extra_fields)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let inv = invoices::create_draft(&mut tx, o.customer_id, &o.tld.currency, None).await?;
    let (desc, qty, unit) = if o.kind == "transfer" {
        (
            format!("Domain transfer: {} (+1 year)", o.name),
            Decimal::ONE,
            o.tld.transfer_price,
        )
    } else {
        (
            format!("Domain registration: {} ({} yr)", o.name, years),
            Decimal::from(years),
            o.tld.register_price,
        )
    };
    let mut items = vec![NewItem {
        description: desc,
        quantity: qty,
        unit_price: unit,
        tax_rate: Decimal::ZERO,
        service_id: None,
        period: None,
        auto_tax: true,
    }];
    if o.privacy && !o.tld.id_protection_price.is_zero() {
        items.push(NewItem {
            description: format!("ID protection: {}", o.name),
            quantity: Decimal::from(years),
            unit_price: o.tld.id_protection_price,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
    }
    if o.dns_management && !o.tld.dns_management_price.is_zero() {
        items.push(NewItem {
            description: format!("DNS management: {}", o.name),
            quantity: Decimal::from(years),
            unit_price: o.tld.dns_management_price,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
    }
    let first_year = if o.kind == "transfer" {
        o.tld.transfer_price
    } else {
        o.tld.register_price
    };
    let mut free = false;
    if o.free_service.is_some() || o.free_order_item.is_some() {
        let (rule, tlds, cycles, cyc) =
            free_rule(&mut tx, o.customer_id, o.free_service, o.free_order_item)
                .await?
                .ok_or(AppError::Validation("error.free_domain_not_eligible"))?;
        if !free_domain_eligible(&rule, &tlds, &cycles, &o.tld.tld, &cyc) {
            return Err(AppError::Validation("error.free_domain_not_eligible"));
        }
        let used: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM domains WHERE (free_with_service = $1 OR free_with_order_item = $2) AND status NOT IN ('cancelled','failed'))")
            .bind(o.free_service).bind(o.free_order_item).fetch_one(&mut *tx).await.map_err(db_err)?;
        if used {
            return Err(AppError::Conflict("error.free_domain_used"));
        }
        sqlx::query(
            "UPDATE domains SET free_with_service = $2, free_with_order_item = $3 WHERE id = $1",
        )
        .bind(id)
        .bind(o.free_service)
        .bind(o.free_order_item)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if first_year > Decimal::ZERO {
            items.push(NewItem {
                description: format!("Free domain (1 yr): {}", o.name),
                quantity: Decimal::ONE,
                unit_price: -first_year,
                tax_rate: Decimal::ZERO,
                service_id: None,
                period: None,
                auto_tax: true,
            });
        }
        free = true;
    }
    if let Some(code) = o.coupon.map(str::trim).filter(|c| !c.is_empty()) {
        let c =
            crate::coupons::lock_for_order(&mut tx, code, o.customer_id, &o.tld.currency).await?;
        if !c.applies_to_tld(&o.tld.tld) {
            return Err(AppError::Validation("error.coupon_not_applicable"));
        }
        let exp = invoices::exponent(&mut tx, &o.tld.currency).await?;
        let base = first_year * qty - if free { first_year } else { Decimal::ZERO };
        let d = crate::coupons::line_discount(&c.kind, c.value, base, exp);
        if d <= Decimal::ZERO {
            return Err(AppError::Validation("error.coupon_not_applicable"));
        }
        items.push(NewItem {
            description: format!("Promo {}", c.code),
            quantity: Decimal::ONE,
            unit_price: -d,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
        crate::coupons::redeem_domain(&mut tx, &c, o.customer_id, id, d).await?;
    }
    invoices::add_items(&mut tx, inv, &o.tld.currency, &items).await?;
    let invoice = invoices::finalize(&mut tx, inv, 7).await?;
    sqlx::query("UPDATE domains SET invoice_id = $2 WHERE id = $1")
        .bind(id)
        .bind(inv)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    let d = get(pool, id).await?.ok_or(AppError::NotFound)?;
    Ok((d, invoice))
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Domain>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM domains WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn get_owned(pool: &PgPool, id: Uuid, customer_id: Uuid) -> AppResult<Domain> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM domains WHERE id = $1 AND customer_id = $2"
    )))
    .bind(id)
    .bind(customer_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// True when a live record already holds the name.
pub async fn name_taken(pool: &PgPool, name: &str) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM domains WHERE lower(name) = lower($1) AND status NOT IN ('cancelled','failed')")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok(n > 0)
}

pub async fn list(
    pool: &PgPool,
    customer: Option<Uuid>,
    status: Option<&str>,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Domain>, i64)> {
    let q = q.map(|s| format!("%{}%", s.trim().to_lowercase()));
    let filter = "($1::uuid IS NULL OR customer_id = $1) AND ($2::text IS NULL OR status = $2) AND ($3::text IS NULL OR lower(name) LIKE $3)";
    let rows = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM domains WHERE {filter} ORDER BY created_at DESC LIMIT $4 OFFSET $5"
    )))
    .bind(customer)
    .bind(status)
    .bind(&q)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM domains WHERE {filter}"
    )))
    .bind(customer)
    .bind(status)
    .bind(&q)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((rows, total))
}

pub async fn set_nameservers(pool: &PgPool, id: Uuid, ns: &[String]) -> AppResult<()> {
    sqlx::query("UPDATE domains SET nameservers = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(ns)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_contact(pool: &PgPool, id: Uuid, contact: &Value, privacy: bool) -> AppResult<()> {
    sqlx::query("UPDATE domains SET contact = $2, privacy = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(contact)
        .bind(privacy)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_auto_renew(pool: &PgPool, id: Uuid, on: bool) -> AppResult<()> {
    sqlx::query("UPDATE domains SET auto_renew = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(on)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn admin_update(
    pool: &PgPool,
    id: Uuid,
    status: Option<&str>,
    expires_on: Option<Date>,
    auto_renew: Option<bool>,
) -> AppResult<Domain> {
    if status.is_some_and(|s| !STATUSES.contains(&s)) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE domains SET status = COALESCE($2, status), expires_on = COALESCE($3, expires_on), auto_renew = COALESCE($4, auto_renew),
           updated_at = now() WHERE id = $1 RETURNING {COLS}"
    )))
    .bind(id)
    .bind(status)
    .bind(expires_on)
    .bind(auto_renew)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Pending orders whose invoice is paid, with the sealed EPP code.
pub async fn paid_pending(pool: &PgPool) -> AppResult<Vec<(Domain, Option<Vec<u8>>)>> {
    let ds: Vec<Domain> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM domains d JOIN invoices i ON i.id = d.invoice_id WHERE d.status = 'pending' AND i.status = 'paid' ORDER BY d.created_at LIMIT 100",
        COLS.split(", ").map(|c| format!("d.{}", c.trim())).collect::<Vec<_>>().join(", ")
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::with_capacity(ds.len());
    for d in ds {
        let a: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT auth_code_sealed FROM domains WHERE id = $1")
                .bind(d.id)
                .fetch_one(pool)
                .await
                .map_err(db_err)?;
        out.push((d, a));
    }
    Ok(out)
}

pub async fn mark_active(
    pool: &PgPool,
    id: Uuid,
    registered_on: Date,
    expires_on: Date,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE domains SET status = 'active', registered_on = $2, expires_on = $3, last_error = NULL, auth_code_sealed = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(registered_on)
    .bind(expires_on)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Record a registrar failure; `fatal` moves the order to `failed`.
pub async fn mark_error(pool: &PgPool, id: Uuid, err: &str, fatal: bool) -> AppResult<()> {
    sqlx::query("UPDATE domains SET last_error = left($2, 500), status = CASE WHEN $3 THEN 'failed' ELSE status END, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(err)
        .bind(fatal)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Create a renewal invoice for the domain's current term (idempotent per
/// term: returns `None` if one already exists).
pub async fn create_renewal(
    pool: &PgPool,
    d: &Domain,
    t: &Tld,
    years: i32,
) -> AppResult<Option<invoices::Invoice>> {
    let exp = d
        .expires_on
        .ok_or(AppError::Validation("error.domain_not_active"))?;
    if years < 1 || years > t.max_years {
        return Err(AppError::Validation("error.invalid_years"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let rid: Option<Uuid> = sqlx::query_scalar("INSERT INTO domain_renewals (domain_id, years, for_expiry) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING RETURNING id")
        .bind(d.id)
        .bind(years)
        .bind(exp)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let Some(rid) = rid else {
        return Ok(None);
    };
    let inv = invoices::create_draft(&mut tx, d.customer_id, &t.currency, None).await?;
    let end = add_years(exp, years);
    let renew_price = d.premium_renew_price.unwrap_or(t.renew_price);
    let mut items = vec![NewItem {
        description: format!("Domain renewal: {} ({} yr, until {end})", d.name, years),
        quantity: Decimal::from(years),
        unit_price: renew_price,
        tax_rate: Decimal::ZERO,
        service_id: None,
        period: None,
        auto_tax: true,
    }];
    if d.privacy && !t.id_protection_price.is_zero() {
        items.push(NewItem {
            description: format!("ID protection: {}", d.name),
            quantity: Decimal::from(years),
            unit_price: t.id_protection_price,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
    }
    if d.dns_management && !t.dns_management_price.is_zero() {
        items.push(NewItem {
            description: format!("DNS management: {}", d.name),
            quantity: Decimal::from(years),
            unit_price: t.dns_management_price,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
    }
    // Free domain renewing with its hosting (rule `on_renewal`, service still active).
    let free_renew: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM domains dm JOIN services s ON s.id = dm.free_with_service OR (dm.free_with_service IS NULL AND s.order_item_id = dm.free_with_order_item)
         JOIN products p ON p.id = s.product_id WHERE dm.id = $1 AND s.status = 'active' AND p.free_domain = 'on_renewal'
           AND (cardinality(p.free_domain_tlds) = 0 OR dm.tld = ANY(p.free_domain_tlds)))",
    )
    .bind(d.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if free_renew && renew_price > Decimal::ZERO {
        items.push(NewItem {
            description: format!("Free domain renewal (1 yr): {}", d.name),
            quantity: Decimal::ONE,
            unit_price: -renew_price,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
    }
    let fee = late_fee(&d.status, t);
    if !fee.is_zero() {
        let label = if d.status == "redemption" {
            "Redemption (restore) fee"
        } else {
            "Late renewal (grace) fee"
        };
        items.push(NewItem {
            description: format!("{label}: {}", d.name),
            quantity: Decimal::ONE,
            unit_price: fee,
            tax_rate: Decimal::ZERO,
            service_id: None,
            period: None,
            auto_tax: true,
        });
        sqlx::query("UPDATE domain_renewals SET late_fee = $2 WHERE id = $1")
            .bind(rid)
            .bind(fee)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    invoices::add_items(&mut tx, inv, &t.currency, &items).await?;
    let invoice = invoices::finalize(&mut tx, inv, 7).await?;
    sqlx::query("UPDATE domain_renewals SET invoice_id = $2 WHERE id = $1")
        .bind(rid)
        .bind(inv)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(invoice))
}

/// Active auto-renew domains expiring within `days` without a renewal for
/// the current term.
pub async fn due_auto_renew(pool: &PgPool, today: Date, days: i64) -> AppResult<Vec<Domain>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM domains d WHERE status = 'active' AND auto_renew AND expires_on <= $1::date + $2::int
           AND NOT EXISTS (SELECT 1 FROM domain_renewals r WHERE r.domain_id = d.id AND r.for_expiry = d.expires_on) LIMIT 500"
    )))
    .bind(today)
    .bind(days as i32)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PaidRenewal {
    pub id: Uuid,
    pub domain_id: Uuid,
    pub years: i32,
    pub for_expiry: Date,
}

pub async fn paid_renewals(pool: &PgPool) -> AppResult<Vec<PaidRenewal>> {
    sqlx::query_as(
        "SELECT r.id, r.domain_id, r.years, r.for_expiry FROM domain_renewals r JOIN invoices i ON i.id = r.invoice_id
         WHERE r.applied_at IS NULL AND i.status = 'paid' ORDER BY r.created_at LIMIT 100",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn apply_renewal(pool: &PgPool, r: &PaidRenewal, new_expiry: Date) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("UPDATE domain_renewals SET applied_at = now() WHERE id = $1")
        .bind(r.id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE domains SET expires_on = $2, status = 'active', last_error = NULL, updated_at = now() WHERE id = $1")
        .bind(r.domain_id)
        .bind(new_expiry)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

type RenewalRow = (Uuid, i32, Date, Option<Uuid>, Option<OffsetDateTime>);

#[allow(clippy::type_complexity)]
pub async fn renewals(pool: &PgPool, domain_id: Uuid) -> AppResult<Vec<Value>> {
    let rows: Vec<RenewalRow> =
        sqlx::query_as("SELECT id, years, for_expiry, invoice_id, applied_at FROM domain_renewals WHERE domain_id = $1 ORDER BY created_at DESC")
            .bind(domain_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, years, for_expiry, invoice_id, applied)| serde_json::json!({"id": id, "years": years, "for_expiry": for_expiry, "invoice_id": invoice_id, "applied": applied.is_some()}))
        .collect())
}

/// Fee added to a renewal raised while the domain is past expiry.
pub fn late_fee(status: &str, t: &Tld) -> Decimal {
    match status {
        "grace" => t.grace_fee,
        "redemption" => t.redemption_fee,
        _ => Decimal::ZERO,
    }
}

/// Lifecycle state for a domain `days_past` days after its expiry date:
/// active → grace → redemption → expired (WHMCS-style).
pub fn lapse_status(days_past: i64, grace_days: i32, redemption_days: i32) -> &'static str {
    if days_past <= 0 {
        "active"
    } else if days_past <= grace_days as i64 {
        "grace"
    } else if days_past <= (grace_days + redemption_days) as i64 {
        "redemption"
    } else {
        "expired"
    }
}

/// Move lapsed domains through grace → redemption → expired using the TLD's
/// periods (defaults 30/30 when the TLD is no longer priced). Returns how
/// many rows changed status.
pub async fn expire_overdue(pool: &PgPool, today: Date) -> AppResult<u64> {
    let r = sqlx::query(
        "WITH p AS (
           SELECT d.id, ($1::date - d.expires_on) AS past,
                  COALESCE((SELECT grace_days FROM tld_pricing t WHERE t.tld = d.tld ORDER BY currency LIMIT 1), 30) AS g,
                  COALESCE((SELECT redemption_days FROM tld_pricing t WHERE t.tld = d.tld ORDER BY currency LIMIT 1), 30) AS r
           FROM domains d WHERE d.status IN ('active','grace','redemption') AND d.expires_on < $1)
         UPDATE domains d SET status = CASE WHEN p.past <= p.g THEN 'grace' WHEN p.past <= p.g + p.r THEN 'redemption' ELSE 'expired' END,
                updated_at = now()
         FROM p WHERE p.id = d.id
           AND d.status IS DISTINCT FROM (CASE WHEN p.past <= p.g THEN 'grace' WHEN p.past <= p.g + p.r THEN 'redemption' ELSE 'expired' END)",
    )
    .bind(today)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

// ---- DNS records ----

pub const DNS_TYPES: &[&str] = &[
    "A", "AAAA", "CNAME", "MX", "TXT", "SRV", "CAA", "NS", "URL", "FRAME",
];

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, PartialEq)]
pub struct DnsRecord {
    #[serde(default = "Uuid::nil")]
    pub id: Uuid,
    pub rtype: String,
    pub host: String,
    pub value: String,
    #[serde(default = "ttl_default")]
    pub ttl: i32,
    #[serde(default)]
    pub priority: Option<i32>,
}
fn ttl_default() -> i32 {
    3600
}

/// Validate one record (type, host label, value shape, TTL, MX/SRV priority).
pub fn validate_dns(r: &DnsRecord) -> AppResult<DnsRecord> {
    let bad = || AppError::Validation("error.invalid_dns_record");
    let rtype = r.rtype.trim().to_ascii_uppercase();
    if !DNS_TYPES.contains(&rtype.as_str()) {
        return Err(bad());
    }
    let host = r.host.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = if host.is_empty() {
        "@".to_string()
    } else {
        host
    };
    let host_ok = host == "@"
        || host.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && (l == "*"
                    || l.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
        });
    if !host_ok || host.len() > 253 {
        return Err(bad());
    }
    let value = r.value.trim().to_string();
    if value.is_empty() || value.len() > 2048 || value.chars().any(|c| c.is_control()) {
        return Err(bad());
    }
    let ok = match rtype.as_str() {
        "A" => value.parse::<std::net::Ipv4Addr>().is_ok(),
        "AAAA" => value.parse::<std::net::Ipv6Addr>().is_ok(),
        "CNAME" | "NS" | "MX" => value.contains('.') && !value.contains(' '),
        "URL" | "FRAME" => value.starts_with("https://") || value.starts_with("http://"),
        _ => true,
    };
    if !ok || !(60..=604_800).contains(&r.ttl) {
        return Err(bad());
    }
    let needs_prio = matches!(rtype.as_str(), "MX" | "SRV");
    let priority = if needs_prio {
        Some(r.priority.unwrap_or(10))
    } else {
        None
    };
    if priority.is_some_and(|p| !(0..=65_535).contains(&p)) {
        return Err(bad());
    }
    Ok(DnsRecord {
        id: r.id,
        rtype,
        host,
        value,
        ttl: r.ttl,
        priority,
    })
}

pub async fn dns_records(pool: &PgPool, domain_id: Uuid) -> AppResult<Vec<DnsRecord>> {
    sqlx::query_as("SELECT id, rtype, host, value, ttl, priority FROM domain_dns_records WHERE domain_id = $1 ORDER BY rtype, host, created_at")
        .bind(domain_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Replace the whole zone atomically (max 200 records).
pub async fn replace_dns(
    pool: &PgPool,
    domain_id: Uuid,
    records: &[DnsRecord],
) -> AppResult<Vec<DnsRecord>> {
    if records.len() > 200 {
        return Err(AppError::Validation("error.too_many_records"));
    }
    let clean: Vec<DnsRecord> = records.iter().map(validate_dns).collect::<AppResult<_>>()?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM domain_dns_records WHERE domain_id = $1")
        .bind(domain_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for r in &clean {
        sqlx::query("INSERT INTO domain_dns_records (domain_id, rtype, host, value, ttl, priority) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(domain_id)
            .bind(&r.rtype)
            .bind(&r.host)
            .bind(&r.value)
            .bind(r.ttl)
            .bind(r.priority)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    dns_records(pool, domain_id).await
}

pub async fn set_flags(
    pool: &PgPool,
    id: Uuid,
    dns_management: Option<bool>,
    registrar_lock: Option<bool>,
    privacy: Option<bool>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE domains SET dns_management = COALESCE($2, dns_management), registrar_lock = COALESCE($3, registrar_lock),
           privacy = COALESCE($4, privacy), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(dns_management)
    .bind(registrar_lock)
    .bind(privacy)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn set_admin_notes(pool: &PgPool, id: Uuid, notes: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE domains SET admin_notes = left($2, 5000), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(notes)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Record a registrar sync result.
pub async fn mark_synced(
    pool: &PgPool,
    id: Uuid,
    expires_on: Date,
    status: Option<&str>,
    err: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE domains SET expires_on = $2, status = COALESCE($3, status), last_error = $4, synced_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(expires_on)
    .bind(status)
    .bind(err)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Live domains least recently synced (for the daily registrar sync).
pub async fn sync_batch(pool: &PgPool, limit: i64) -> AppResult<Vec<Domain>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM domains WHERE status IN ('active','grace','redemption','transferring')
           AND (synced_at IS NULL OR synced_at < now() - interval '20 hours') ORDER BY synced_at NULLS FIRST LIMIT $1"
    )))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Bulk TLD price adjustment by `percent` (e.g. 10 = +10%), rounded to 2 dp.
/// Only the given TLDs (all when empty) and currency (all when None).
pub async fn bulk_adjust_prices(
    pool: &PgPool,
    tlds: &[String],
    currency: Option<&str>,
    percent: Decimal,
) -> AppResult<u64> {
    if percent <= Decimal::from(-100) || percent > Decimal::from(1000) {
        return Err(AppError::Validation("error.validation"));
    }
    let f = Decimal::ONE + percent / Decimal::from(100);
    let r = sqlx::query(
        "UPDATE tld_pricing SET register_price = round(register_price * $1, 2), renew_price = round(renew_price * $1, 2),
           transfer_price = round(transfer_price * $1, 2)
         WHERE (cardinality($2::text[]) = 0 OR tld = ANY($2)) AND ($3::text IS NULL OR currency = upper($3))",
    )
    .bind(f)
    .bind(tlds)
    .bind(currency)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Copy every TLD price from `from` currency into `to` at `rate`, keeping
/// existing rows for `to` unless `overwrite`.
pub async fn copy_currency(
    pool: &PgPool,
    from: &str,
    to: &str,
    rate: Decimal,
    overwrite: bool,
) -> AppResult<u64> {
    if rate <= Decimal::ZERO {
        return Err(AppError::Validation("error.validation"));
    }
    let r = sqlx::query(
        "INSERT INTO tld_pricing (tld, currency, register_price, renew_price, transfer_price, min_years, max_years, registrar, active,
           grace_days, grace_fee, redemption_days, redemption_fee, id_protection_price, dns_management_price, epp_required, tld_group, sort_order)
         SELECT tld, upper($2), round(register_price*$3,2), round(renew_price*$3,2), round(transfer_price*$3,2), min_years, max_years, registrar, active,
           grace_days, round(grace_fee*$3,2), redemption_days, round(redemption_fee*$3,2), round(id_protection_price*$3,2), round(dns_management_price*$3,2),
           epp_required, tld_group, sort_order
         FROM tld_pricing WHERE currency = upper($1)
         ON CONFLICT (tld, currency) DO UPDATE SET register_price = EXCLUDED.register_price, renew_price = EXCLUDED.renew_price,
           transfer_price = EXCLUDED.transfer_price, grace_fee = EXCLUDED.grace_fee, redemption_fee = EXCLUDED.redemption_fee,
           id_protection_price = EXCLUDED.id_protection_price, dns_management_price = EXCLUDED.dns_management_price
         WHERE $4",
    )
    .bind(from)
    .bind(to)
    .bind(rate)
    .bind(overwrite)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Smallest reminder threshold covering `days_left` (thresholds ascending
/// or not; `None` if outside all of them or already expired).
pub fn threshold(days_left: i64, thresholds: &[i64]) -> Option<i64> {
    if days_left < 0 {
        return None;
    }
    thresholds.iter().copied().filter(|t| days_left <= *t).min()
}

/// Claim a reminder slot; true the first time only.
pub async fn claim_reminder(pool: &PgPool, d: &Domain, days_before: i64) -> AppResult<bool> {
    let Some(exp) = d.expires_on else {
        return Ok(false);
    };
    let r = sqlx::query("INSERT INTO domain_reminders (domain_id, for_expiry, days_before) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(d.id)
        .bind(exp)
        .bind(days_before as i32)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn expiring(pool: &PgPool, today: Date, within: i64) -> AppResult<Vec<Domain>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLS} FROM domains WHERE status = 'active' AND expires_on >= $1 AND expires_on <= $1::date + $2::int")))
        .bind(today)
        .bind(within as i32)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;
    #[test]
    fn names() {
        assert_eq!(
            normalize(" Example.CO.uk. ").unwrap(),
            ("example.co.uk".into(), "co.uk".into())
        );
        assert!(normalize("nodot").is_err());
        assert!(normalize("-bad.com").is_err());
        assert!(normalize("a..com").is_err());
        assert!(normalize("sp ace.com").is_err());
        assert!(normalize_ns(&["ns1.x.io".into()]).is_err());
        assert_eq!(
            normalize_ns(&["NS1.x.io".into(), "ns1.x.io".into(), "ns2.x.io".into()])
                .unwrap()
                .len(),
            2
        );
        assert!(normalize_ns(&[]).unwrap().is_empty());
    }
    #[test]
    fn lapse_and_dns() {
        assert_eq!(lapse_status(0, 30, 30), "active");
        assert_eq!(lapse_status(1, 30, 30), "grace");
        assert_eq!(lapse_status(31, 30, 30), "redemption");
        assert_eq!(lapse_status(61, 30, 30), "expired");
        assert_eq!(lapse_status(5, 0, 30), "redemption");
        let r = |t: &str, h: &str, v: &str| DnsRecord {
            id: Uuid::nil(),
            rtype: t.into(),
            host: h.into(),
            value: v.into(),
            ttl: 3600,
            priority: None,
        };
        assert_eq!(validate_dns(&r("a", "", "1.2.3.4")).unwrap().host, "@");
        assert!(validate_dns(&r("A", "www", "1.2.3")).is_err());
        assert!(validate_dns(&r("AAAA", "v6", "2001:db8::1")).is_ok());
        assert_eq!(
            validate_dns(&r("MX", "@", "mx.example.com"))
                .unwrap()
                .priority,
            Some(10)
        );
        assert!(validate_dns(&r("TXT", "_dmarc", "v=DMARC1; p=none")).is_ok());
        assert!(validate_dns(&r("URL", "go", "javascript:alert(1)")).is_err());
        assert!(validate_dns(&r("BOGUS", "@", "x")).is_err());
        assert!(validate_dns(&r("A", "bad host", "1.1.1.1")).is_err());
        assert!(validate_dns(&r("TXT", "@", "a\nb")).is_err());
    }
    #[test]
    fn dates_and_thresholds() {
        assert_eq!(add_years(date!(2024 - 02 - 29), 1), date!(2025 - 02 - 28));
        assert_eq!(add_years(date!(2026 - 10 - 29), 2), date!(2028 - 10 - 29));
        assert_eq!(threshold(5, &[30, 7, 1]), Some(7));
        assert_eq!(threshold(0, &[30, 7, 1]), Some(1));
        assert_eq!(threshold(40, &[30, 7, 1]), None);
        assert_eq!(threshold(-1, &[30, 7, 1]), None);
    }
}
