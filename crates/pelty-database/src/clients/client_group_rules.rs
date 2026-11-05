//! Automatic client-group assignment rules (first match by priority wins).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

pub const KINDS: &[&str] = &[
    "country",
    "email_domain",
    "active_services_min",
    "account_age_days_min",
];

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Rule {
    pub id: Uuid,
    pub group_id: Uuid,
    pub priority: i32,
    pub kind: String,
    pub value: String,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Normalise and validate a rule value for its kind.
pub fn normalize(kind: &str, value: &str) -> AppResult<String> {
    let list = |f: &dyn Fn(&str) -> Option<String>| -> AppResult<String> {
        let items: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(f)
            .collect::<Option<_>>()
            .ok_or(AppError::Validation("error.invalid_rule_value"))?;
        if items.is_empty() || items.len() > 100 {
            return Err(AppError::Validation("error.invalid_rule_value"));
        }
        Ok(items.join(","))
    };
    match kind {
        "country" => list(&|s| {
            let s = s.to_ascii_uppercase();
            (s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())).then_some(s)
        }),
        "email_domain" => list(&|s| {
            let s = s.trim_start_matches('@').to_ascii_lowercase();
            (s.contains('.')
                && s.len() <= 253
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
            .then_some(s)
        }),
        "active_services_min" | "account_age_days_min" => value
            .trim()
            .parse::<u32>()
            .map(|n| n.to_string())
            .map_err(|_| AppError::Validation("error.invalid_rule_value")),
        _ => Err(AppError::Validation("error.invalid_rule_kind")),
    }
}

const COLS: &str = "id, group_id, priority, kind, value, enabled, created_at";

pub async fn list(pool: &PgPool) -> AppResult<Vec<Rule>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM client_group_rules ORDER BY priority, created_at"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create(
    pool: &PgPool,
    group_id: Uuid,
    priority: i32,
    kind: &str,
    value: &str,
    enabled: bool,
) -> AppResult<Rule> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("INSERT INTO client_group_rules (group_id, priority, kind, value, enabled) VALUES ($1,$2,$3,$4,$5) RETURNING {COLS}")))
        .bind(group_id)
        .bind(priority)
        .bind(kind)
        .bind(value)
        .bind(enabled)
        .fetch_one(pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => AppError::Validation("error.invalid_group"),
            _ => db_err(e),
        })
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    group_id: Uuid,
    priority: i32,
    kind: &str,
    value: &str,
    enabled: bool,
) -> AppResult<Option<Rule>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE client_group_rules SET group_id=$2, priority=$3, kind=$4, value=$5, enabled=$6 WHERE id=$1 RETURNING {COLS}")))
        .bind(id)
        .bind(group_id)
        .bind(priority)
        .bind(kind)
        .bind(value)
        .bind(enabled)
        .fetch_optional(pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => AppError::Validation("error.invalid_group"),
            _ => db_err(e),
        })
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    Ok(sqlx::query("DELETE FROM client_group_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected()
        > 0)
}

/// SQL predicate on customer alias `c` for one rule; `$1` is the rule value.
fn predicate(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "country" => "c.country = ANY (string_to_array($1, ','))",
        "email_domain" => "lower(split_part(c.email::text, '@', 2)) = ANY (string_to_array($1, ','))",
        "active_services_min" => "(SELECT count(*) FROM services s WHERE s.customer_id = c.id AND s.status = 'active') >= $1::int",
        "account_age_days_min" => "c.created_at <= now() - make_interval(days => $1::int)",
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub customer_id: Uuid,
    pub from: Option<Uuid>,
    pub to: Uuid,
    pub rule_id: Uuid,
}

/// Compute (and unless `dry_run`, apply) group changes. Closed customers are
/// skipped. Without `overwrite` only customers with no group are touched.
pub async fn apply(pool: &PgPool, dry_run: bool, overwrite: bool) -> AppResult<Vec<Change>> {
    let rules: Vec<Rule> = list(pool)
        .await?
        .into_iter()
        .filter(|r| r.enabled)
        .collect();
    let mut decided: std::collections::HashMap<Uuid, (Option<Uuid>, Uuid, Uuid)> =
        Default::default();
    for r in &rules {
        let Some(p) = predicate(&r.kind) else {
            continue;
        };
        let rows: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT c.id, c.client_group_id FROM customers c WHERE c.status <> 'closed' AND c.merged_into IS NULL AND ({p}) {}",
            if overwrite { "" } else { "AND c.client_group_id IS NULL" }
        )))
        .bind(&r.value)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        for (id, cur) in rows {
            decided.entry(id).or_insert((cur, r.group_id, r.id));
        }
    }
    let mut out: Vec<Change> = decided
        .into_iter()
        .filter(|(_, (cur, to, _))| *cur != Some(*to))
        .map(|(customer_id, (from, to, rule_id))| Change {
            customer_id,
            from,
            to,
            rule_id,
        })
        .collect();
    out.sort_by_key(|c| c.customer_id);
    if !dry_run && !out.is_empty() {
        let mut tx = pool.begin().await.map_err(db_err)?;
        for c in &out {
            sqlx::query(
                "UPDATE customers SET client_group_id = $2, updated_at = now() WHERE id = $1",
            )
            .bind(c.customer_id)
            .bind(c.to)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::normalize;
    #[test]
    fn normalizes_values() {
        assert_eq!(normalize("country", "ro, de ,").unwrap(), "RO,DE");
        assert!(normalize("country", "ROU").is_err());
        assert_eq!(
            normalize("email_domain", "@Example.COM,uni.ro").unwrap(),
            "example.com,uni.ro"
        );
        assert!(normalize("email_domain", "nodot").is_err());
        assert_eq!(normalize("active_services_min", " 3 ").unwrap(), "3");
        assert!(normalize("account_age_days_min", "-1").is_err());
        assert!(normalize("bogus", "1").is_err());
    }
}
