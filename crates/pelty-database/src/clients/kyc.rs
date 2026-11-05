//! Know-your-customer: document uploads, staff review and the derived
//! customer status used to gate ordering (configurable, off by default).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const SETTINGS_KEY: &str = "kyc";
pub const KINDS: &[&str] = &["identity", "address", "company", "other"];
pub const MAX_DOCS: i64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(default)]
pub struct KycSettings {
    pub enabled: bool,
    /// Document kinds that must each have an approved document.
    pub required_kinds: Vec<String>,
    /// Block new orders (products and domains) until verified.
    pub gate_orders: bool,
    pub max_mb: u32,
}

impl Default for KycSettings {
    fn default() -> Self {
        Self { enabled: false, required_kinds: vec!["identity".into()], gate_orders: true, max_mb: 5 }
    }
}

impl KycSettings {
    pub fn validate(&self) -> AppResult<()> {
        if self.required_kinds.is_empty() || self.required_kinds.iter().any(|k| !KINDS.contains(&k.as_str())) || !(1..=20).contains(&self.max_mb) {
            return Err(AppError::Validation("error.validation"));
        }
        Ok(())
    }
}

pub async fn settings(pool: &PgPool) -> AppResult<KycSettings> {
    Ok(crate::settings::get(pool, SETTINGS_KEY).await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

pub async fn set_settings(pool: &PgPool, s: &KycSettings) -> AppResult<()> {
    s.validate()?;
    let mut s = s.clone();
    s.required_kinds.sort();
    s.required_kinds.dedup();
    crate::settings::set(pool, SETTINGS_KEY, serde_json::to_value(&s).map_err(|_| AppError::Validation("error.validation"))?).await
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Document {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub kind: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i32,
    pub status: String,
    pub review_note: String,
    pub reviewed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

const DCOLS: &str = "id, customer_id, kind, filename, content_type, size_bytes, status, review_note, reviewed_at, created_at";

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Status {
    pub kyc_status: String,
    pub kyc_note: String,
    pub kyc_reviewed_at: Option<OffsetDateTime>,
}

pub async fn status(pool: &PgPool, customer: Uuid) -> AppResult<Status> {
    sqlx::query_as("SELECT kyc_status, kyc_note, kyc_reviewed_at FROM customers WHERE id = $1").bind(customer).fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound)
}

pub async fn documents(pool: &PgPool, customer: Uuid) -> AppResult<Vec<Document>> {
    sqlx::query_as(&format!("SELECT {DCOLS} FROM kyc_documents WHERE customer_id = $1 ORDER BY created_at DESC")).bind(customer).fetch_all(pool).await.map_err(db_err)
}

/// Pure status rule: verified when every required kind has an approved doc;
/// otherwise pending if anything awaits review, rejected if something was
/// rejected, else none.
pub fn derive(required: &[String], docs: &[(String, String)]) -> &'static str {
    let has = |k: &str, st: &str| docs.iter().any(|(dk, ds)| dk == k && ds == st);
    if !required.is_empty() && required.iter().all(|k| has(k, "approved")) {
        "verified"
    } else if docs.iter().any(|(_, s)| s == "pending") {
        "pending"
    } else if docs.iter().any(|(_, s)| s == "rejected") {
        "rejected"
    } else {
        "none"
    }
}

/// Recompute and store the customer's status from its documents.
pub async fn refresh(pool: &PgPool, customer: Uuid) -> AppResult<String> {
    let cfg = settings(pool).await?;
    let docs: Vec<(String, String)> = sqlx::query_as("SELECT kind, status FROM kyc_documents WHERE customer_id = $1").bind(customer).fetch_all(pool).await.map_err(db_err)?;
    let st = derive(&cfg.required_kinds, &docs);
    sqlx::query("UPDATE customers SET kyc_status = $2, kyc_reviewed_at = CASE WHEN $2 IN ('verified','rejected') THEN now() ELSE kyc_reviewed_at END WHERE id = $1")
        .bind(customer).bind(st).execute(pool).await.map_err(db_err)?;
    Ok(st.to_string())
}

pub async fn upload(pool: &PgPool, customer: Uuid, kind: &str, filename: &str, content_type: &str, data: &[u8]) -> AppResult<Document> {
    if !KINDS.contains(&kind) {
        return Err(AppError::Validation("error.validation"));
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM kyc_documents WHERE customer_id = $1").bind(customer).fetch_one(pool).await.map_err(db_err)?;
    if n >= MAX_DOCS {
        return Err(AppError::Validation("error.kyc_too_many"));
    }
    let d: Document = sqlx::query_as(&format!("INSERT INTO kyc_documents (customer_id, kind, filename, content_type, size_bytes, data) VALUES ($1,$2,$3,$4,$5,$6) RETURNING {DCOLS}"))
        .bind(customer).bind(kind).bind(filename).bind(content_type).bind(data.len() as i32).bind(data)
        .fetch_one(pool).await.map_err(db_err)?;
    // A verified customer stays verified while new docs await review.
    let cur = status(pool, customer).await?.kyc_status;
    if cur != "verified" {
        refresh(pool, customer).await?;
    }
    Ok(d)
}

/// Customers may withdraw only documents not yet reviewed.
pub async fn delete_pending(pool: &PgPool, customer: Uuid, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM kyc_documents WHERE id = $1 AND customer_id = $2 AND status = 'pending'").bind(id).bind(customer).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    let cur = status(pool, customer).await?.kyc_status;
    if cur != "verified" {
        refresh(pool, customer).await?;
    }
    Ok(())
}

pub async fn file(pool: &PgPool, id: Uuid) -> AppResult<Option<(Uuid, String, String, Vec<u8>)>> {
    sqlx::query_as("SELECT customer_id, filename, content_type, data FROM kyc_documents WHERE id = $1").bind(id).fetch_optional(pool).await.map_err(db_err)
}

/// Approve/reject one document; returns (customer, new status).
pub async fn review(pool: &PgPool, id: Uuid, approve: bool, note: &str, by: Uuid) -> AppResult<(Uuid, String)> {
    if note.len() > 1000 || (!approve && note.trim().is_empty()) {
        return Err(AppError::Validation("error.kyc_note_required"));
    }
    let cid: Uuid = sqlx::query_scalar("UPDATE kyc_documents SET status = $2, review_note = $3, reviewed_by = $4, reviewed_at = now() WHERE id = $1 RETURNING customer_id")
        .bind(id).bind(if approve { "approved" } else { "rejected" }).bind(note.trim()).bind(by)
        .fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound)?;
    if !approve {
        sqlx::query("UPDATE customers SET kyc_note = $2 WHERE id = $1").bind(cid).bind(note.trim()).execute(pool).await.map_err(db_err)?;
    }
    let st = refresh(pool, cid).await?;
    Ok((cid, st))
}

/// Staff override of the customer status.
pub async fn set_status(pool: &PgPool, customer: Uuid, st: &str, note: &str) -> AppResult<()> {
    if !["none", "pending", "verified", "rejected"].contains(&st) || note.len() > 1000 {
        return Err(AppError::Validation("error.validation"));
    }
    let r = sqlx::query("UPDATE customers SET kyc_status = $2, kyc_note = $3, kyc_reviewed_at = now() WHERE id = $1").bind(customer).bind(st).bind(note.trim()).execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct QueueRow {
    pub customer_id: Uuid,
    pub email: String,
    pub name: String,
    pub kyc_status: String,
    pub pending: i64,
    pub oldest: Option<OffsetDateTime>,
}

/// Customers with documents (filtered by status), oldest pending first.
pub async fn queue(pool: &PgPool, st: Option<&str>) -> AppResult<Vec<QueueRow>> {
    sqlx::query_as(
        "SELECT c.id AS customer_id, c.email::text AS email, trim(c.first_name || ' ' || c.last_name) AS name, c.kyc_status,
                count(*) FILTER (WHERE d.status = 'pending') AS pending, min(d.created_at) FILTER (WHERE d.status = 'pending') AS oldest
         FROM customers c JOIN kyc_documents d ON d.customer_id = c.id
         WHERE ($1::text IS NULL OR c.kyc_status = $1)
         GROUP BY c.id ORDER BY min(d.created_at) FILTER (WHERE d.status = 'pending') ASC NULLS LAST, c.email LIMIT 500",
    )
    .bind(st)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Order gate: Err(kyc_required) when enabled + gating and not verified.
pub async fn ensure_can_order(pool: &PgPool, customer: Uuid) -> AppResult<()> {
    let cfg = settings(pool).await?;
    if !cfg.enabled || !cfg.gate_orders {
        return Ok(());
    }
    if status(pool, customer).await?.kyc_status != "verified" {
        return Err(AppError::Conflict("error.kyc_required"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(k: &str, s: &str) -> (String, String) {
        (k.into(), s.into())
    }
    #[test]
    fn derive_rules() {
        let req = vec!["identity".to_string(), "address".to_string()];
        assert_eq!(derive(&req, &[]), "none");
        assert_eq!(derive(&req, &[d("identity", "pending")]), "pending");
        assert_eq!(derive(&req, &[d("identity", "approved")]), "none");
        assert_eq!(derive(&req, &[d("identity", "approved"), d("address", "rejected")]), "rejected");
        assert_eq!(derive(&req, &[d("identity", "approved"), d("address", "rejected"), d("address", "pending")]), "pending");
        assert_eq!(derive(&req, &[d("identity", "approved"), d("address", "approved"), d("other", "rejected")]), "verified");
        assert!(KycSettings { required_kinds: vec!["passport".into()], ..Default::default() }.validate().is_err());
        assert!(KycSettings::default().validate().is_ok());
    }
}
