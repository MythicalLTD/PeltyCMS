//! Product reviews and Q&A (module `product_reviews`). Customers rate a
//! product once (editing re-enters moderation), ask questions, and staff or
//! the owning marketplace creator reply. A review is marked `verified` when
//! the customer bought the product (a service, a paid order line or a license).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "product_reviews";
pub const STATUSES: &[&str] = &["pending", "approved", "rejected"];

/// Module settings (stored in `feature_modules.settings`).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Config {
    /// New reviews and questions wait for staff approval.
    pub require_approval: bool,
    /// Only customers who bought the product may review it.
    pub verified_only: bool,
    /// Product Q&A is open.
    pub questions: bool,
}

impl Config {
    pub fn from_value(v: &Value) -> Self {
        let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
        Self {
            require_approval: b("require_approval", true),
            verified_only: b("verified_only", false),
            questions: b("questions", true),
        }
    }
}

pub async fn config(pool: &PgPool) -> AppResult<Config> {
    Ok(Config::from_value(
        &crate::feature_modules::settings(pool, MODULE).await?,
    ))
}

/// Did the customer buy this product?
pub async fn purchased(pool: &PgPool, customer: Uuid, product: Uuid) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM services WHERE customer_id = $1 AND product_id = $2 AND status <> 'pending')
             OR EXISTS (SELECT 1 FROM invoices i JOIN order_items oi ON oi.order_id = i.order_id
                         WHERE i.customer_id = $1 AND oi.product_id = $2 AND i.status = 'paid')
             OR EXISTS (SELECT 1 FROM software_licenses WHERE customer_id = $1 AND product_id = $2)",
    )
    .bind(customer)
    .bind(product)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Public view of a product that can be reviewed (active, not hidden).
pub async fn product_visible(pool: &PgPool, product: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM products WHERE id = $1 AND status = 'active')")
        .bind(product)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Review {
    pub id: Uuid,
    pub product_id: Uuid,
    pub customer_id: Uuid,
    pub rating: i16,
    pub title: String,
    pub body: String,
    pub status: String,
    pub verified: bool,
    pub reply: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub replied_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub struct NewReview<'a> {
    pub rating: i16,
    pub title: &'a str,
    pub body: &'a str,
}

/// Create or replace the customer's review. Editing re-enters moderation
/// when approval is required and clears the staff reply.
pub async fn upsert(
    pool: &PgPool,
    customer: Uuid,
    product: Uuid,
    r: &NewReview<'_>,
    verified: bool,
    approved: bool,
) -> AppResult<Review> {
    if !(1..=5).contains(&r.rating)
        || r.title.chars().count() > 120
        || r.body.chars().count() > 5000
    {
        return Err(AppError::Validation("error.review_invalid"));
    }
    let status = if approved { "approved" } else { "pending" };
    sqlx::query_as(
        "INSERT INTO product_reviews (id, product_id, customer_id, rating, title, body, status, verified)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (product_id, customer_id) DO UPDATE SET
           rating = EXCLUDED.rating, title = EXCLUDED.title, body = EXCLUDED.body,
           status = EXCLUDED.status, verified = EXCLUDED.verified,
           reply = '', replied_at = NULL, updated_at = now()
         RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(product)
    .bind(customer)
    .bind(r.rating)
    .bind(r.title.trim())
    .bind(r.body.trim())
    .bind(status)
    .bind(verified)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Review> {
    sqlx::query_as("SELECT * FROM product_reviews WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn delete_own(pool: &PgPool, customer: Uuid, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM product_reviews WHERE id = $1 AND customer_id = $2")
        .bind(id)
        .bind(customer)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn mine(pool: &PgPool, customer: Uuid) -> AppResult<Vec<Review>> {
    sqlx::query_as("SELECT * FROM product_reviews WHERE customer_id = $1 ORDER BY created_at DESC")
        .bind(customer)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Approved review as shown publicly: the author is "First L.".
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct PublicReview {
    pub id: Uuid,
    pub author: String,
    pub rating: i16,
    pub title: String,
    pub body: String,
    pub verified: bool,
    pub reply: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub replied_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const AUTHOR: &str = "COALESCE(NULLIF(trim(c.first_name || ' ' || left(c.last_name, 1) || CASE WHEN c.last_name <> '' THEN '.' ELSE '' END), ''), 'Customer')";

pub async fn public(
    pool: &PgPool,
    product: Uuid,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<PublicReview>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT r.id, {AUTHOR} AS author, r.rating, r.title, r.body, r.verified, r.reply, r.replied_at, r.created_at
           FROM product_reviews r JOIN customers c ON c.id = r.customer_id
          WHERE r.product_id = $1 AND r.status = 'approved'
          ORDER BY r.verified DESC, r.created_at DESC LIMIT $2 OFFSET $3"
    )))
    .bind(product)
    .bind(limit.clamp(1, 100))
    .bind(offset.max(0))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Summary {
    pub count: i64,
    /// Average rating with two decimals, `"0"` when there are none.
    pub average: String,
    /// Count per star, index 0 = 1 star.
    pub histogram: [i64; 5],
}

pub async fn summary(pool: &PgPool, product: Uuid) -> AppResult<Summary> {
    let rows: Vec<(i16, i64)> = sqlx::query_as(
        "SELECT rating, count(*) FROM product_reviews WHERE product_id = $1 AND status = 'approved' GROUP BY rating",
    )
    .bind(product)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut histogram = [0i64; 5];
    for (r, n) in rows {
        if (1..=5).contains(&r) {
            histogram[(r - 1) as usize] = n;
        }
    }
    let count: i64 = histogram.iter().sum();
    let total: i64 = histogram
        .iter()
        .enumerate()
        .map(|(i, n)| (i as i64 + 1) * n)
        .sum();
    let average = if count == 0 {
        "0".into()
    } else {
        let hundredths = (total * 100 + count / 2) / count;
        format!("{}.{:02}", hundredths / 100, hundredths % 100)
    };
    Ok(Summary {
        count,
        average,
        histogram,
    })
}

/// Review row for moderation lists.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ReviewRow {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub review: Review,
    pub product_name: String,
    pub customer_email: String,
}

/// Moderation list. `creator` limits it to that creator's products.
pub async fn list(
    pool: &PgPool,
    status: Option<&str>,
    creator: Option<Uuid>,
) -> AppResult<Vec<ReviewRow>> {
    sqlx::query_as(
        "SELECT r.*, p.name AS product_name, c.email::text AS customer_email
           FROM product_reviews r JOIN products p ON p.id = r.product_id JOIN customers c ON c.id = r.customer_id
          WHERE ($1::text IS NULL OR r.status = $1)
            AND ($2::uuid IS NULL OR EXISTS (SELECT 1 FROM creator_products cp WHERE cp.product_id = r.product_id AND cp.creator_id = $2))
          ORDER BY (r.status = 'pending') DESC, r.created_at DESC LIMIT 500",
    )
    .bind(status)
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> AppResult<Review> {
    if !STATUSES.contains(&status) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as(
        "UPDATE product_reviews SET status = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(status)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Staff or creator reply; an empty reply removes it.
pub async fn reply(pool: &PgPool, id: Uuid, text: &str) -> AppResult<Review> {
    let text = text.trim();
    if text.chars().count() > 5000 {
        return Err(AppError::Validation("error.review_invalid"));
    }
    sqlx::query_as(
        "UPDATE product_reviews SET reply = $2, replied_at = CASE WHEN $2 = '' THEN NULL ELSE now() END WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(text)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Question {
    pub id: Uuid,
    pub product_id: Uuid,
    pub customer_id: Uuid,
    pub body: String,
    pub status: String,
    pub answer: String,
    pub answered_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub answered_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn ask(
    pool: &PgPool,
    customer: Uuid,
    product: Uuid,
    body: &str,
    approved: bool,
) -> AppResult<Question> {
    let body = body.trim();
    if !(3..=2000).contains(&body.chars().count()) {
        return Err(AppError::Validation("error.question_invalid"));
    }
    // Throttle: at most 5 open questions per customer and product.
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM product_questions WHERE customer_id = $1 AND product_id = $2 AND answer = '' AND status <> 'rejected'",
    )
    .bind(customer)
    .bind(product)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if open >= 5 {
        return Err(AppError::Conflict("error.question_limit"));
    }
    sqlx::query_as(
        "INSERT INTO product_questions (id, product_id, customer_id, body, status) VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(product)
    .bind(customer)
    .bind(body)
    .bind(if approved { "approved" } else { "pending" })
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn question(pool: &PgPool, id: Uuid) -> AppResult<Question> {
    sqlx::query_as("SELECT * FROM product_questions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct PublicQuestion {
    pub id: Uuid,
    pub author: String,
    pub body: String,
    pub answer: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub answered_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn public_questions(pool: &PgPool, product: Uuid) -> AppResult<Vec<PublicQuestion>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT q.id, {AUTHOR} AS author, q.body, q.answer, q.answered_at, q.created_at
           FROM product_questions q JOIN customers c ON c.id = q.customer_id
          WHERE q.product_id = $1 AND q.status = 'approved'
          ORDER BY (q.answer <> '') DESC, q.created_at DESC LIMIT 200"
    )))
    .bind(product)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct QuestionRow {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub question: Question,
    pub product_name: String,
    pub customer_email: String,
}

pub async fn questions(
    pool: &PgPool,
    status: Option<&str>,
    creator: Option<Uuid>,
) -> AppResult<Vec<QuestionRow>> {
    sqlx::query_as(
        "SELECT q.*, p.name AS product_name, c.email::text AS customer_email
           FROM product_questions q JOIN products p ON p.id = q.product_id JOIN customers c ON c.id = q.customer_id
          WHERE ($1::text IS NULL OR q.status = $1)
            AND ($2::uuid IS NULL OR EXISTS (SELECT 1 FROM creator_products cp WHERE cp.product_id = q.product_id AND cp.creator_id = $2))
          ORDER BY (q.answer = '' AND q.status <> 'rejected') DESC, q.created_at DESC LIMIT 500",
    )
    .bind(status)
    .bind(creator)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_question_status(pool: &PgPool, id: Uuid, status: &str) -> AppResult<Question> {
    if !STATUSES.contains(&status) {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query_as("UPDATE product_questions SET status = $2 WHERE id = $1 RETURNING *")
        .bind(id)
        .bind(status)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

/// Answer a question; answering also publishes it.
pub async fn answer(pool: &PgPool, id: Uuid, by: Uuid, text: &str) -> AppResult<Question> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 5000 {
        return Err(AppError::Validation("error.question_invalid"));
    }
    sqlx::query_as(
        "UPDATE product_questions SET answer = $2, answered_by = $3, answered_at = now(), status = 'approved' WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(text)
    .bind(by)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}
