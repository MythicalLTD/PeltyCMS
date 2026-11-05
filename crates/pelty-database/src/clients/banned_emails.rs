//! Banned e-mail domains (WHMCS "Banned Emails").
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BannedDomain {
    pub domain: String,
    pub reason: String,
    pub hits: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

pub fn normalize(domain: &str) -> Result<String, AppError> {
    let d = domain.trim().trim_start_matches('@').to_ascii_lowercase();
    let ok = !d.is_empty()
        && d.len() <= 253
        && d.contains('.')
        && !d.starts_with('.')
        && !d.ends_with('.')
        && d.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
    if ok {
        Ok(d)
    } else {
        Err(AppError::Validation("error.invalid_domain"))
    }
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<BannedDomain>> {
    sqlx::query_as(
        "SELECT domain, reason, hits, created_at FROM banned_email_domains ORDER BY domain",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add(pool: &PgPool, domain: &str, reason: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO banned_email_domains (domain, reason) VALUES ($1, $2) ON CONFLICT (domain) DO UPDATE SET reason = EXCLUDED.reason")
        .bind(domain).bind(reason).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn remove(pool: &PgPool, domain: &str) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM banned_email_domains WHERE domain = $1")
        .bind(domain)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        Err(AppError::NotFound)
    } else {
        Ok(())
    }
}

/// True when the address' domain (or a parent domain) is banned; counts a hit.
pub async fn is_banned(pool: &PgPool, email: &str) -> AppResult<bool> {
    let Some((_, host)) = email.trim().rsplit_once('@') else {
        return Ok(false);
    };
    let host = host.to_ascii_lowercase();
    let mut cands = vec![host.clone()];
    let mut rest = host.as_str();
    while let Some((_, r)) = rest.split_once('.') {
        if r.contains('.') {
            cands.push(r.to_string());
        }
        rest = r;
    }
    let n = sqlx::query("UPDATE banned_email_domains SET hits = hits + 1 WHERE domain = ANY($1)")
        .bind(&cands)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::normalize;
    #[test]
    fn normalizes() {
        assert_eq!(normalize(" @Mailinator.COM ").unwrap(), "mailinator.com");
        assert!(normalize("nodot").is_err());
        assert!(normalize("bad domain.com").is_err());
        assert!(normalize(".x.com").is_err());
    }
}
