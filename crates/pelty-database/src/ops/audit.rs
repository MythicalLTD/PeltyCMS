//! Audit log repository: record and list admin/client actions.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub actor_id: Option<Uuid>,
    /// Joined from users (no N+1): email and display name of the actor.
    pub actor_email: Option<String>,
    pub actor_name: Option<String>,
    pub actor_avatar_version: Option<i64>,
    pub action: String,
    pub target: Option<String>,
    pub ip: Option<String>,
    pub metadata: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Append an audit entry. Never pass secrets in `metadata`.
pub async fn record(
    pool: &PgPool,
    actor: Option<Uuid>,
    action: &str,
    target: Option<&str>,
    ip: Option<&str>,
    metadata: serde_json::Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO audit_logs (actor_id, action, target, ip, metadata) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(actor)
    .bind(action)
    .bind(target)
    .bind(ip)
    .bind(metadata)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn list(pool: &PgPool, limit: i64, offset: i64) -> AppResult<(Vec<AuditEntry>, i64)> {
    let rows = sqlx::query_as(
        "SELECT a.id, a.actor_id, u.email::text AS actor_email, u.display_name AS actor_name, u.avatar_version AS actor_avatar_version, a.action, a.target, a.ip, a.metadata, a.created_at \
         FROM audit_logs a LEFT JOIN users u ON u.id = a.actor_id ORDER BY a.id DESC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = sqlx::query_scalar("SELECT count(*) FROM audit_logs")
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok((rows, total))
}

/// Escape LIKE wildcards (default escape char is `\\`) so `_` in action
/// names such as `webhook_endpoint.` matches literally.
fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Admin audit-log filters. Blank fields are ignored.
#[derive(Debug, Default, Clone)]
pub struct AuditFilter {
    /// Action prefix, e.g. `invoice.`.
    pub action: Option<String>,
    /// Actor e-mail substring.
    pub actor: Option<String>,
    pub actor_id: Option<Uuid>,
    /// Exact target (usually an id).
    pub target: Option<String>,
    /// IP prefix, e.g. `10.0.` or a full address.
    pub ip: Option<String>,
    /// Free text over action, target and metadata.
    pub q: Option<String>,
    /// `created_at >= from`.
    pub from: Option<OffsetDateTime>,
    /// `created_at < to`.
    pub to: Option<OffsetDateTime>,
    /// Only entries without an actor (system, jobs, webhooks).
    pub system: bool,
}

const SELECT: &str = "SELECT a.id, a.actor_id, u.email::text AS actor_email, u.display_name AS actor_name, u.avatar_version AS actor_avatar_version, a.action, a.target, a.ip, a.metadata, a.created_at \
     FROM audit_logs a LEFT JOIN users u ON u.id = a.actor_id";
const WHERE: &str = "WHERE ($1::text IS NULL OR a.action LIKE $1) AND ($2::text IS NULL OR u.email::text ILIKE $2) AND ($3::text IS NULL OR a.target = $3) \
     AND ($4::uuid IS NULL OR a.actor_id = $4) AND ($5::text IS NULL OR a.ip LIKE $5) \
     AND ($6::text IS NULL OR a.action ILIKE $6 OR a.target ILIKE $6 OR a.metadata::text ILIKE $6) \
     AND ($7::timestamptz IS NULL OR a.created_at >= $7) AND ($8::timestamptz IS NULL OR a.created_at < $8) AND (NOT $9 OR a.actor_id IS NULL)";

type Binds = (Option<String>, Option<String>, Option<String>, Option<Uuid>, Option<String>, Option<String>, Option<OffsetDateTime>, Option<OffsetDateTime>, bool);

fn binds(f: &AuditFilter) -> Binds {
    (
        f.action.as_deref().map(|a| format!("{}%", like_escape(a))),
        f.actor.as_deref().map(|a| format!("%{}%", like_escape(a))),
        f.target.clone(),
        f.actor_id,
        f.ip.as_deref().map(|a| format!("{}%", like_escape(a))),
        f.q.as_deref().map(|a| format!("%{}%", like_escape(a))),
        f.from,
        f.to,
        f.system,
    )
}

macro_rules! bind_all {
    ($q:expr, $b:expr) => {
        $q.bind(&$b.0).bind(&$b.1).bind(&$b.2).bind($b.3).bind(&$b.4).bind(&$b.5).bind($b.6).bind($b.7).bind($b.8)
    };
}

/// Filtered, paged audit search, newest first. Returns rows and the total match count.
pub async fn filter(pool: &PgPool, f: &AuditFilter, limit: i64, offset: i64) -> AppResult<(Vec<AuditEntry>, i64)> {
    let b = binds(f);
    let sql = format!("{SELECT} {WHERE} ORDER BY a.id DESC LIMIT $10 OFFSET $11");
    let rows = bind_all!(sqlx::query_as::<_, AuditEntry>(&sql), b).bind(limit).bind(offset).fetch_all(pool).await.map_err(db_err)?;
    let count = format!("SELECT count(*) FROM audit_logs a LEFT JOIN users u ON u.id = a.actor_id {WHERE}");
    let total = bind_all!(sqlx::query_scalar::<_, i64>(&count), b).fetch_one(pool).await.map_err(db_err)?;
    Ok((rows, total))
}

/// Distinct action names with counts (for the filter picker), most frequent first.
pub async fn action_counts(pool: &PgPool, limit: i64) -> AppResult<Vec<(String, i64)>> {
    sqlx::query_as("SELECT action, count(*) FROM audit_logs GROUP BY action ORDER BY count(*) DESC, action LIMIT $1")
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Filtered audit search: action prefix (e.g. `invoice.`), actor email substring, target exact.
pub async fn search(pool: &PgPool, action: Option<&str>, actor: Option<&str>, target: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<AuditEntry>, i64)> {
    let f = AuditFilter { action: action.map(Into::into), actor: actor.map(Into::into), target: target.map(Into::into), ..Default::default() };
    filter(pool, &f, limit, offset).await
}
