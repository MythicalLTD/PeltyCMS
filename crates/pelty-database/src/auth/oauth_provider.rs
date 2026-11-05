//! Storage for PeltyCMS acting as an OAuth 2.0 / OIDC provider. Codes and
//! tokens are passed in already hashed; nothing here sees a raw secret.
use crate::db_err;
use pelty_core::AppResult;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Client {
    pub id: Uuid,
    pub client_id: String,
    pub secret_hash: Option<String>,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub trusted: bool,
    pub enabled: bool,
}

const COLS: &str = "id, client_id, secret_hash, name, redirect_uris, scopes, trusted, enabled";

pub async fn client_by_public_id(pool: &PgPool, cid: &str) -> AppResult<Option<Client>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM oauth_clients WHERE client_id=$1"
    )))
    .bind(cid)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn list_clients(pool: &PgPool) -> AppResult<Vec<Client>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM oauth_clients ORDER BY created_at DESC"
    )))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub struct NewClient<'a> {
    pub id: Uuid,
    pub client_id: &'a str,
    pub secret_hash: Option<&'a str>,
    pub name: &'a str,
    pub redirect_uris: &'a [String],
    pub scopes: &'a [String],
    pub trusted: bool,
}

pub async fn create_client(pool: &PgPool, c: &NewClient<'_>) -> AppResult<()> {
    sqlx::query("INSERT INTO oauth_clients (id, client_id, secret_hash, name, redirect_uris, scopes, trusted) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(c.id).bind(c.client_id).bind(c.secret_hash).bind(c.name).bind(c.redirect_uris).bind(c.scopes).bind(c.trusted)
        .execute(pool).await.map_err(db_err)?;
    Ok(())
}

#[derive(Default)]
pub struct ClientPatch<'a> {
    pub name: Option<&'a str>,
    pub redirect_uris: Option<&'a [String]>,
    pub scopes: Option<&'a [String]>,
    pub trusted: Option<bool>,
    pub enabled: Option<bool>,
}

/// Disabling a client also revokes all its live tokens.
pub async fn update_client(
    pool: &PgPool,
    id: Uuid,
    p: &ClientPatch<'_>,
) -> AppResult<Option<Client>> {
    let row: Option<Client> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE oauth_clients SET name=COALESCE($2,name), redirect_uris=COALESCE($3,redirect_uris), scopes=COALESCE($4,scopes), \
         trusted=COALESCE($5,trusted), enabled=COALESCE($6,enabled) WHERE id=$1 RETURNING {COLS}"
    )))
    .bind(id).bind(p.name).bind(p.redirect_uris).bind(p.scopes).bind(p.trusted).bind(p.enabled)
    .fetch_optional(pool).await.map_err(db_err)?;
    if row.is_some() && p.enabled == Some(false) {
        sqlx::query(
            "UPDATE oauth_tokens SET revoked_at = now() WHERE client_id=$1 AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    }
    Ok(row)
}

pub async fn delete_client(pool: &PgPool, id: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar("DELETE FROM oauth_clients WHERE id=$1 RETURNING client_id")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn set_secret(pool: &PgPool, id: Uuid, secret_hash: &str) -> AppResult<Option<String>> {
    sqlx::query_scalar("UPDATE oauth_clients SET secret_hash=$2 WHERE id=$1 RETURNING client_id")
        .bind(id)
        .bind(secret_hash)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

// ---- signing keys

pub async fn active_key(pool: &PgPool) -> AppResult<Option<(String, Vec<u8>)>> {
    sqlx::query_as(
        "SELECT kid, pkcs8 FROM oauth_signing_keys WHERE active ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn insert_key(pool: &PgPool, kid: &str, pkcs8: &[u8], x: &str, y: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO oauth_signing_keys (kid, pkcs8, public_x, public_y) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(kid).bind(pkcs8).bind(x).bind(y).execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// (kid, x, y) of every published key, newest first.
pub async fn public_keys(pool: &PgPool) -> AppResult<Vec<(String, String, String)>> {
    sqlx::query_as(
        "SELECT kid, public_x, public_y FROM oauth_signing_keys ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ---- consents

pub async fn consent_scopes(
    pool: &PgPool,
    user: Uuid,
    client: Uuid,
) -> AppResult<Option<Vec<String>>> {
    sqlx::query_scalar("SELECT scopes FROM oauth_consents WHERE user_id=$1 AND client_id=$2")
        .bind(user)
        .bind(client)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Adds `scopes` to the stored consent (union).
pub async fn grant_consent(
    pool: &PgPool,
    user: Uuid,
    client: Uuid,
    scopes: &[String],
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO oauth_consents (user_id, client_id, scopes) VALUES ($1,$2,$3) \
         ON CONFLICT (user_id, client_id) DO UPDATE SET scopes = (SELECT array_agg(DISTINCT x) FROM unnest(oauth_consents.scopes || EXCLUDED.scopes) x), granted_at = now()",
    )
    .bind(user).bind(client).bind(scopes).execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// (client uuid, client name, scopes, granted_at)
pub async fn consents_of(
    pool: &PgPool,
    user: Uuid,
) -> AppResult<Vec<(Uuid, String, Vec<String>, OffsetDateTime)>> {
    sqlx::query_as(
        "SELECT c.id, c.name, k.scopes, k.granted_at FROM oauth_consents k JOIN oauth_clients c ON c.id=k.client_id WHERE k.user_id=$1 ORDER BY k.granted_at DESC",
    )
    .bind(user).fetch_all(pool).await.map_err(db_err)
}

/// Removes the consent and revokes the user's tokens for that client.
pub async fn revoke_consent(pool: &PgPool, user: Uuid, client: Uuid) -> AppResult<bool> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r = sqlx::query("DELETE FROM oauth_consents WHERE user_id=$1 AND client_id=$2")
        .bind(user)
        .bind(client)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE oauth_tokens SET revoked_at = now() WHERE user_id=$1 AND client_id=$2 AND revoked_at IS NULL")
        .bind(user).bind(client).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

// ---- codes

pub struct NewCode<'a> {
    pub code_hash: &'a str,
    pub client: Uuid,
    pub user: Uuid,
    pub redirect_uri: &'a str,
    pub scope: &'a str,
    pub nonce: Option<&'a str>,
    pub code_challenge: Option<&'a str>,
    pub expires_at: OffsetDateTime,
}

pub async fn insert_code(pool: &PgPool, c: &NewCode<'_>) -> AppResult<()> {
    sqlx::query("DELETE FROM oauth_codes WHERE expires_at < now() - interval '1 day'")
        .execute(pool)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO oauth_codes (code_hash, client_id, user_id, redirect_uri, scope, nonce, code_challenge, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(c.code_hash).bind(c.client).bind(c.user).bind(c.redirect_uri).bind(c.scope).bind(c.nonce).bind(c.code_challenge).bind(c.expires_at)
        .execute(pool).await.map_err(db_err)?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
pub struct Code {
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub redirect_uri: String,
    pub scope: String,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub expires_at: OffsetDateTime,
}

pub enum Redeem {
    Unknown,
    /// Second use of a code: every token minted from it was revoked.
    Replayed {
        user: Uuid,
    },
    Fresh(Code),
}

/// Marks the code used atomically. A replay revokes its token family.
pub async fn redeem_code(pool: &PgPool, code_hash: &str) -> AppResult<Redeem> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let row: Option<(Uuid, Option<OffsetDateTime>)> =
        sqlx::query_as("SELECT user_id, used_at FROM oauth_codes WHERE code_hash=$1 FOR UPDATE")
            .bind(code_hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let out = match row {
        None => Redeem::Unknown,
        Some((user, Some(_))) => {
            sqlx::query("UPDATE oauth_tokens SET revoked_at = now() WHERE code_hash=$1 AND revoked_at IS NULL")
                .bind(code_hash).execute(&mut *tx).await.map_err(db_err)?;
            Redeem::Replayed { user }
        }
        Some(_) => {
            let c: Code = sqlx::query_as(
                "UPDATE oauth_codes SET used_at = now() WHERE code_hash=$1 RETURNING client_id, user_id, redirect_uri, scope, nonce, code_challenge, expires_at",
            )
            .bind(code_hash).fetch_one(&mut *tx).await.map_err(db_err)?;
            Redeem::Fresh(c)
        }
    };
    tx.commit().await.map_err(db_err)?;
    Ok(out)
}

// ---- tokens

pub struct NewToken<'a> {
    pub token_hash: &'a str,
    pub kind: &'a str,
    pub client: Uuid,
    pub user: Uuid,
    pub scope: &'a str,
    pub code_hash: Option<&'a str>,
    pub expires_at: OffsetDateTime,
}

pub async fn insert_token(pool: &PgPool, t: &NewToken<'_>) -> AppResult<()> {
    sqlx::query("INSERT INTO oauth_tokens (token_hash, kind, client_id, user_id, scope, code_hash, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(t.token_hash).bind(t.kind).bind(t.client).bind(t.user).bind(t.scope).bind(t.code_hash).bind(t.expires_at)
        .execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// Burns a live refresh token of `client`: (user, scope, code_hash).
pub async fn take_refresh(
    pool: &PgPool,
    token_hash: &str,
    client: Uuid,
) -> AppResult<Option<(Uuid, String, Option<String>)>> {
    sqlx::query_as(
        "UPDATE oauth_tokens SET revoked_at = now() WHERE token_hash=$1 AND client_id=$2 AND kind='refresh' AND revoked_at IS NULL AND expires_at > now() \
         RETURNING user_id, scope, code_hash",
    )
    .bind(token_hash).bind(client).fetch_optional(pool).await.map_err(db_err)
}

/// Live access token of an enabled client: (user, scope).
pub async fn access_token(pool: &PgPool, token_hash: &str) -> AppResult<Option<(Uuid, String)>> {
    sqlx::query_as(
        "SELECT t.user_id, t.scope FROM oauth_tokens t JOIN oauth_clients c ON c.id=t.client_id \
         WHERE t.token_hash=$1 AND t.kind='access' AND t.revoked_at IS NULL AND t.expires_at > now() AND c.enabled",
    )
    .bind(token_hash).fetch_optional(pool).await.map_err(db_err)
}

pub async fn revoke_token(pool: &PgPool, token_hash: &str, client: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE oauth_tokens SET revoked_at = now() WHERE token_hash=$1 AND client_id=$2 AND revoked_at IS NULL")
        .bind(token_hash).bind(client).execute(pool).await.map_err(db_err)?;
    Ok(())
}
