//! SAML 2.0 SP storage (migration 0289): IdPs, pending requests, replay cache
//! and IdP-session links for single logout.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SamlIdp {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub idp_entity_id: String,
    pub sso_url: String,
    pub slo_url: String,
    pub idp_certs: Vec<String>,
    pub metadata_url: String,
    pub sp_entity_id: String,
    pub sign_requests: bool,
    #[serde(skip)]
    pub sp_key_sealed: Option<Vec<u8>>,
    pub sp_cert_pem: Option<String>,
    pub want_assertions_signed: bool,
    pub allow_idp_initiated: bool,
    pub clock_skew_secs: i32,
    pub attr_email: String,
    pub attr_name: String,
    pub attr_first_name: String,
    pub attr_last_name: String,
    pub attr_groups: String,
    pub group_map: Value,
    pub role_map: Value,
    pub nameid_is_email: bool,
    pub allow_register: bool,
    pub link_by_verified_email: bool,
    pub allow_staff: bool,
    pub button_label: String,
    pub last_test: Option<Value>,
    pub last_test_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Everything an admin can set (the SP key is managed separately).
#[derive(Debug, Clone)]
pub struct IdpInput {
    pub name: String,
    pub enabled: bool,
    pub idp_entity_id: String,
    pub sso_url: String,
    pub slo_url: String,
    pub idp_certs: Vec<String>,
    pub metadata_url: String,
    pub sp_entity_id: String,
    pub sign_requests: bool,
    pub want_assertions_signed: bool,
    pub allow_idp_initiated: bool,
    pub clock_skew_secs: i32,
    pub attr_email: String,
    pub attr_name: String,
    pub attr_first_name: String,
    pub attr_last_name: String,
    pub attr_groups: String,
    pub group_map: Value,
    pub role_map: Value,
    pub nameid_is_email: bool,
    pub allow_register: bool,
    pub link_by_verified_email: bool,
    pub allow_staff: bool,
    pub button_label: String,
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<SamlIdp>> {
    sqlx::query_as("SELECT * FROM saml_idps ORDER BY name").fetch_all(pool).await.map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<SamlIdp>> {
    sqlx::query_as("SELECT * FROM saml_idps WHERE id = $1").bind(id).fetch_optional(pool).await.map_err(db_err)
}

pub async fn save(pool: &PgPool, id: Option<Uuid>, i: &IdpInput) -> AppResult<Option<Uuid>> {
    let id_v = id.unwrap_or_else(Uuid::new_v4);
    let sql = if id.is_some() {
        "UPDATE saml_idps SET name=$2, enabled=$3, idp_entity_id=$4, sso_url=$5, slo_url=$6, idp_certs=$7, metadata_url=$8, sp_entity_id=$9,
            sign_requests=$10, want_assertions_signed=$11, allow_idp_initiated=$12, clock_skew_secs=$13, attr_email=$14, attr_name=$15,
            attr_first_name=$16, attr_last_name=$17, attr_groups=$18, group_map=$19, role_map=$20, nameid_is_email=$21, allow_register=$22,
            link_by_verified_email=$23, allow_staff=$24, button_label=$25, updated_at=now() WHERE id=$1 RETURNING id"
    } else {
        "INSERT INTO saml_idps (id, name, enabled, idp_entity_id, sso_url, slo_url, idp_certs, metadata_url, sp_entity_id, sign_requests,
            want_assertions_signed, allow_idp_initiated, clock_skew_secs, attr_email, attr_name, attr_first_name, attr_last_name, attr_groups,
            group_map, role_map, nameid_is_email, allow_register, link_by_verified_email, allow_staff, button_label)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25) RETURNING id"
    };
    sqlx::query_scalar(sql)
        .bind(id_v)
        .bind(&i.name)
        .bind(i.enabled)
        .bind(&i.idp_entity_id)
        .bind(&i.sso_url)
        .bind(&i.slo_url)
        .bind(&i.idp_certs)
        .bind(&i.metadata_url)
        .bind(&i.sp_entity_id)
        .bind(i.sign_requests)
        .bind(i.want_assertions_signed)
        .bind(i.allow_idp_initiated)
        .bind(i.clock_skew_secs)
        .bind(&i.attr_email)
        .bind(&i.attr_name)
        .bind(&i.attr_first_name)
        .bind(&i.attr_last_name)
        .bind(&i.attr_groups)
        .bind(&i.group_map)
        .bind(&i.role_map)
        .bind(i.nameid_is_email)
        .bind(i.allow_register)
        .bind(i.link_by_verified_email)
        .bind(i.allow_staff)
        .bind(&i.button_label)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn set_sp_key(pool: &PgPool, id: Uuid, sealed: &[u8], cert_pem: &str) -> AppResult<()> {
    sqlx::query("UPDATE saml_idps SET sp_key_sealed=$2, sp_cert_pem=$3, updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(sealed)
        .bind(cert_pem)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<bool> {
    // Linked identities use provider "saml:{id}".
    sqlx::query("DELETE FROM user_identities WHERE provider = $1").bind(format!("saml:{id}")).execute(pool).await.map_err(db_err)?;
    Ok(sqlx::query("DELETE FROM saml_idps WHERE id=$1").bind(id).execute(pool).await.map_err(db_err)?.rows_affected() == 1)
}

pub async fn put_request(pool: &PgPool, id: &str, idp: Uuid, mode: &str, user_id: Option<Uuid>) -> AppResult<()> {
    sqlx::query("DELETE FROM saml_requests WHERE expires_at < now()").execute(pool).await.map_err(db_err)?;
    sqlx::query("INSERT INTO saml_requests (id, idp_id, mode, user_id, expires_at) VALUES ($1,$2,$3,$4, now() + interval '10 minutes')")
        .bind(id)
        .bind(idp)
        .bind(mode)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// One-time: (mode, user_id) of a live pending request for this IdP.
pub async fn take_request(pool: &PgPool, id: &str, idp: Uuid) -> AppResult<Option<(String, Option<Uuid>)>> {
    sqlx::query_as("DELETE FROM saml_requests WHERE id=$1 AND idp_id=$2 AND expires_at > now() RETURNING mode, user_id")
        .bind(id)
        .bind(idp)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Record an assertion ID; false when it was already seen (replay).
pub async fn remember_assertion(pool: &PgPool, idp: Uuid, assertion_id: &str, until: OffsetDateTime) -> AppResult<bool> {
    sqlx::query("DELETE FROM saml_replay WHERE expires_at < now() - interval '1 hour'").execute(pool).await.map_err(db_err)?;
    let r = sqlx::query("INSERT INTO saml_replay (idp_id, assertion_id, expires_at) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(idp)
        .bind(assertion_id)
        .bind(until)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn link_session(pool: &PgPool, session: Uuid, idp: Uuid, user: Uuid, name_id: &str, session_index: Option<&str>) -> AppResult<()> {
    sqlx::query("INSERT INTO saml_sessions (session_id, idp_id, user_id, name_id, session_index) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (session_id) DO NOTHING")
        .bind(session)
        .bind(idp)
        .bind(user)
        .bind(name_id)
        .bind(session_index)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct SamlSession {
    pub idp_id: Uuid,
    pub name_id: String,
    pub session_index: Option<String>,
}

pub async fn session(pool: &PgPool, session: Uuid) -> AppResult<Option<SamlSession>> {
    sqlx::query_as("SELECT idp_id, name_id, session_index FROM saml_sessions WHERE session_id=$1").bind(session).fetch_optional(pool).await.map_err(db_err)
}

/// IdP-initiated logout: revoke every session that came from this NameID.
pub async fn revoke_by_name_id(pool: &PgPool, idp: Uuid, name_id: &str) -> AppResult<u64> {
    let r = sqlx::query(
        "UPDATE sessions SET revoked_at = now() WHERE revoked_at IS NULL AND id IN (SELECT session_id FROM saml_sessions WHERE idp_id=$1 AND name_id=$2)",
    )
    .bind(idp)
    .bind(name_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    sqlx::query("DELETE FROM saml_sessions WHERE idp_id=$1 AND name_id=$2").bind(idp).bind(name_id).execute(pool).await.map_err(db_err)?;
    Ok(r.rows_affected())
}

/// Add a role by name (no-op when missing or already granted).
pub async fn grant_role(pool: &PgPool, user: Uuid, role: &str) -> AppResult<bool> {
    let r = sqlx::query("INSERT INTO user_roles (user_id, role_id) SELECT $1, id FROM roles WHERE name = $2 ON CONFLICT DO NOTHING")
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn set_last_test(pool: &PgPool, id: Uuid, result: &Value) -> AppResult<()> {
    sqlx::query("UPDATE saml_idps SET last_test=$2, last_test_at=now() WHERE id=$1").bind(id).bind(result).execute(pool).await.map_err(db_err)?;
    Ok(())
}
