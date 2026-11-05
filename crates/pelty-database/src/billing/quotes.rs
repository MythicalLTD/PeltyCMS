//! Quotes / proposals: draft → sent → accepted (becomes a finalized invoice)
//! or declined. Line math reuses the invoice rounding rules.
use crate::db_err;
use crate::invoices::{self, NewItem};
use pelty_billing::math::{self, LineInput};
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Quote {
    pub id: Uuid,
    pub number: String,
    pub customer_id: Uuid,
    pub customer_email: String,
    pub currency: String,
    pub subject: String,
    pub notes: String,
    pub status: String,
    #[schema(value_type = String)]
    pub valid_until: Date,
    pub invoice_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
    #[serde(default)]
    #[sqlx(skip)]
    #[schema(value_type = String)]
    pub subtotal: Decimal,
    #[serde(default)]
    #[sqlx(skip)]
    #[schema(value_type = String)]
    pub tax_total: Decimal,
    #[serde(default)]
    #[sqlx(skip)]
    #[schema(value_type = String)]
    pub total: Decimal,
    #[serde(default)]
    #[sqlx(skip)]
    pub items: Vec<QuoteItem>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct QuoteItem {
    pub description: String,
    #[schema(value_type = String)]
    pub quantity: Decimal,
    #[schema(value_type = String)]
    pub unit_price: Decimal,
    #[serde(default)]
    #[schema(value_type = String)]
    pub tax_rate: Decimal,
    #[serde(default, skip_deserializing)]
    #[sqlx(default)]
    #[schema(value_type = String)]
    pub amount: Decimal,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuoteInput {
    pub customer_id: Uuid,
    pub currency: String,
    pub subject: String,
    #[serde(default)]
    pub notes: String,
    #[schema(value_type = String)]
    pub valid_until: Date,
    pub items: Vec<QuoteItem>,
}

const COLS: &str = "q.id, q.number, q.customer_id, c.email::text AS customer_email, q.currency::text AS currency, q.subject, q.notes,
    q.status, q.valid_until, q.invoice_id, q.created_at";

async fn exponent(pool: &PgPool, currency: &str) -> AppResult<u32> {
    let e: Option<i16> =
        sqlx::query_scalar("SELECT exponent FROM currencies WHERE code = $1 AND enabled")
            .bind(currency)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    e.map(|e| e.max(0) as u32)
        .ok_or(AppError::Validation("error.invalid_currency"))
}

async fn hydrate(pool: &PgPool, mut q: Quote) -> AppResult<Quote> {
    let exp = exponent(pool, &q.currency).await?;
    let mut items: Vec<QuoteItem> = sqlx::query_as(
        "SELECT description, quantity, unit_price, tax_rate FROM quote_items WHERE quote_id = $1 ORDER BY sort_order",
    )
    .bind(q.id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let (mut sub, mut tax) = (Decimal::ZERO, Decimal::ZERO);
    for it in &mut items {
        let l = math::line(
            &LineInput {
                quantity: it.quantity,
                unit_price: it.unit_price,
                tax_rate: it.tax_rate,
            },
            exp,
        )
        .map_err(|_| AppError::Validation("error.invalid_amount"))?;
        it.amount = l.amount;
        sub += l.amount;
        tax += l.tax_amount;
    }
    q.subtotal = sub;
    q.tax_total = tax;
    q.total = sub + tax;
    q.items = items;
    Ok(q)
}

pub async fn get(pool: &PgPool, id: Uuid, customer: Option<Uuid>) -> AppResult<Option<Quote>> {
    let q: Option<Quote> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM quotes q JOIN customers c ON c.id = q.customer_id
          WHERE q.id = $1 AND ($2::uuid IS NULL OR (q.customer_id = $2 AND q.status <> 'draft'))"
    ))
    .bind(id)
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match q {
        Some(q) => Ok(Some(hydrate(pool, q).await?)),
        None => Ok(None),
    }
}

pub async fn list(
    pool: &PgPool,
    customer: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Quote>, i64)> {
    let filter = "($1::uuid IS NULL OR (q.customer_id = $1 AND q.status <> 'draft'))";
    let rows: Vec<Quote> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM quotes q JOIN customers c ON c.id = q.customer_id WHERE {filter}
          ORDER BY q.created_at DESC LIMIT $2 OFFSET $3"
    ))
    .bind(customer)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM quotes q WHERE {filter}"))
        .bind(customer)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let mut out = Vec::with_capacity(rows.len());
    for q in rows {
        out.push(hydrate(pool, q).await?);
    }
    Ok((out, total))
}

fn validate(b: &QuoteInput, exp: u32) -> AppResult<()> {
    if b.subject.trim().is_empty() || b.subject.len() > 200 || b.notes.len() > 10_000 {
        return Err(AppError::Validation("error.validation"));
    }
    if b.items.is_empty() || b.items.len() > 100 {
        return Err(AppError::Validation("error.quote.items"));
    }
    for it in &b.items {
        if it.description.trim().is_empty() || it.description.len() > 500 {
            return Err(AppError::Validation("error.validation"));
        }
        if it.unit_price.normalize().scale() > exp.max(4) {
            return Err(AppError::Validation("error.invalid_amount"));
        }
        math::line(
            &LineInput {
                quantity: it.quantity,
                unit_price: it.unit_price,
                tax_rate: it.tax_rate,
            },
            exp,
        )
        .map_err(|_| AppError::Validation("error.invalid_amount"))?;
    }
    Ok(())
}

/// Create (id = None) or replace a draft quote.
pub async fn save(pool: &PgPool, id: Option<Uuid>, b: &QuoteInput) -> AppResult<Uuid> {
    let currency = b.currency.to_uppercase();
    let exp = exponent(pool, &currency).await?;
    validate(b, exp)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = match id {
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO quotes (id, number, customer_id, currency, subject, notes, valid_until)
                 VALUES ($1, 'Q-' || lpad(nextval('quote_number_seq')::text, 6, '0'), $2, $3, $4, $5, $6)",
            )
            .bind(id)
            .bind(b.customer_id)
            .bind(&currency)
            .bind(b.subject.trim())
            .bind(&b.notes)
            .bind(b.valid_until)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            id
        }
        Some(id) => {
            let r = sqlx::query(
                "UPDATE quotes SET customer_id = $2, currency = $3, subject = $4, notes = $5, valid_until = $6
                  WHERE id = $1 AND status = 'draft'",
            )
            .bind(id)
            .bind(b.customer_id)
            .bind(&currency)
            .bind(b.subject.trim())
            .bind(&b.notes)
            .bind(b.valid_until)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            if r.rows_affected() == 0 {
                return Err(AppError::Conflict("error.quote.not_draft"));
            }
            sqlx::query("DELETE FROM quote_items WHERE quote_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            id
        }
    };
    for (i, it) in b.items.iter().enumerate() {
        sqlx::query(
            "INSERT INTO quote_items (id, quote_id, description, quantity, unit_price, tax_rate, sort_order) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(it.description.trim())
        .bind(it.quantity)
        .bind(it.unit_price)
        .bind(it.tax_rate)
        .bind(i as i32)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

pub async fn send(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query(
        "UPDATE quotes SET status = 'sent', sent_at = now() WHERE id = $1 AND status = 'draft'",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.quote.not_draft"));
    }
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM quotes WHERE id = $1 AND status = 'draft'")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.quote.not_draft"));
    }
    Ok(())
}

/// Customer decision. Accept creates a finalized invoice inside the same
/// transaction; the quote row lock makes double-accept impossible.
pub async fn decide(
    pool: &PgPool,
    id: Uuid,
    customer: Uuid,
    accept: bool,
) -> AppResult<Option<Uuid>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(String, String, Date)> = sqlx::query_as(
        "SELECT status, currency::text, valid_until FROM quotes WHERE id = $1 AND customer_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (status, currency, valid_until) = row.ok_or(AppError::NotFound)?;
    if status != "sent" {
        return Err(if status == "draft" {
            AppError::NotFound
        } else {
            AppError::Conflict("error.quote.decided")
        });
    }
    if !accept {
        sqlx::query("UPDATE quotes SET status = 'declined', decided_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        return Ok(None);
    }
    if valid_until < OffsetDateTime::now_utc().date() {
        return Err(AppError::Conflict("error.quote.expired"));
    }
    let items: Vec<QuoteItem> = sqlx::query_as(
        "SELECT description, quantity, unit_price, tax_rate FROM quote_items WHERE quote_id = $1 ORDER BY sort_order",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let inv = invoices::create_draft(&mut tx, customer, &currency, None).await?;
    let lines: Vec<NewItem> = items
        .into_iter()
        .map(|i| NewItem {
            description: i.description,
            quantity: i.quantity,
            unit_price: i.unit_price,
            tax_rate: i.tax_rate,
            service_id: None,
            period: None,
            auto_tax: false,
        })
        .collect();
    invoices::add_items(&mut tx, inv, &currency, &lines).await?;
    invoices::finalize(&mut tx, inv, 7).await?;
    sqlx::query(
        "UPDATE quotes SET status = 'accepted', decided_at = now(), invoice_id = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(inv)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(inv))
}
