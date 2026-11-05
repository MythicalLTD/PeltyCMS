//! Staff roles/permissions editor, role assignment and the staff 2FA policy.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub const REQUIRE_2FA_KEY: &str = "security.staff_require_2fa";
const ADMIN_ROLE: &str = "admin";

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    pub permissions: Vec<String>,
    pub members: i64,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct RoleInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct StaffMember {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub roles: Vec<String>,
    pub mfa_enabled: bool,
    pub passkeys: i64,
}

/// Role names follow the DB check: lowercase, 2..41 chars.
pub fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    (2..=41).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

const ROLE_SQL: &str = "SELECT r.id, r.name, r.description, r.builtin, \
    ARRAY(SELECT p.name FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id WHERE rp.role_id = r.id ORDER BY p.name) AS permissions, \
    (SELECT count(*) FROM user_roles ur WHERE ur.role_id = r.id) AS members FROM roles r";

pub async fn permissions(pool: &PgPool) -> AppResult<Vec<String>> {
    sqlx::query_scalar("SELECT name FROM permissions ORDER BY name")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn roles(pool: &PgPool) -> AppResult<Vec<Role>> {
    sqlx::query_as(&format!("{ROLE_SQL} ORDER BY r.builtin DESC, r.name"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn role(pool: &PgPool, id: Uuid) -> AppResult<Role> {
    sqlx::query_as(&format!("{ROLE_SQL} WHERE r.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

async fn set_perms(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    perms: &[String],
) -> AppResult<()> {
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM permissions WHERE name = ANY($1)")
        .bind(perms)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    let mut uniq = perms.to_vec();
    uniq.sort();
    uniq.dedup();
    if known != uniq.len() as i64 {
        return Err(AppError::Validation("error.unknown_permission"));
    }
    sqlx::query("DELETE FROM role_permissions WHERE role_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO role_permissions (role_id, permission_id) SELECT $1, id FROM permissions WHERE name = ANY($2)")
        .bind(id)
        .bind(&uniq)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn create_role(pool: &PgPool, i: &RoleInput) -> AppResult<Uuid> {
    let name = i.name.trim();
    if !valid_name(name) || i.description.chars().count() > 200 {
        return Err(AppError::Validation("error.invalid_role"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = Uuid::new_v4();
    let r = sqlx::query(
        "INSERT INTO roles (id, name, description) VALUES ($1,$2,$3) ON CONFLICT (name) DO NOTHING",
    )
    .bind(id)
    .bind(name)
    .bind(i.description.trim())
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.role_exists"));
    }
    set_perms(&mut tx, id, &i.permissions).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

/// Builtin roles keep their name; the admin role always keeps every permission.
pub async fn update_role(pool: &PgPool, id: Uuid, i: &RoleInput) -> AppResult<()> {
    let cur = role(pool, id).await?;
    let name = i.name.trim();
    if !valid_name(name) || i.description.chars().count() > 200 {
        return Err(AppError::Validation("error.invalid_role"));
    }
    if cur.builtin && name != cur.name || cur.name == ADMIN_ROLE {
        return Err(AppError::Conflict("error.role_locked"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("UPDATE roles SET name = $2, description = $3 WHERE id = $1 AND NOT EXISTS (SELECT 1 FROM roles WHERE name = $2 AND id <> $1)")
        .bind(id)
        .bind(name)
        .bind(i.description.trim())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.role_exists"));
    }
    set_perms(&mut tx, id, &i.permissions).await?;
    tx.commit().await.map_err(db_err)
}

pub async fn delete_role(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let cur = role(pool, id).await?;
    if cur.builtin {
        return Err(AppError::Conflict("error.role_locked"));
    }
    if cur.members > 0 {
        return Err(AppError::Conflict("error.role_in_use"));
    }
    sqlx::query("DELETE FROM roles WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Users holding any role other than `customer`.
pub async fn staff(pool: &PgPool) -> AppResult<Vec<StaffMember>> {
    sqlx::query_as(
        "SELECT u.id, u.email::text AS email, u.display_name, \
         ARRAY(SELECT r.name FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE ur.user_id = u.id ORDER BY r.name) AS roles, \
         (EXISTS(SELECT 1 FROM user_mfa m WHERE m.user_id = u.id AND m.enabled_at IS NOT NULL) \
          OR (EXISTS(SELECT 1 FROM user_passkey_2fa f WHERE f.user_id = u.id) AND EXISTS(SELECT 1 FROM passkeys p WHERE p.user_id = u.id))) AS mfa_enabled, \
         (SELECT count(*) FROM passkeys p WHERE p.user_id = u.id) AS passkeys \
         FROM users u WHERE EXISTS (SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE ur.user_id = u.id AND r.name <> 'customer') \
         ORDER BY u.email LIMIT 500",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Replace a user's roles. An actor can't drop their own admin role and the
/// last admin can't be demoted (lock-out guards).
pub async fn set_user_roles(
    pool: &PgPool,
    actor: Uuid,
    user: Uuid,
    names: &[String],
) -> AppResult<()> {
    let mut want = names.to_vec();
    want.sort();
    want.dedup();
    if want.is_empty() {
        return Err(AppError::Validation("error.roles_required"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM roles WHERE name = ANY($1)")
        .bind(&want)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
    if ids.len() != want.len() {
        return Err(AppError::Validation("error.unknown_role"));
    }
    let keeps_admin = want.iter().any(|r| r == ADMIN_ROLE);
    if !keeps_admin {
        if actor == user {
            return Err(AppError::Conflict("error.cannot_demote_self"));
        }
        sqlx::query("SELECT 1 FROM roles WHERE name = 'admin' FOR UPDATE")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let others: i64 = sqlx::query_scalar("SELECT count(*) FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE r.name = 'admin' AND ur.user_id <> $1")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        if others == 0 {
            return Err(AppError::Conflict("error.last_admin"));
        }
    }
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO user_roles (user_id, role_id) SELECT $1, unnest($2::uuid[])")
        .bind(user)
        .bind(&ids)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

pub async fn require_2fa(pool: &PgPool) -> AppResult<bool> {
    Ok(crate::settings::get(pool, REQUIRE_2FA_KEY)
        .await?
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

/// True when the policy is on and this staff user has no 2FA yet.
pub async fn blocked_by_2fa(pool: &PgPool, user: Uuid) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT COALESCE((SELECT value = 'true'::jsonb FROM system_settings WHERE key = $2), false) \
         AND NOT EXISTS(SELECT 1 FROM user_mfa WHERE user_id = $1 AND enabled_at IS NOT NULL) \
         AND NOT (EXISTS(SELECT 1 FROM user_passkey_2fa WHERE user_id = $1) AND EXISTS(SELECT 1 FROM passkeys WHERE user_id = $1))",
    )
    .bind(user)
    .bind(REQUIRE_2FA_KEY)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::valid_name;
    #[test]
    fn names() {
        assert!(valid_name("billing_staff"));
        assert!(!valid_name("Billing"));
        assert!(!valid_name("a"));
        assert!(!valid_name("1abc"));
        assert!(!valid_name("a-b"));
    }
}
