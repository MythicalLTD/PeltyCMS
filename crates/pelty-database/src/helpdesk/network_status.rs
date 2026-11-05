//! Network / service status issues and their timeline updates.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const KINDS: [&str; 3] = ["outage", "degraded", "maintenance"];
pub const PRIORITIES: [&str; 4] = ["low", "medium", "high", "critical"];
pub const STATUSES: [&str; 5] = [
    "scheduled",
    "investigating",
    "identified",
    "monitoring",
    "resolved",
];

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct IssueBody {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub component: String,
    pub kind: String,
    pub priority: String,
    pub status: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub starts_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>)]
    pub ends_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateBody {
    pub status: String,
    pub message: String,
}

/// Validate an issue body (pure; unit-tested).
pub fn validate(b: &IssueBody) -> Result<(), AppError> {
    let t = b.title.trim().chars().count();
    if t == 0
        || t > 200
        || b.description.chars().count() > 20_000
        || b.component.chars().count() > 120
    {
        return Err(AppError::Validation("error.validation"));
    }
    if !KINDS.contains(&b.kind.as_str())
        || !PRIORITIES.contains(&b.priority.as_str())
        || !STATUSES.contains(&b.status.as_str())
    {
        return Err(AppError::Validation("error.validation"));
    }
    if let (Some(s), Some(e)) = (b.starts_at, b.ends_at) {
        if e < s {
            return Err(AppError::Validation("error.validation"));
        }
    }
    Ok(())
}

const ISSUE_JSON: &str = "json_build_object('id', i.id, 'title', i.title, 'description', i.description, 'component', i.component,
    'kind', i.kind, 'priority', i.priority, 'status', i.status, 'starts_at', i.starts_at, 'ends_at', i.ends_at,
    'created_at', i.created_at, 'updated_at', i.updated_at,
    'updates', COALESCE((SELECT json_agg(json_build_object('id', u.id, 'status', u.status, 'message', u.message, 'created_at', u.created_at) ORDER BY u.created_at DESC)
                         FROM network_issue_updates u WHERE u.issue_id = i.id), '[]'::json))";

/// Issues as JSON (with timeline), open first. `public` limits to open
/// issues plus those resolved in the last 14 days.
pub async fn list(pool: &PgPool, public: bool, limit: i64) -> AppResult<Value> {
    let filter = if public {
        "i.status <> 'resolved' OR i.updated_at > now() - interval '14 days'"
    } else {
        "TRUE"
    };
    let q = format!(
        "SELECT COALESCE(json_agg(x.j), '[]'::json)::jsonb FROM (
            SELECT {ISSUE_JSON} AS j FROM network_issues i WHERE {filter}
            ORDER BY (i.status = 'resolved'), i.starts_at DESC LIMIT $1) x"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(q))
        .bind(limit)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn create(pool: &PgPool, b: &IssueBody, by: Uuid) -> AppResult<Uuid> {
    validate(b)?;
    sqlx::query_scalar(
        "INSERT INTO network_issues (title, description, component, kind, priority, status, starts_at, ends_at, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,COALESCE($7, now()),$8,$9) RETURNING id",
    )
    .bind(b.title.trim()).bind(&b.description).bind(b.component.trim()).bind(&b.kind).bind(&b.priority).bind(&b.status)
    .bind(b.starts_at).bind(b.ends_at).bind(by)
    .fetch_one(pool).await.map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, b: &IssueBody) -> AppResult<()> {
    validate(b)?;
    let r = sqlx::query(
        "UPDATE network_issues SET title=$2, description=$3, component=$4, kind=$5, priority=$6, status=$7,
             starts_at=COALESCE($8, starts_at), ends_at=CASE WHEN $7 = 'resolved' THEN COALESCE($9, ends_at, now()) ELSE $9 END, updated_at=now()
         WHERE id=$1",
    )
    .bind(id).bind(b.title.trim()).bind(&b.description).bind(b.component.trim()).bind(&b.kind).bind(&b.priority).bind(&b.status)
    .bind(b.starts_at).bind(b.ends_at)
    .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn add_update(pool: &PgPool, id: Uuid, b: &UpdateBody, by: Uuid) -> AppResult<()> {
    let n = b.message.trim().chars().count();
    if n == 0 || n > 5000 || !STATUSES.contains(&b.status.as_str()) {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query(
        "UPDATE network_issues SET status=$2, updated_at=now(),
             ends_at = CASE WHEN $2 = 'resolved' THEN COALESCE(ends_at, now()) ELSE ends_at END WHERE id=$1",
    )
    .bind(id).bind(&b.status).execute(&mut *tx).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("INSERT INTO network_issue_updates (issue_id, status, message, created_by) VALUES ($1,$2,$3,$4)")
        .bind(id).bind(&b.status).bind(b.message.trim()).bind(by)
        .execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM network_issues WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn body() -> IssueBody {
        IssueBody {
            title: "EU-1 down".into(),
            description: String::new(),
            component: "EU-1".into(),
            kind: "outage".into(),
            priority: "high".into(),
            status: "investigating".into(),
            starts_at: None,
            ends_at: None,
        }
    }
    #[test]
    fn validates() {
        assert!(validate(&body()).is_ok());
        assert!(validate(&IssueBody {
            title: "  ".into(),
            ..body()
        })
        .is_err());
        assert!(validate(&IssueBody {
            kind: "boom".into(),
            ..body()
        })
        .is_err());
        assert!(validate(&IssueBody {
            status: "done".into(),
            ..body()
        })
        .is_err());
        let now = OffsetDateTime::now_utc();
        assert!(validate(&IssueBody {
            starts_at: Some(now),
            ends_at: Some(now - time::Duration::hours(1)),
            ..body()
        })
        .is_err());
    }
}
