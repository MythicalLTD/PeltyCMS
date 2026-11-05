//! Mass mail campaigns: segment selection, recipient snapshot, batched
//! sending state and unsubscribe. Opted-out customers are excluded when the
//! campaign is queued and re-checked right before each send.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct Segment {
    /// Customer statuses (default: active).
    #[serde(default)]
    pub statuses: Vec<String>,
    #[serde(default)]
    pub client_group_id: Option<Uuid>,
    /// Customers with an active service of any of these products.
    #[serde(default)]
    pub product_ids: Vec<Uuid>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub with_active_service: bool,
    /// Mail merge to an explicit selection (e.g. from the clients list).
    #[serde(default)]
    pub customer_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct CampaignInput {
    pub name: String,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub segment: Segment,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Campaign {
    pub id: Uuid,
    pub name: String,
    pub subject: String,
    pub body: String,
    pub segment: serde_json::Value,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub queued_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub scheduled_at: Option<OffsetDateTime>,
    pub total: i64,
    pub sent: i64,
    pub failed: i64,
    pub skipped: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct Recipient {
    pub customer_id: Uuid,
    pub email: String,
    pub unsub_token: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub company_name: String,
    pub city: String,
    pub country: String,
    pub locale: Option<String>,
}

const STATUSES: [&str; 3] = ["active", "inactive", "closed"];

pub fn check(i: &CampaignInput) -> AppResult<()> {
    let n = |s: &str, max: usize| !s.trim().is_empty() && s.chars().count() <= max;
    if !n(&i.name, 200)
        || !n(&i.subject, 300)
        || !n(&i.body, 50000)
        || i.subject.contains(['\r', '\n'])
    {
        return Err(AppError::Validation("error.invalid_campaign"));
    }
    if i.segment
        .statuses
        .iter()
        .any(|s| !STATUSES.contains(&s.as_str()))
    {
        return Err(AppError::Validation("error.invalid_segment"));
    }
    if i.segment
        .country
        .as_deref()
        .is_some_and(|c| !(c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase())))
    {
        return Err(AppError::Validation("error.invalid_segment"));
    }
    Ok(())
}

/// WHERE clause over `customers c` for a segment; binds $1..$6 (see `bind_seg`).
const SEG_WHERE: &str = "c.marketing_opt_out = false AND c.email <> '' \
    AND c.status = ANY($1) \
    AND ($2::uuid IS NULL OR c.client_group_id = $2) \
    AND (cardinality($3::uuid[]) = 0 OR EXISTS (SELECT 1 FROM services s WHERE s.customer_id = c.id AND s.status = 'active' AND s.product_id = ANY($3))) \
    AND ($4::text IS NULL OR c.country = $4) \
    AND ($5::text IS NULL OR c.currency = $5) \
    AND (NOT $6 OR EXISTS (SELECT 1 FROM services s WHERE s.customer_id = c.id AND s.status = 'active')) \
    AND (cardinality($7::uuid[]) = 0 OR c.id = ANY($7))";

fn statuses(seg: &Segment) -> Vec<String> {
    if seg.statuses.is_empty() && !seg.customer_ids.is_empty() {
        STATUSES.iter().map(|s| s.to_string()).collect()
    } else if seg.statuses.is_empty() {
        vec!["active".into()]
    } else {
        seg.statuses.clone()
    }
}

pub async fn count_segment(pool: &PgPool, seg: &Segment) -> AppResult<i64> {
    sqlx::query_scalar(&format!(
        "SELECT count(*) FROM customers c WHERE {SEG_WHERE}"
    ))
    .bind(statuses(seg))
    .bind(seg.client_group_id)
    .bind(&seg.product_ids)
    .bind(&seg.country)
    .bind(seg.currency.as_deref().map(str::to_ascii_uppercase))
    .bind(seg.with_active_service)
    .bind(&seg.customer_ids)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

const COLS: &str = "m.id, m.name, m.subject, m.body, m.segment, m.status, m.created_at, m.queued_at, m.finished_at, m.scheduled_at, \
    (SELECT count(*) FROM mail_campaign_recipients r WHERE r.campaign_id = m.id) AS total, \
    (SELECT count(*) FROM mail_campaign_recipients r WHERE r.campaign_id = m.id AND r.status = 'sent') AS sent, \
    (SELECT count(*) FROM mail_campaign_recipients r WHERE r.campaign_id = m.id AND r.status = 'failed') AS failed, \
    (SELECT count(*) FROM mail_campaign_recipients r WHERE r.campaign_id = m.id AND r.status = 'skipped') AS skipped";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Campaign>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM mail_campaigns m ORDER BY m.created_at DESC LIMIT 200"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Campaign> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM mail_campaigns m WHERE m.id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn create(pool: &PgPool, by: Uuid, i: &CampaignInput) -> AppResult<Uuid> {
    check(i)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO mail_campaigns (id, name, subject, body, segment, created_by) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(i.name.trim())
        .bind(i.subject.trim())
        .bind(&i.body)
        .bind(serde_json::to_value(&i.segment).map_err(AppError::internal)?)
        .bind(by)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(id)
}

/// Only drafts can be edited or deleted.
pub async fn update(pool: &PgPool, id: Uuid, i: &CampaignInput) -> AppResult<()> {
    check(i)?;
    let r = sqlx::query("UPDATE mail_campaigns SET name = $2, subject = $3, body = $4, segment = $5 WHERE id = $1 AND status = 'draft'")
        .bind(id)
        .bind(i.name.trim())
        .bind(i.subject.trim())
        .bind(&i.body)
        .bind(serde_json::to_value(&i.segment).map_err(AppError::internal)?)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.campaign_not_draft"));
    }
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query(
        "DELETE FROM mail_campaigns WHERE id = $1 AND status IN ('draft','sent','cancelled')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.campaign_sending"));
    }
    Ok(())
}

/// Put a draft on hold until `at` (in the future). Returns false if not a draft.
pub async fn schedule(pool: &PgPool, id: Uuid, at: OffsetDateTime) -> AppResult<()> {
    if at <= OffsetDateTime::now_utc() {
        return Err(AppError::Validation("error.schedule_in_past"));
    }
    let r = sqlx::query("UPDATE mail_campaigns SET status = 'scheduled', scheduled_at = $2 WHERE id = $1 AND status = 'draft'")
        .bind(id).bind(at).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.campaign_not_draft"));
    }
    Ok(())
}

/// Back to draft (edit or reschedule). Only from 'scheduled'.
pub async fn unschedule(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("UPDATE mail_campaigns SET status = 'draft', scheduled_at = NULL WHERE id = $1 AND status = 'scheduled'")
        .bind(id).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.campaign_not_scheduled"));
    }
    Ok(())
}

/// Snapshot the segment into recipients and mark the campaign sending.
/// Returns the recipient count (0 → `error.campaign_empty`).
pub async fn queue(pool: &PgPool, id: Uuid) -> AppResult<i64> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let seg: Option<serde_json::Value> = sqlx::query_scalar("SELECT segment FROM mail_campaigns WHERE id = $1 AND status IN ('draft','scheduled') FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let Some(seg) = seg else {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.campaign_not_draft"));
    };
    let seg: Segment = serde_json::from_value(seg).unwrap_or_default();
    let n = sqlx::query(&format!(
        "INSERT INTO mail_campaign_recipients (campaign_id, customer_id, email, unsub_token) \
         SELECT $8, c.id, c.email, gen_random_uuid() FROM customers c WHERE {SEG_WHERE}"
    ))
    .bind(statuses(&seg))
    .bind(seg.client_group_id)
    .bind(&seg.product_ids)
    .bind(&seg.country)
    .bind(seg.currency.as_deref().map(str::to_ascii_uppercase))
    .bind(seg.with_active_service)
    .bind(&seg.customer_ids)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected() as i64;
    if n == 0 {
        return Err(AppError::Conflict("error.campaign_empty"));
    }
    sqlx::query("UPDATE mail_campaigns SET status = 'sending', queued_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(n)
}

pub async fn cancel(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("UPDATE mail_campaigns SET status = 'cancelled', finished_at = now() WHERE id = $1 AND status IN ('draft','scheduled','sending')")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        get(pool, id).await?;
        return Err(AppError::Conflict("error.campaign_finished"));
    }
    sqlx::query("UPDATE mail_campaign_recipients SET status = 'skipped' WHERE campaign_id = $1 AND status = 'pending'").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Next batch of pending recipients for a sending campaign. Recipients who
/// opted out since queueing are marked skipped here.
pub async fn next_batch(
    pool: &PgPool,
    id: Uuid,
    n: i64,
) -> AppResult<Option<(Campaign, Vec<Recipient>)>> {
    let c = get(pool, id).await?;
    if c.status != "sending" {
        return Ok(None);
    }
    sqlx::query(
        "UPDATE mail_campaign_recipients r SET status = 'skipped', error = 'opted_out' FROM customers c \
         WHERE r.campaign_id = $1 AND r.status = 'pending' AND c.id = r.customer_id AND c.marketing_opt_out",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    let rows = sqlx::query_as(
        "SELECT r.customer_id, r.email::text AS email, r.unsub_token, c.first_name, c.last_name, c.organization AS company_name, c.city, c.country, u.locale FROM mail_campaign_recipients r \
         JOIN customers c ON c.id = r.customer_id LEFT JOIN users u ON u.id = c.owner_user_id \
         WHERE r.campaign_id = $1 AND r.status = 'pending' ORDER BY r.customer_id LIMIT $2",
    )
    .bind(id)
    .bind(n)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(Some((c, rows)))
}

/// Record one send result (only from pending, so retries never double-send).
pub async fn mark(pool: &PgPool, id: Uuid, customer: Uuid, err: Option<&str>) -> AppResult<()> {
    sqlx::query(
        "UPDATE mail_campaign_recipients SET status = CASE WHEN $3::text IS NULL THEN 'sent' ELSE 'failed' END, error = $3, sent_at = now() \
         WHERE campaign_id = $1 AND customer_id = $2 AND status = 'pending'",
    )
    .bind(id)
    .bind(customer)
    .bind(err)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Close the campaign when nothing is pending. Returns true when finished.
pub async fn finish_if_done(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query(
        "UPDATE mail_campaigns SET status = 'sent', finished_at = now() WHERE id = $1 AND status = 'sending' \
         AND NOT EXISTS (SELECT 1 FROM mail_campaign_recipients WHERE campaign_id = $1 AND status = 'pending')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

/// Public unsubscribe by token: `apply=false` only looks it up (safe for
/// link scanners), `apply=true` opts the customer out. Returns the masked email.
pub async fn unsubscribe(pool: &PgPool, token: Uuid, apply: bool) -> AppResult<Option<String>> {
    let sql = if apply {
        "UPDATE customers c SET marketing_opt_out = true, updated_at = now() FROM mail_campaign_recipients r \
         WHERE r.unsub_token = $1 AND c.id = r.customer_id RETURNING c.email::text"
    } else {
        "SELECT c.email::text FROM mail_campaign_recipients r JOIN customers c ON c.id = r.customer_id WHERE r.unsub_token = $1"
    };
    let email: Option<String> = sqlx::query_scalar(sql)
        .bind(token)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(email.map(|e| mask(&e)))
}

pub fn mask(e: &str) -> String {
    match e.split_once('@') {
        Some((u, d)) => format!("{}***@{d}", u.chars().next().unwrap_or('*')),
        None => "***".into(),
    }
}

pub async fn opt_out(pool: &PgPool, customer: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT marketing_opt_out FROM customers WHERE id = $1")
        .bind(customer)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn set_opt_out(pool: &PgPool, customer: Uuid, v: bool) -> AppResult<()> {
    sqlx::query("UPDATE customers SET marketing_opt_out = $2, updated_at = now() WHERE id = $1")
        .bind(customer)
        .bind(v)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inp() -> CampaignInput {
        CampaignInput {
            name: "News".into(),
            subject: "Hello".into(),
            body: "Hi {{first_name}}".into(),
            segment: Segment::default(),
        }
    }
    #[test]
    fn validation() {
        assert!(check(&inp()).is_ok());
        assert!(check(&CampaignInput {
            subject: "a\nb".into(),
            ..inp()
        })
        .is_err());
        assert!(check(&CampaignInput {
            body: " ".into(),
            ..inp()
        })
        .is_err());
        let mut i = inp();
        i.segment.statuses = vec!["weird".into()];
        assert!(check(&i).is_err());
        let mut i = inp();
        i.segment.country = Some("ro".into());
        assert!(check(&i).is_err());
    }
    #[test]
    fn masks() {
        assert_eq!(mask("john@example.com"), "j***@example.com");
        assert_eq!(mask("bad"), "***");
    }
}
