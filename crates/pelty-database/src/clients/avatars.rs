//! Profile pictures (already validated and re-encoded by the API layer).
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use uuid::Uuid;

/// Store both sizes and bump the version. Returns the new version.
pub async fn set(pool: &PgPool, user_id: Uuid, large: &[u8], small: &[u8]) -> AppResult<i64> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let v: i64 = sqlx::query_scalar(
        "UPDATE users SET avatar_version = (extract(epoch from clock_timestamp()) * 1000)::bigint WHERE id = $1 RETURNING avatar_version",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query(
        "INSERT INTO user_avatars (user_id, large, small, version) VALUES ($1,$2,$3,$4)
         ON CONFLICT (user_id) DO UPDATE SET large = EXCLUDED.large, small = EXCLUDED.small, version = EXCLUDED.version, updated_at = now()",
    )
    .bind(user_id)
    .bind(large)
    .bind(small)
    .bind(v)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(v)
}

pub async fn remove(pool: &PgPool, user_id: Uuid) -> AppResult<bool> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("DELETE FROM user_avatars WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE users SET avatar_version = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// (bytes, version) for the requested size.
pub async fn get(pool: &PgPool, user_id: Uuid, small: bool) -> AppResult<Option<(Vec<u8>, i64)>> {
    let col = if small { "small" } else { "large" };
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {col}, version FROM user_avatars WHERE user_id = $1"
    )))
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}
