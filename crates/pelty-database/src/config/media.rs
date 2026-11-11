//! Uploaded images (`media_files`): who uploaded what, for per-user quotas.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use uuid::Uuid;

/// Upload limits for one non-staff uploader (creators).
pub const CREATOR_PER_HOUR: i64 = 60;
pub const CREATOR_MAX_FILES: i64 = 1000;
pub const CREATOR_MAX_BYTES: i64 = 500 * 1024 * 1024;

pub async fn record(
    pool: &PgPool,
    id: Uuid,
    owner: Option<Uuid>,
    scope: &str,
    bytes: usize,
    (w, h): (u32, u32),
) -> AppResult<()> {
    sqlx::query("INSERT INTO media_files (id, owner_user_id, scope, bytes, width, height) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(owner)
        .bind(scope)
        .bind(i32::try_from(bytes).unwrap_or(i32::MAX))
        .bind(i32::try_from(w).unwrap_or(0))
        .bind(i32::try_from(h).unwrap_or(0))
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// (files in the last hour, files in total, bytes in total) for one uploader and scope.
pub async fn usage(pool: &PgPool, owner: Uuid, scope: &str) -> AppResult<(i64, i64, i64)> {
    let r: (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE created_at > now() - interval '1 hour'),
                count(*), COALESCE(sum(bytes), 0)::BIGINT
           FROM media_files WHERE owner_user_id = $1 AND scope = $2",
    )
    .bind(owner)
    .bind(scope)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok(r)
}

/// The quota error for a creator about to upload `next` more bytes, if any.
pub fn creator_quota_error(
    (hour, files, bytes): (i64, i64, i64),
    next: usize,
) -> Option<&'static str> {
    if hour >= CREATOR_PER_HOUR {
        Some("error.media_rate_limited")
    } else if files >= CREATOR_MAX_FILES || bytes + next as i64 > CREATOR_MAX_BYTES {
        Some("error.media_quota")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota() {
        assert_eq!(creator_quota_error((0, 0, 0), 10), None);
        assert_eq!(
            creator_quota_error((CREATOR_PER_HOUR, 1, 1), 10),
            Some("error.media_rate_limited")
        );
        assert_eq!(
            creator_quota_error((1, CREATOR_MAX_FILES, 1), 10),
            Some("error.media_quota")
        );
        assert_eq!(
            creator_quota_error((1, 1, CREATOR_MAX_BYTES - 5), 10),
            Some("error.media_quota")
        );
    }
}
