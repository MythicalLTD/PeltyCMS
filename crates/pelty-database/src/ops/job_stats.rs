//! Read-only aggregates over the job queue for the automation dashboard.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct KindStats {
    pub kind: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_finished_at: Option<OffsetDateTime>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_result: Option<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_run_at: Option<OffsetDateTime>,
    pub running: i64,
    pub succeeded_24h: i64,
    pub failed_24h: i64,
    pub dead_24h: i64,
}

/// Stats for each requested kind (kinds without jobs get an empty row).
pub async fn for_kinds(pool: &PgPool, kinds: &[&str]) -> AppResult<Vec<KindStats>> {
    let kinds: Vec<String> = kinds.iter().map(|k| k.to_string()).collect();
    let rows = sqlx::query_as::<_, KindStats>(
        r#"SELECT k.kind,
                  l.finished_at AS last_finished_at, l.status AS last_status, l.last_error, l.result AS last_result,
                  (SELECT min(run_at) FROM jobs q WHERE q.kind = k.kind AND q.status = 'queued') AS next_run_at,
                  (SELECT count(*) FROM jobs r WHERE r.kind = k.kind AND r.status = 'running') AS running,
                  (SELECT count(*) FROM jobs c WHERE c.kind = k.kind AND c.status = 'succeeded' AND c.finished_at > now() - interval '24 hours') AS succeeded_24h,
                  (SELECT count(*) FROM jobs c WHERE c.kind = k.kind AND c.status = 'failed' AND c.created_at > now() - interval '24 hours') AS failed_24h,
                  (SELECT count(*) FROM jobs c WHERE c.kind = k.kind AND c.status = 'dead' AND c.created_at > now() - interval '24 hours') AS dead_24h
           FROM unnest($1::text[]) AS k(kind)
           LEFT JOIN LATERAL (
               SELECT finished_at, status, last_error, result FROM jobs j
               WHERE j.kind = k.kind AND j.status IN ('succeeded','failed','dead')
               ORDER BY j.finished_at DESC NULLS LAST, j.id DESC LIMIT 1
           ) l ON true"#,
    )
    .bind(&kinds)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows)
}

/// Queue totals by status (all kinds).
pub async fn totals(pool: &PgPool) -> AppResult<Vec<(String, i64)>> {
    sqlx::query_as::<_, (String, i64)>("SELECT status, count(*) FROM jobs GROUP BY status ORDER BY status").fetch_all(pool).await.map_err(db_err)
}
