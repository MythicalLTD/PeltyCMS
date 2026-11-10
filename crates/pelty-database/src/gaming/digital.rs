//! Digital products (module `digital_products`): versioned files with a
//! changelog, short-lived download links, software license keys with
//! activation limits and domain/IP/HWID binding, and external asset grants
//! (e.g. FiveM escrow) recorded for a hook to fulfil.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Unambiguous upper-case alphabet (no 0/O/1/I).
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Uses allowed per download link (retries, download managers).
pub const LINK_MAX_USES: i32 = 5;
pub const LICENSE_STATUSES: &[&str] = &["active", "suspended", "revoked"];

/// `XXXXX-XXXXX-XXXXX-XXXXX-XXXXX` (125 bits).
pub fn generate_key() -> String {
    let mut r = rand::rng();
    (0..5)
        .map(|_| {
            (0..5)
                .map(|_| ALPHABET[r.random_range(0..ALPHABET.len())] as char)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Keys compare case- and dash-insensitively.
pub fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

pub fn key_hash(key: &str) -> Vec<u8> {
    Sha256::digest(normalize_key(key).as_bytes()).to_vec()
}

pub fn key_hint(key: &str) -> String {
    let n = normalize_key(key);
    format!("…{}", &n[n.len().saturating_sub(5)..])
}

/// `https://www.Example.com:8080/x` → `example.com`.
pub fn normalize_domain(d: &str) -> String {
    let d = d.trim().to_ascii_lowercase();
    let d = d.split("://").last().unwrap_or("");
    let d = d.split(['/', '?', '#']).next().unwrap_or("");
    let d = d
        .rsplit_once(':')
        .filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit()))
        .map_or(d, |(h, _)| h);
    d.trim_start_matches("www.")
        .trim_end_matches('.')
        .to_string()
}

pub fn version_ok(v: &str) -> bool {
    let b = v.as_bytes();
    !b.is_empty()
        && b.len() <= 40
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b".+_-".contains(c))
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Settings {
    #[serde(default)]
    pub product_id: Uuid,
    pub license_enabled: bool,
    /// 0 = unlimited.
    pub max_activations: i32,
    pub bind_domain: bool,
    pub bind_ip: bool,
    pub bind_hwid: bool,
    /// Days a license is valid; empty = as long as the service.
    pub license_days: Option<i32>,
    pub link_ttl_secs: i32,
    /// External asset id granted on purchase (e.g. a FiveM escrow resource).
    #[serde(default)]
    pub escrow_asset: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            product_id: Uuid::nil(),
            license_enabled: true,
            max_activations: 1,
            bind_domain: false,
            bind_ip: false,
            bind_hwid: false,
            license_days: None,
            link_ttl_secs: 900,
            escrow_asset: String::new(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        if !(0..=1000).contains(&self.max_activations)
            || !(60..=86400).contains(&self.link_ttl_secs)
            || self.license_days.is_some_and(|d| !(1..=36500).contains(&d))
            || self.escrow_asset.len() > 200
            || self.escrow_asset.chars().any(char::is_control)
        {
            return Err(AppError::Validation("error.digital_settings_invalid"));
        }
        Ok(())
    }
}

const SETTINGS_COLS: &str = "product_id, license_enabled, max_activations, bind_domain, bind_ip, bind_hwid, license_days, link_ttl_secs, escrow_asset";

pub async fn settings<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    product: Uuid,
) -> AppResult<Option<Settings>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SETTINGS_COLS} FROM digital_products WHERE product_id = $1"
    )))
    .bind(product)
    .fetch_optional(ex)
    .await
    .map_err(db_err)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ProductRow {
    pub product_id: Uuid,
    pub name: String,
    pub license_enabled: bool,
    pub max_activations: i32,
    pub versions: i64,
    pub licenses: i64,
    pub latest: Option<String>,
}

pub async fn products(pool: &PgPool) -> AppResult<Vec<ProductRow>> {
    sqlx::query_as(
        "SELECT d.product_id, p.name, d.license_enabled, d.max_activations,
                (SELECT count(*) FROM digital_versions v WHERE v.product_id = d.product_id) AS versions,
                (SELECT count(*) FROM software_licenses l WHERE l.product_id = d.product_id) AS licenses,
                (SELECT v.version FROM digital_versions v WHERE v.product_id = d.product_id ORDER BY v.created_at DESC LIMIT 1) AS latest
         FROM digital_products d JOIN products p ON p.id = d.product_id ORDER BY p.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn save_settings(pool: &PgPool, product: Uuid, s: &Settings) -> AppResult<Settings> {
    s.validate()?;
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO digital_products ({SETTINGS_COLS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT (product_id) DO UPDATE SET license_enabled = $2, max_activations = $3, bind_domain = $4, bind_ip = $5,
           bind_hwid = $6, license_days = $7, link_ttl_secs = $8, escrow_asset = $9, updated_at = now()
         RETURNING {SETTINGS_COLS}"
    )))
    .bind(product)
    .bind(s.license_enabled)
    .bind(s.max_activations)
    .bind(s.bind_domain)
    .bind(s.bind_ip)
    .bind(s.bind_hwid)
    .bind(s.license_days)
    .bind(s.link_ttl_secs)
    .bind(s.escrow_asset.trim())
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_foreign_key_violation() => AppError::NotFound,
        _ => db_err(e),
    })
}

pub async fn delete_settings(pool: &PgPool, product: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM digital_products WHERE product_id = $1")
        .bind(product)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

// ---------------------------------------------------------------- versions

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Version {
    pub id: Uuid,
    pub product_id: Uuid,
    pub version: String,
    pub changelog: String,
    pub filename: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub downloads: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const VERSION_COLS: &str = "id, product_id, version, changelog, filename, size_bytes, encode(sha256, 'hex') AS sha256, downloads, created_at";

pub struct NewVersion<'a> {
    pub product_id: Uuid,
    pub version: &'a str,
    pub changelog: &'a str,
    pub filename: &'a str,
    pub content_type: &'a str,
    pub data: &'a [u8],
}

pub async fn add_version(pool: &PgPool, n: NewVersion<'_>) -> AppResult<Version> {
    if !version_ok(n.version) || n.changelog.len() > 20000 {
        return Err(AppError::Validation("error.digital_version_invalid"));
    }
    if settings(pool, n.product_id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO digital_versions (id, product_id, version, changelog, filename, content_type, size_bytes, sha256, data)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {VERSION_COLS}"
    )))
    .bind(Uuid::new_v4())
    .bind(n.product_id)
    .bind(n.version)
    .bind(n.changelog)
    .bind(n.filename)
    .bind(n.content_type)
    .bind(n.data.len() as i64)
    .bind(Sha256::digest(n.data).to_vec())
    .bind(n.data)
    .fetch_one(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.digital_version_exists"),
        _ => db_err(e),
    })
}

pub async fn versions(pool: &PgPool, product: Uuid) -> AppResult<Vec<Version>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {VERSION_COLS} FROM digital_versions WHERE product_id = $1 ORDER BY created_at DESC"
    )))
    .bind(product)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn delete_version(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM digital_versions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Products the customer may download: an active service of the product or an
/// active, unexpired license for it.
const ENTITLED: &str = "(EXISTS (SELECT 1 FROM services s WHERE s.customer_id = $1 AND s.product_id = dp.product_id AND s.status = 'active')
     OR EXISTS (SELECT 1 FROM software_licenses l WHERE l.customer_id = $1 AND l.product_id = dp.product_id AND l.status = 'active'
                AND (l.expires_at IS NULL OR l.expires_at > now())))";

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Owned {
    pub product_id: Uuid,
    pub name: String,
}

pub async fn owned(pool: &PgPool, customer: Uuid) -> AppResult<Vec<Owned>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT dp.product_id, p.name FROM digital_products dp JOIN products p ON p.id = dp.product_id WHERE {ENTITLED} ORDER BY p.name"
    )))
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn entitled(pool: &PgPool, customer: Uuid, version: Uuid) -> AppResult<bool> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS (SELECT 1 FROM digital_versions v JOIN digital_products dp ON dp.product_id = v.product_id WHERE v.id = $2 AND {ENTITLED})"
    )))
    .bind(customer)
    .bind(version)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Stores a link; returns its expiry. The caller hands out the raw token.
pub async fn create_link(
    pool: &PgPool,
    token_hash: &[u8],
    version: Uuid,
    customer: Uuid,
) -> AppResult<OffsetDateTime> {
    sqlx::query_scalar(
        "INSERT INTO digital_download_links (token_hash, version_id, customer_id, expires_at)
         SELECT $1, v.id, $3, now() + make_interval(secs => dp.link_ttl_secs)
         FROM digital_versions v JOIN digital_products dp ON dp.product_id = v.product_id WHERE v.id = $2
         RETURNING expires_at",
    )
    .bind(token_hash)
    .bind(version)
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Redeems a link: (filename, content type, data). Expired, over-used or
/// no-longer-entitled links are 404.
pub async fn redeem_link(pool: &PgPool, token_hash: &[u8]) -> AppResult<(String, String, Vec<u8>)> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let link: Option<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE digital_download_links SET uses = uses + 1
         WHERE token_hash = $1 AND expires_at > now() AND uses < $2 RETURNING version_id, customer_id",
    )
    .bind(token_hash)
    .bind(LINK_MAX_USES)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let (version, customer) = link.ok_or(AppError::NotFound)?;
    let ok: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS (SELECT 1 FROM digital_versions v JOIN digital_products dp ON dp.product_id = v.product_id WHERE v.id = $2 AND {ENTITLED})"
    )))
    .bind(customer)
    .bind(version)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if !ok {
        return Err(AppError::NotFound);
    }
    let file: (String, String, Vec<u8>) = sqlx::query_as(
        "UPDATE digital_versions SET downloads = downloads + 1 WHERE id = $1 RETURNING filename, content_type, data",
    )
    .bind(version)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(file)
}

pub async fn purge_links(pool: &PgPool) -> AppResult<u64> {
    Ok(sqlx::query(
        "DELETE FROM digital_download_links WHERE expires_at < now() - interval '1 day'",
    )
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected())
}

// ---------------------------------------------------------------- licenses

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct License {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product: String,
    pub customer_id: Uuid,
    pub service_id: Option<Uuid>,
    pub key_hint: String,
    pub status: String,
    pub max_activations: i32,
    pub activations: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub note: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(skip)]
    pub key_sealed: Vec<u8>,
}

const LICENSE_COLS: &str = "l.id, l.product_id, p.name AS product, l.customer_id, l.service_id, l.key_hint, l.status, l.max_activations,
    (SELECT count(*) FROM license_activations a WHERE a.license_id = l.id) AS activations, l.expires_at, l.note, l.created_at, l.key_sealed";

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct LicenseFilter {
    pub product_id: Option<Uuid>,
    pub customer_id: Option<Uuid>,
    pub status: Option<String>,
}

pub async fn licenses(pool: &PgPool, f: &LicenseFilter, limit: i64) -> AppResult<Vec<License>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LICENSE_COLS} FROM software_licenses l JOIN products p ON p.id = l.product_id
         WHERE ($1::uuid IS NULL OR l.product_id = $1) AND ($2::uuid IS NULL OR l.customer_id = $2) AND ($3::text IS NULL OR l.status = $3)
         ORDER BY l.created_at DESC LIMIT $4"
    )))
    .bind(f.product_id)
    .bind(f.customer_id)
    .bind(f.status.as_deref())
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn license(pool: &PgPool, id: Uuid) -> AppResult<License> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LICENSE_COLS} FROM software_licenses l JOIN products p ON p.id = l.product_id WHERE l.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub struct NewLicense<'a> {
    pub product_id: Uuid,
    pub customer_id: Uuid,
    pub service_id: Option<Uuid>,
    pub key: &'a str,
    pub key_sealed: &'a [u8],
    pub max_activations: i32,
    pub expires_at: Option<OffsetDateTime>,
    pub note: &'a str,
}

/// Issues a license; `None` when the service already has one (idempotent).
pub async fn issue(pool: &PgPool, n: NewLicense<'_>) -> AppResult<Option<Uuid>> {
    if !(0..=1000).contains(&n.max_activations) || n.note.len() > 500 {
        return Err(AppError::Validation("error.digital_settings_invalid"));
    }
    sqlx::query_scalar(
        "INSERT INTO software_licenses (id, product_id, customer_id, service_id, key_hash, key_sealed, key_hint, max_activations, expires_at, note)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT (service_id) DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(n.product_id)
    .bind(n.customer_id)
    .bind(n.service_id)
    .bind(key_hash(n.key))
    .bind(n.key_sealed)
    .bind(key_hint(n.key))
    .bind(n.max_activations)
    .bind(n.expires_at)
    .bind(n.note)
    .fetch_optional(pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_foreign_key_violation() => AppError::NotFound,
        _ => db_err(e),
    })
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> AppResult<()> {
    if !LICENSE_STATUSES.contains(&status) {
        return Err(AppError::Validation("error.digital_status_invalid"));
    }
    let r =
        sqlx::query("UPDATE software_licenses SET status = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(pool)
            .await
            .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Lifecycle change for a service's license. Revoked licenses stay revoked.
pub async fn set_status_for_service(pool: &PgPool, service: Uuid, status: &str) -> AppResult<u64> {
    Ok(sqlx::query("UPDATE software_licenses SET status = $2, updated_at = now() WHERE service_id = $1 AND status <> 'revoked' AND status <> $2")
        .bind(service)
        .bind(status)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected())
}

pub async fn reset_activations(pool: &PgPool, id: Uuid, customer: Option<Uuid>) -> AppResult<u64> {
    let owns: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM software_licenses WHERE id = $1 AND ($2::uuid IS NULL OR customer_id = $2))")
        .bind(id)
        .bind(customer)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if !owns {
        return Err(AppError::NotFound);
    }
    Ok(
        sqlx::query("DELETE FROM license_activations WHERE license_id = $1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected(),
    )
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Activation {
    pub id: Uuid,
    pub domain: String,
    pub ip: String,
    pub hwid: String,
    #[serde(with = "time::serde::rfc3339")]
    pub first_seen: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen: OffsetDateTime,
}

pub async fn activations(pool: &PgPool, license: Uuid) -> AppResult<Vec<Activation>> {
    sqlx::query_as("SELECT id, domain, ip, hwid, first_seen, last_seen FROM license_activations WHERE license_id = $1 ORDER BY first_seen")
        .bind(license)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// What a product's checker sends; only the fields the product binds count.
#[derive(Debug, Default, Clone, Deserialize, utoipa::ToSchema)]
pub struct Check {
    pub key: String,
    #[serde(default)]
    pub product_id: Option<Uuid>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub hwid: Option<String>,
    /// Register a new activation when there is room (default true).
    #[serde(default = "yes")]
    pub activate: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Verdict {
    pub valid: bool,
    /// Why it is not valid: not_found, wrong_product, suspended, revoked,
    /// expired, domain_required, ip_required, hwid_required, activation_limit,
    /// not_activated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<OffsetDateTime>,
    pub activations: i64,
    pub max_activations: i32,
}

impl Verdict {
    fn no(reason: &'static str) -> Self {
        Self {
            valid: false,
            reason: Some(reason),
            product_id: None,
            status: None,
            expires_at: None,
            activations: 0,
            max_activations: 0,
        }
    }
}

/// Bound identity → (fingerprint, domain, ip, hwid); `Err(reason)` when a
/// bound field is missing.
pub fn fingerprint(
    s: &Settings,
    c: &Check,
    fallback_ip: Option<&str>,
) -> Result<(String, String, String, String), &'static str> {
    let clean = |v: Option<&str>| {
        v.map(|x| {
            x.trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect::<String>()
        })
        .unwrap_or_default()
    };
    let domain = if s.bind_domain {
        normalize_domain(&clean(c.domain.as_deref()))
    } else {
        String::new()
    };
    let ip = if s.bind_ip {
        clean(c.ip.as_deref().or(fallback_ip))
    } else {
        String::new()
    };
    let hwid = if s.bind_hwid {
        clean(c.hwid.as_deref())
    } else {
        String::new()
    };
    if s.bind_domain && domain.is_empty() {
        return Err("domain_required");
    }
    if s.bind_ip && ip.is_empty() {
        return Err("ip_required");
    }
    if s.bind_hwid && hwid.is_empty() {
        return Err("hwid_required");
    }
    let fp = if s.bind_domain || s.bind_ip || s.bind_hwid {
        format!("d={domain}|i={ip}|h={hwid}")
    } else {
        String::new()
    };
    Ok((fp, domain, ip, hwid))
}

/// (license, product, status, max activations, expiry, linked service status).
type LicenseCheckRow = (
    Uuid,
    Uuid,
    String,
    i32,
    Option<OffsetDateTime>,
    Option<String>,
);

/// Validates a key and (optionally) records an activation, atomically per license.
pub async fn validate(pool: &PgPool, c: &Check, fallback_ip: Option<&str>) -> AppResult<Verdict> {
    if normalize_key(&c.key).len() < 10 {
        return Ok(Verdict::no("not_found"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    // The linked service's state is read too, so a license whose service was
    // terminated or cancelled is revoked here even if the revocation event
    // was missed (module off at the time, or a failed hook).
    let row: Option<LicenseCheckRow> = sqlx::query_as(
        "SELECT l.id, l.product_id, l.status, l.max_activations, l.expires_at, sv.status
           FROM software_licenses l LEFT JOIN services sv ON sv.id = l.service_id
          WHERE l.key_hash = $1 FOR UPDATE OF l",
    )
    .bind(key_hash(&c.key))
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some((id, product, mut status, max, expires, service)) = row else {
        return Ok(Verdict::no("not_found"));
    };
    if status != "revoked"
        && matches!(
            service.as_deref(),
            Some("terminated" | "cancelled" | "fraud")
        )
    {
        sqlx::query("UPDATE software_licenses SET status = 'revoked' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        tx = pool.begin().await.map_err(db_err)?;
        status = "revoked".into();
    } else if status == "active" && service.as_deref() == Some("suspended") {
        status = "suspended".into();
    }
    let activations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM license_activations WHERE license_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let mut v = Verdict {
        valid: false,
        reason: None,
        product_id: Some(product),
        status: Some(status.clone()),
        expires_at: expires,
        activations,
        max_activations: max,
    };
    let fail = |mut v: Verdict, r: &'static str| {
        v.reason = Some(r);
        Ok(v)
    };
    if c.product_id.is_some_and(|p| p != product) {
        return fail(Verdict::no("wrong_product"), "wrong_product");
    }
    if status != "active" {
        return fail(
            v,
            if status == "revoked" {
                "revoked"
            } else {
                "suspended"
            },
        );
    }
    if expires.is_some_and(|e| e <= OffsetDateTime::now_utc()) {
        return fail(v, "expired");
    }
    // Read on the transaction's connection: a second pool connection while
    // the row lock is held could exhaust the pool under concurrent checks.
    let Some(s) = settings(&mut *tx, product)
        .await?
        .filter(|s| s.license_enabled)
    else {
        return fail(v, "license_disabled");
    };
    let (fp, domain, ip, hwid) = match fingerprint(&s, c, fallback_ip) {
        Ok(x) => x,
        Err(r) => return fail(v, r),
    };
    if fp.is_empty() {
        v.valid = true;
        return Ok(v);
    }
    let seen = sqlx::query("UPDATE license_activations SET last_seen = now() WHERE license_id = $1 AND fingerprint = $2")
        .bind(id)
        .bind(&fp)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected()
        > 0;
    if !seen {
        if !c.activate {
            return fail(v, "not_activated");
        }
        if max > 0 && v.activations >= i64::from(max) {
            return fail(v, "activation_limit");
        }
        sqlx::query("INSERT INTO license_activations (id, license_id, fingerprint, domain, ip, hwid) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(&fp)
            .bind(&domain)
            .bind(&ip)
            .bind(&hwid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        v.activations += 1;
    }
    tx.commit().await.map_err(db_err)?;
    v.valid = true;
    Ok(v)
}

/// Frees the activation matching the bound identity. Returns whether one was removed.
pub async fn deactivate(pool: &PgPool, c: &Check, fallback_ip: Option<&str>) -> AppResult<bool> {
    let row: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT id, product_id FROM software_licenses WHERE key_hash = $1")
            .bind(key_hash(&c.key))
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let Some((id, product)) = row else {
        return Ok(false);
    };
    let Some(s) = settings(pool, product).await?.filter(|s| s.license_enabled) else {
        return Ok(false);
    };
    let Ok((fp, ..)) = fingerprint(&s, c, fallback_ip) else {
        return Ok(false);
    };
    Ok(
        sqlx::query("DELETE FROM license_activations WHERE license_id = $1 AND fingerprint = $2")
            .bind(id)
            .bind(fp)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected()
            > 0,
    )
}

// ---------------------------------------------------------------- asset grants

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Grant {
    pub id: Uuid,
    pub product_id: Uuid,
    pub customer_id: Uuid,
    pub service_id: Option<Uuid>,
    pub asset: String,
    pub identity: String,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn add_grant(
    pool: &PgPool,
    product: Uuid,
    customer: Uuid,
    service: Uuid,
    asset: &str,
    identity: &str,
) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "INSERT INTO digital_asset_grants (id, product_id, customer_id, service_id, asset, identity) VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT (service_id, asset) DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(product)
    .bind(customer)
    .bind(service)
    .bind(asset)
    .bind(identity)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Marks grants of a service revoked; returns the revoked rows.
pub async fn revoke_grants(pool: &PgPool, service: Uuid) -> AppResult<Vec<Grant>> {
    sqlx::query_as(
        "UPDATE digital_asset_grants SET status = 'revoked', updated_at = now() WHERE service_id = $1 AND status <> 'revoked'
         RETURNING id, product_id, customer_id, service_id, asset, identity, status, created_at",
    )
    .bind(service)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn grants(pool: &PgPool, limit: i64) -> AppResult<Vec<Grant>> {
    sqlx::query_as("SELECT id, product_id, customer_id, service_id, asset, identity, status, created_at FROM digital_asset_grants ORDER BY created_at DESC LIMIT $1")
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Staff (or an integration) confirms an external grant happened.
pub async fn mark_granted(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("UPDATE digital_asset_grants SET status = 'granted', updated_at = now() WHERE id = $1 AND status = 'pending'")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict("error.digital_grant_state"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let k = generate_key();
        assert_eq!(k.len(), 29);
        assert_eq!(key_hash(&k), key_hash(&k.to_lowercase().replace('-', "")));
        assert!(key_hint(&k).ends_with(&k[24..]));
        assert_ne!(generate_key(), k);
    }

    #[test]
    fn domains_and_versions() {
        assert_eq!(
            normalize_domain("https://WWW.Example.com:8080/path?q"),
            "example.com"
        );
        assert_eq!(normalize_domain("shop.example.org."), "shop.example.org");
        assert!(version_ok("1.2.3-beta+5"));
        assert!(!version_ok("../x"));
        assert!(!version_ok(""));
    }

    #[test]
    fn fingerprints_only_bound_fields() {
        let mut s = Settings::default();
        let c = Check {
            key: "x".into(),
            domain: Some("www.a.com".into()),
            ..Default::default()
        };
        assert_eq!(fingerprint(&s, &c, Some("1.2.3.4")).unwrap().0, "");
        s.bind_domain = true;
        assert_eq!(fingerprint(&s, &c, None).unwrap().0, "d=a.com|i=|h=");
        s.bind_hwid = true;
        assert_eq!(fingerprint(&s, &c, None), Err("hwid_required"));
        s.bind_hwid = false;
        s.bind_ip = true;
        assert_eq!(fingerprint(&s, &c, Some("9.9.9.9")).unwrap().2, "9.9.9.9");
    }

    #[test]
    fn settings_bounds() {
        let mut s = Settings::default();
        assert!(s.validate().is_ok());
        s.link_ttl_secs = 10;
        assert!(s.validate().is_err());
    }
}
