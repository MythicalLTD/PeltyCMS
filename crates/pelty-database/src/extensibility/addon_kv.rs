//! Storage for the WASM addon key/value host functions.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;

/// Max keys one addon may keep.
pub const MAX_KEYS: i64 = 10_000;

pub async fn get(pool: &PgPool, addon: &str, key: &str) -> AppResult<Option<(Vec<u8>, bool)>> {
    sqlx::query_as("SELECT value, sealed FROM addon_kv WHERE addon_id = $1 AND key = $2")
        .bind(addon)
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Upsert; `false` when the addon is at [`MAX_KEYS`] and `key` is new.
pub async fn set(
    pool: &PgPool,
    addon: &str,
    key: &str,
    value: &[u8],
    sealed: bool,
) -> AppResult<bool> {
    let r = sqlx::query(
        "INSERT INTO addon_kv (addon_id, key, value, sealed)
         SELECT $1, $2, $3, $4
         WHERE EXISTS (SELECT 1 FROM addon_kv WHERE addon_id = $1 AND key = $2)
            OR (SELECT count(*) FROM addon_kv WHERE addon_id = $1) < $5
         ON CONFLICT (addon_id, key) DO UPDATE SET value = EXCLUDED.value, sealed = EXCLUDED.sealed, updated_at = now()",
    )
    .bind(addon)
    .bind(key)
    .bind(value)
    .bind(sealed)
    .bind(MAX_KEYS)
    .execute(pool)
    .await.map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete(pool: &PgPool, addon: &str, key: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1 AND key = $2")
        .bind(addon)
        .bind(key)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Drop everything an addon stored (uninstall).
pub async fn clear(pool: &PgPool, addon: &str) -> AppResult<u64> {
    Ok(sqlx::query("DELETE FROM addon_kv WHERE addon_id = $1")
        .bind(addon)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected())
}
