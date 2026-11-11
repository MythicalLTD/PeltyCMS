//! Security / accounts suite storage (migration 0240): captcha nonces,
//! OAuth states + linked identities, IP rules, impersonation sessions,
//! WHMCS-compatible API credentials, versioned ToS, client close/merge.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

// ----- captcha -----

/// Consume a one-time captcha nonce; false when it was already used.
pub async fn consume_captcha_nonce(
    pool: &PgPool,
    nonce: &str,
    expires_at: OffsetDateTime,
) -> AppResult<bool> {
    sqlx::query("DELETE FROM captcha_nonces WHERE expires_at < now()")
        .execute(pool)
        .await
        .map_err(db_err)?;
    let r = sqlx::query(
        "INSERT INTO captcha_nonces (nonce, expires_at) VALUES ($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(nonce)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

// ----- OAuth -----

#[derive(Debug, Clone, FromRow)]
pub struct OauthState {
    pub provider: String,
    pub verifier: String,
    pub nonce: String,
    pub mode: String,
    pub user_id: Option<Uuid>,
    /// Hash of the browser-binding cookie set at start (game sign-in).
    pub bind_hash: Option<Vec<u8>>,
    /// Same-site path to land on after signing in.
    pub return_to: Option<String>,
}

/// Extra state of a sign-in attempt beyond PKCE/nonce.
#[derive(Debug, Clone, Default)]
pub struct StateExtra<'a> {
    pub bind_hash: Option<&'a [u8]>,
    pub return_to: Option<&'a str>,
}

pub async fn put_oauth_state(
    pool: &PgPool,
    hash: &[u8],
    provider: &str,
    verifier: &str,
    nonce: &str,
    mode: &str,
    user_id: Option<Uuid>,
) -> AppResult<()> {
    put_oauth_state_ext(
        pool,
        hash,
        provider,
        verifier,
        nonce,
        mode,
        user_id,
        StateExtra::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn put_oauth_state_ext(
    pool: &PgPool,
    hash: &[u8],
    provider: &str,
    verifier: &str,
    nonce: &str,
    mode: &str,
    user_id: Option<Uuid>,
    extra: StateExtra<'_>,
) -> AppResult<()> {
    sqlx::query("DELETE FROM oauth_states WHERE expires_at < now()")
        .execute(pool)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO oauth_states (state_hash, provider, verifier, nonce, mode, user_id, bind_hash, return_to, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now() + interval '10 minutes')")
        .bind(hash)
        .bind(provider)
        .bind(verifier)
        .bind(nonce)
        .bind(mode)
        .bind(user_id)
        .bind(extra.bind_hash)
        .bind(extra.return_to)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// One-time: deletes the state row.
pub async fn take_oauth_state(pool: &PgPool, hash: &[u8]) -> AppResult<Option<OauthState>> {
    sqlx::query_as("DELETE FROM oauth_states WHERE state_hash = $1 AND expires_at > now() RETURNING provider, verifier, nonce, mode, user_id, bind_hash, return_to")
        .bind(hash)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Identity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub subject: String,
    pub email: String,
    /// Display handle at the provider (Cfx.re username, Steam persona).
    pub username: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_login_at: Option<OffsetDateTime>,
}
const ID_COLS: &str = "id, user_id, provider, subject, email, username, created_at, last_login_at";

pub async fn find_identity(
    pool: &PgPool,
    provider: &str,
    subject: &str,
) -> AppResult<Option<Identity>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ID_COLS} FROM user_identities WHERE provider = $1 AND subject = $2"
    )))
    .bind(provider)
    .bind(subject)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Remember the provider's display handle of an identity.
pub async fn set_identity_username(
    pool: &PgPool,
    provider: &str,
    subject: &str,
    username: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE user_identities SET username = $3 WHERE provider = $1 AND subject = $2")
        .bind(provider)
        .bind(subject)
        .bind(username.chars().take(100).collect::<String>())
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Verified identity without an e-mail, waiting for the sign-up form.
#[derive(Debug, Clone, FromRow)]
pub struct SsoSignup {
    pub provider: String,
    pub subject: String,
    pub name: String,
    pub username: String,
    pub return_to: Option<String>,
}

pub async fn put_sso_signup(pool: &PgPool, hash: &[u8], s: &SsoSignup) -> AppResult<()> {
    sqlx::query("DELETE FROM sso_signups WHERE expires_at < now()")
        .execute(pool)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO sso_signups (token_hash, provider, subject, name, username, return_to, expires_at) VALUES ($1,$2,$3,$4,$5,$6, now() + interval '15 minutes')")
        .bind(hash)
        .bind(&s.provider)
        .bind(&s.subject)
        .bind(s.name.chars().take(100).collect::<String>())
        .bind(s.username.chars().take(100).collect::<String>())
        .bind(&s.return_to)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Read a pending sign-up without consuming it (the form shows its name).
pub async fn peek_sso_signup(pool: &PgPool, hash: &[u8]) -> AppResult<Option<SsoSignup>> {
    sqlx::query_as("SELECT provider, subject, name, username, return_to FROM sso_signups WHERE token_hash = $1 AND expires_at > now()")
        .bind(hash)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// One-time: deletes the pending sign-up.
pub async fn take_sso_signup(pool: &PgPool, hash: &[u8]) -> AppResult<Option<SsoSignup>> {
    sqlx::query_as("DELETE FROM sso_signups WHERE token_hash = $1 AND expires_at > now() RETURNING provider, subject, name, username, return_to")
        .bind(hash)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn touch_identity(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE user_identities SET last_login_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn link_identity(
    pool: &PgPool,
    user_id: Uuid,
    provider: &str,
    subject: &str,
    email: &str,
) -> AppResult<Identity> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO user_identities (id, user_id, provider, subject, email) VALUES ($1,$2,$3,$4,$5) RETURNING {ID_COLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(provider)
    .bind(subject)
    .bind(email.chars().take(254).collect::<String>())
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.identity_linked"),
        _ => db_err(e),
    })
}

pub async fn list_identities(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<Identity>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {ID_COLS} FROM user_identities WHERE user_id = $1 ORDER BY provider"
    )))
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn unlink_identity(pool: &PgPool, user_id: Uuid, id: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar(
        "DELETE FROM user_identities WHERE id = $1 AND user_id = $2 RETURNING provider",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

// ----- IP rules -----

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct IpRule {
    pub id: Uuid,
    pub cidr: String,
    pub kind: String,
    pub reason: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub created_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
const IP_COLS: &str = "id, cidr, kind, reason, expires_at, created_by, created_at";

/// Active (non-expired) rules.
pub async fn active_ip_rules(pool: &PgPool) -> AppResult<Vec<IpRule>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {IP_COLS} FROM ip_rules WHERE expires_at IS NULL OR expires_at > now() ORDER BY created_at")))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn list_ip_rules(pool: &PgPool) -> AppResult<Vec<IpRule>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {IP_COLS} FROM ip_rules ORDER BY kind, created_at DESC"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add_ip_rule(
    pool: &PgPool,
    cidr: &str,
    kind: &str,
    reason: &str,
    expires_at: Option<OffsetDateTime>,
    by: Uuid,
) -> AppResult<IpRule> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO ip_rules (id, cidr, kind, reason, expires_at, created_by) VALUES ($1,$2,$3,$4,$5,$6) RETURNING {IP_COLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(cidr)
    .bind(kind)
    .bind(reason)
    .bind(expires_at)
    .bind(by)
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.ip_rule_exists"),
        _ => db_err(e),
    })
}

pub async fn delete_ip_rule(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM ip_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

// ----- impersonation -----

/// Mark a freshly created session as an impersonation of `target` by `admin`.
pub async fn mark_impersonation(
    pool: &PgPool,
    session_id: Uuid,
    admin: Uuid,
    parent: Uuid,
) -> AppResult<()> {
    sqlx::query("UPDATE sessions SET impersonator_id = $2, parent_session_id = $3 WHERE id = $1")
        .bind(session_id)
        .bind(admin)
        .bind(parent)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// (impersonator_id, parent_session_id) of a session.
pub async fn impersonation_of(
    pool: &PgPool,
    session_id: Uuid,
) -> AppResult<Option<(Uuid, Option<Uuid>)>> {
    let r: Option<(Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as("SELECT impersonator_id, parent_session_id FROM sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(r.and_then(|(a, p)| a.map(|a| (a, p))))
}

/// True when the session is live (not revoked / expired).
pub async fn session_live(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sessions WHERE id = $1 AND revoked_at IS NULL AND idle_expires_at > now() AND absolute_expires_at > now())")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

// ----- ToS -----

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TosVersion {
    pub id: Uuid,
    pub version: String,
    pub url: String,
    pub summary: String,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: OffsetDateTime,
}

pub async fn publish_tos(
    pool: &PgPool,
    version: &str,
    url: &str,
    summary: &str,
) -> AppResult<TosVersion> {
    sqlx::query_as("INSERT INTO tos_versions (id, version, url, summary) VALUES ($1,$2,$3,$4) RETURNING id, version, url, summary, published_at")
        .bind(Uuid::new_v4())
        .bind(version)
        .bind(url)
        .bind(summary)
        .fetch_one(pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.tos_version_exists"),
            _ => db_err(e),
        })
}

pub async fn list_tos(pool: &PgPool) -> AppResult<Vec<(TosVersion, i64)>> {
    let rows: Vec<(Uuid, String, String, String, OffsetDateTime, i64)> = sqlx::query_as(
        "SELECT v.id, v.version, v.url, v.summary, v.published_at, (SELECT count(*) FROM tos_acceptances a WHERE a.version_id = v.id)
         FROM tos_versions v ORDER BY v.published_at DESC, v.version DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, version, url, summary, published_at, n)| {
            (
                TosVersion {
                    id,
                    version,
                    url,
                    summary,
                    published_at,
                },
                n,
            )
        })
        .collect())
}

pub async fn current_tos(pool: &PgPool) -> AppResult<Option<TosVersion>> {
    sqlx::query_as("SELECT id, version, url, summary, published_at FROM tos_versions ORDER BY published_at DESC, version DESC LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn accepted_tos(
    pool: &PgPool,
    user_id: Uuid,
    version_id: Uuid,
) -> AppResult<Option<OffsetDateTime>> {
    sqlx::query_scalar(
        "SELECT accepted_at FROM tos_acceptances WHERE user_id = $1 AND version_id = $2",
    )
    .bind(user_id)
    .bind(version_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn accept_tos(
    pool: &PgPool,
    user_id: Uuid,
    version_id: Uuid,
    ip: Option<&str>,
) -> AppResult<()> {
    sqlx::query("INSERT INTO tos_acceptances (user_id, version_id, ip) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .bind(version_id)
        .bind(ip)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// All acceptances of a user (GDPR export).
pub async fn tos_history(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<(String, OffsetDateTime)>> {
    sqlx::query_as("SELECT v.version, a.accepted_at FROM tos_acceptances a JOIN tos_versions v ON v.id = a.version_id WHERE a.user_id = $1 ORDER BY a.accepted_at")
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

// ----- customers: close / merge -----

/// Close a client account: status closed, sessions revoked, login disabled.
/// Returns the owner user id.
pub async fn close_customer(pool: &PgPool, id: Uuid) -> AppResult<Option<Option<Uuid>>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let owner: Option<Option<Uuid>> = sqlx::query_scalar("UPDATE customers SET status = 'closed', closed_at = now() WHERE id = $1 AND merged_into IS NULL RETURNING owner_user_id")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    if let Some(Some(u)) = owner {
        sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(u)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(owner)
}

/// Re-point every row that references customer `from` to `into`, then close
/// `from` with `merged_into` set. Runs in one transaction. Returns the number
/// of re-pointed rows. Tables with a UNIQUE customer reference keep the
/// target's row (the source row is left on the closed account).
pub async fn merge_customers(pool: &PgPool, from: Uuid, into: Uuid) -> AppResult<i64> {
    if from == into {
        return Err(AppError::Validation("error.merge_same"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let ok: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM customers WHERE id IN ($1,$2) AND merged_into IS NULL FOR UPDATE",
    )
    .bind(from)
    .bind(into)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    if ok.len() != 2 {
        return Err(AppError::NotFound);
    }
    // Every FK column pointing at customers(id), discovered from the catalog.
    let refs: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.conrelid::regclass::text, a.attname::text
           FROM pg_constraint c
           JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey)
          WHERE c.contype = 'f' AND c.confrelid = 'customers'::regclass AND array_length(c.conkey, 1) = 1
            AND NOT (c.conrelid = 'customers'::regclass)",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut moved = 0i64;
    for (table, col) in refs {
        // Identifiers come from pg_catalog (not user input); quote anyway.
        let q = format!(
            "UPDATE {} SET \"{}\" = $2 WHERE \"{}\" = $1",
            table,
            col.replace('"', ""),
            col.replace('"', "")
        );
        sqlx::query("SAVEPOINT merge_step")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        match sqlx::query(sqlx::AssertSqlSafe(q))
            .bind(from)
            .bind(into)
            .execute(&mut *tx)
            .await
        {
            Ok(r) => {
                moved += r.rows_affected() as i64;
                sqlx::query("RELEASE SAVEPOINT merge_step")
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
            }
            Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
                sqlx::query("ROLLBACK TO SAVEPOINT merge_step")
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
            }
            Err(e) => return Err(db_err(e)),
        }
    }
    sqlx::query(
        "UPDATE customers SET status = 'closed', closed_at = now(), merged_into = $2 WHERE id = $1",
    )
    .bind(from)
    .bind(into)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(moved)
}
