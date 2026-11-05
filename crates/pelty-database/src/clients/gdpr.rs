//! GDPR data export and erasure.
//!
//! Export: everything we hold about a user as JSON, with secrets (password,
//! token and key hashes, CSRF tokens, MFA seeds) stripped.
//!
//! Erasure anonymises the user and their customer record and deletes
//! credentials, sessions, contacts and avatars. Invoices, payments and
//! orders are kept (statutory bookkeeping retention) but no longer point to
//! identifying data. The audit log is append-only (DB trigger) and is
//! retained as a legitimate-interest security record.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

/// (section, SQL with $1 = user id). Each returns one jsonb array.
const SECTIONS: &[(&str, &str)] = &[
    ("account", "SELECT to_jsonb(u) - 'password_hash' FROM users u WHERE u.id = $1"),
    ("roles", "SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = $1"),
    ("sessions", "SELECT to_jsonb(s) - 'token_hash' - 'csrf_token' FROM sessions s WHERE s.user_id = $1"),
    ("api_keys", "SELECT to_jsonb(k) - 'key_hash' FROM api_keys k WHERE k.user_id = $1"),
    ("mfa_enabled", "SELECT to_jsonb(EXISTS(SELECT 1 FROM user_mfa WHERE user_id = $1))"),
    ("customer", "SELECT to_jsonb(c) - 'notes' FROM customers c WHERE c.owner_user_id = $1"),
    ("contacts", "SELECT to_jsonb(x) FROM customer_contacts x JOIN customers c ON c.id = x.customer_id WHERE c.owner_user_id = $1"),
    ("orders", "SELECT to_jsonb(o) FROM orders o JOIN customers c ON c.id = o.customer_id WHERE c.owner_user_id = $1"),
    ("services", "SELECT to_jsonb(s) - 'module_data' FROM services s JOIN customers c ON c.id = s.customer_id WHERE c.owner_user_id = $1"),
    ("invoices", "SELECT to_jsonb(i) || jsonb_build_object('items', (SELECT coalesce(jsonb_agg(to_jsonb(it) ORDER BY it.sort_order), '[]') FROM invoice_items it WHERE it.invoice_id = i.id))
                  FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE c.owner_user_id = $1"),
    ("payments", "SELECT to_jsonb(p) FROM payments p WHERE p.user_id = $1 OR p.customer_id IN (SELECT id FROM customers WHERE owner_user_id = $1)"),
    ("tickets", "SELECT to_jsonb(t) || jsonb_build_object('messages', (SELECT coalesce(jsonb_agg(jsonb_build_object('from_staff', m.is_staff, 'body', m.body, 'created_at', m.created_at) ORDER BY m.created_at), '[]')
                  FROM ticket_messages m WHERE m.ticket_id = t.id AND NOT m.is_internal))
                 FROM tickets t JOIN customers c ON c.id = t.customer_id WHERE c.owner_user_id = $1"),
    ("activity", "SELECT jsonb_build_object('action', a.action, 'ip', a.ip, 'at', a.created_at) FROM audit_logs a WHERE a.actor_id = $1"),
    ("linked_identities", "SELECT to_jsonb(i) FROM user_identities i WHERE i.user_id = $1"),
    ("tos_acceptances", "SELECT jsonb_build_object('version', v.version, 'accepted_at', a.accepted_at, 'ip', a.ip) FROM tos_acceptances a JOIN tos_versions v ON v.id = a.version_id WHERE a.user_id = $1"),
    ("erasure_requests", "SELECT to_jsonb(g) - 'decided_by' FROM gdpr_requests g WHERE g.user_id = $1"),
];

pub async fn export(pool: &PgPool, user_id: Uuid) -> AppResult<Value> {
    let mut out = serde_json::Map::new();
    for (name, q) in SECTIONS {
        let rows: Vec<Value> = sqlx::query_scalar(&format!("SELECT to_jsonb(x) FROM ({q}) AS x(v)"))
            .bind(user_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
        // Unwrap the single-column record.
        let rows: Vec<Value> = rows.into_iter().map(|r| r.get("v").cloned().unwrap_or(r)).collect();
        let v = match *name {
            "account" | "customer" | "mfa_enabled" => rows.into_iter().next().unwrap_or(Value::Null),
            _ => Value::Array(rows),
        };
        out.insert((*name).to_string(), v);
    }
    out.insert("exported_at".into(), Value::String(OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default()));
    Ok(Value::Object(out))
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Request {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub subject_ref: String,
    pub status: String,
    pub reason: String,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub decided_at: Option<OffsetDateTime>,
}
const COLS: &str = "id, user_id, subject_ref, status, reason, note, requested_at, decided_at";

pub async fn latest_for(pool: &PgPool, user_id: Uuid) -> AppResult<Option<Request>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM gdpr_requests WHERE user_id = $1 ORDER BY requested_at DESC LIMIT 1"))
        .bind(user_id).fetch_optional(pool).await.map_err(db_err)
}

pub async fn create(pool: &PgPool, user_id: Uuid, email: &str, reason: &str) -> AppResult<Request> {
    sqlx::query_as(&format!("INSERT INTO gdpr_requests (user_id, subject_ref, reason) VALUES ($1,$2,$3) RETURNING {COLS}"))
        .bind(user_id).bind(email).bind(reason).fetch_one(pool).await.map_err(db_err)
}

pub async fn cancel(pool: &PgPool, user_id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("UPDATE gdpr_requests SET status = 'cancelled', decided_at = now() WHERE user_id = $1 AND status = 'pending'")
        .bind(user_id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

pub async fn list(pool: &PgPool, status: Option<&str>, limit: i64, offset: i64) -> AppResult<(Vec<Request>, i64)> {
    let items = sqlx::query_as(&format!("SELECT {COLS} FROM gdpr_requests WHERE ($1::text IS NULL OR status = $1) ORDER BY requested_at DESC, id LIMIT $2 OFFSET $3"))
        .bind(status).bind(limit).bind(offset).fetch_all(pool).await.map_err(db_err)?;
    let total = sqlx::query_scalar("SELECT count(*) FROM gdpr_requests WHERE ($1::text IS NULL OR status = $1)")
        .bind(status).fetch_one(pool).await.map_err(db_err)?;
    Ok((items, total))
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Request>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM gdpr_requests WHERE id = $1")).bind(id).fetch_optional(pool).await.map_err(db_err)
}

pub async fn reject(pool: &PgPool, id: Uuid, by: Uuid, note: &str) -> AppResult<bool> {
    let r = sqlx::query("UPDATE gdpr_requests SET status = 'rejected', note = $3, decided_at = now(), decided_by = $2 WHERE id = $1 AND status = 'pending'")
        .bind(id).bind(by).bind(note).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Why erasure can't run yet (an i18n error code), if anything.
pub async fn blocker(pool: &PgPool, user_id: Uuid) -> AppResult<Option<&'static str>> {
    let (staff, live, unpaid): (bool, bool, bool) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE ur.user_id = $1 AND r.name <> 'customer'),
                EXISTS(SELECT 1 FROM services s JOIN customers c ON c.id = s.customer_id WHERE c.owner_user_id = $1 AND s.status IN ('pending','active','suspended')),
                EXISTS(SELECT 1 FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE c.owner_user_id = $1 AND i.status = 'unpaid')",
    )
    .bind(user_id).fetch_one(pool).await.map_err(db_err)?;
    Ok(if staff {
        Some("error.gdpr_staff_account")
    } else if live {
        Some("error.gdpr_active_services")
    } else if unpaid {
        Some("error.gdpr_unpaid_invoices")
    } else {
        None
    })
}

/// Anonymise the user (one transaction) and complete the request.
pub async fn erase(pool: &PgPool, request_id: Uuid, by: Uuid) -> AppResult<()> {
    erase_as(pool, request_id, Some(by)).await
}

/// Erasure decided by the retention job (no staff actor).
pub async fn erase_system(pool: &PgPool, request_id: Uuid) -> AppResult<()> {
    erase_as(pool, request_id, None).await
}

async fn erase_as(pool: &PgPool, request_id: Uuid, by: Option<Uuid>) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let user_id: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM gdpr_requests WHERE id = $1 AND status = 'pending' FOR UPDATE")
        .bind(request_id).fetch_optional(&mut *tx).await.map_err(db_err)?.flatten();
    let user_id = user_id.ok_or(AppError::NotFound)?;
    let anon = format!("erased-{}@erased.invalid", user_id.simple());
    let stmts: &[&str] = &[
        "DELETE FROM sessions WHERE user_id = $1",
        "DELETE FROM user_tokens WHERE user_id = $1",
        "DELETE FROM user_mfa WHERE user_id = $1",
        "DELETE FROM user_backup_codes WHERE user_id = $1",
        "DELETE FROM mfa_challenges WHERE user_id = $1",
        "DELETE FROM user_avatars WHERE user_id = $1",
        "DELETE FROM api_keys WHERE user_id = $1",
        "DELETE FROM customer_contacts WHERE customer_id IN (SELECT id FROM customers WHERE owner_user_id = $1)",
        "UPDATE ticket_messages SET body = '[erased]' WHERE author_id = $1",
    ];
    for q in stmts {
        sqlx::query(q).bind(user_id).execute(&mut *tx).await.map_err(db_err)?;
    }
    sqlx::query("UPDATE customers SET email = $2, first_name = '', last_name = '', organization = '', phone = '', address_line1 = '', address_line2 = '',
                 city = '', region = '', postal_code = '', country = '', tax_id = '', vat_number = '', notes = '', status = 'closed', updated_at = now()
                 WHERE owner_user_id = $1")
        .bind(user_id).bind(&anon).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE users SET email = $2, display_name = '', password_hash = '!erased', locale = NULL, is_active = false,
                 email_verified_at = NULL, avatar_version = NULL, updated_at = now() WHERE id = $1")
        .bind(user_id).bind(&anon).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE gdpr_requests SET status = 'completed', decided_at = now(), decided_by = $2, subject_ref = $3, reason = '' WHERE id = $1")
        .bind(request_id).bind(by).bind(&anon).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}
