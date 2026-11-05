//! Personal API keys. Creating, listing and revoking keys requires an
//! interactive session (keys cannot manage keys). The secret is shown once.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::extract::{CurrentUser, API_KEY_PREFIX};
use crate::AppState;
use axum::extract::Query;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{api_keys, audit};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const MAX_KEYS: i64 = 25;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api-keys", get(list).post(create))
        .route("/api-keys/{id}", delete(revoke).patch(update))
        .route("/admin/api-keys", get(admin_list))
        .route(
            "/admin/api-keys/{id}",
            delete(admin_revoke).patch(admin_update),
        )
        .route(
            "/admin/users/{id}/api-keys/revoke-all",
            post(admin_revoke_all),
        )
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateKey {
    pub name: String,
    /// Permission names; each must be one the caller holds.
    pub scopes: Vec<String>,
    /// 1-3650 days; omit for no expiry.
    #[serde(default)]
    pub expires_in_days: Option<i64>,
    /// IP addresses or CIDR ranges (max 20); empty or omitted = any IP.
    #[serde(default)]
    pub allowed_ips: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateKey {
    /// Replace the IP allowlist (`[]` = any IP); omit to keep it.
    #[serde(default)]
    pub allowed_ips: Option<Vec<String>>,
    /// New expiry in days from now (1-3650), `0` to clear it; omit to keep.
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

pub const MAX_ALLOWED_IPS: usize = 20;

/// Validate and normalise an allowlist: each entry is an IP address or a
/// CIDR range (`203.0.113.0/24`, `2001:db8::/32`); sorted, deduplicated.
pub fn normalize_ips(list: &[String]) -> Result<Vec<String>, AppError> {
    let mut out = Vec::new();
    for raw in list {
        let e = raw.trim();
        if e.is_empty() {
            continue;
        }
        let (addr, len) = match e.split_once('/') {
            Some((a, l)) => (a, Some(l)),
            None => (e, None),
        };
        let ip: std::net::IpAddr = addr
            .parse()
            .map_err(|_| AppError::Validation("error.invalid_ip_allowlist"))?;
        let max = if ip.is_ipv4() { 32 } else { 128 };
        let norm = match len {
            None => ip.to_string(),
            Some(l) => {
                let n: u8 = l
                    .parse()
                    .map_err(|_| AppError::Validation("error.invalid_ip_allowlist"))?;
                if n > max {
                    return Err(AppError::Validation("error.invalid_ip_allowlist"));
                }
                format!("{ip}/{n}")
            }
        };
        out.push(norm);
    }
    out.sort();
    out.dedup();
    if out.len() > MAX_ALLOWED_IPS {
        return Err(AppError::Validation("error.invalid_ip_allowlist"));
    }
    Ok(out)
}

fn expiry(days: Option<i64>) -> Result<Option<OffsetDateTime>, AppError> {
    match days {
        None => Ok(None),
        Some(d) if (1..=3650).contains(&d) => {
            Ok(Some(OffsetDateTime::now_utc() + Duration::days(d)))
        }
        Some(_) => Err(AppError::Validation("error.invalid_expiry")),
    }
}

fn session_only(u: &CurrentUser) -> Result<(), AppError> {
    if u.api_key {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/api-keys", responses((status = 200), (status = 401)))]
pub async fn list(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    session_only(&u)?;
    Ok(Json(
        json!({ "items": api_keys::list(&s.pool, u.user_id).await?, "available_scopes": u.permissions }),
    ))
}

#[utoipa::path(post, path = "/api/v1/api-keys", request_body = CreateKey, responses((status = 200), (status = 422)))]
pub async fn create(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<CreateKey>,
) -> ApiResult<Json<Value>> {
    session_only(&u)?;
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Validation("error.invalid_name").into());
    }
    let mut scopes: Vec<String> = b.scopes.iter().map(|x| x.trim().to_string()).collect();
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty() || scopes.iter().any(|x| !u.has(x)) {
        return Err(AppError::Validation("error.invalid_scopes").into());
    }
    let expires_at = expiry(b.expires_in_days)?;
    let allowed_ips = normalize_ips(&b.allowed_ips)?;
    if api_keys::count_active(&s.pool, u.user_id).await? >= MAX_KEYS {
        return Err(AppError::Conflict("error.too_many_keys").into());
    }
    let id_part = &pelty_auth::token::generate()[..10];
    let prefix = format!("{API_KEY_PREFIX}{id_part}");
    let secret = format!("{prefix}_{}", pelty_auth::token::generate());
    let key = api_keys::create(
        &s.pool,
        u.user_id,
        name,
        &prefix,
        &pelty_auth::token::hash(&secret),
        &scopes,
        &allowed_ips,
        expires_at,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "api_key.created",
        Some(&key.id.to_string()),
        u.ip.as_deref(),
        json!({ "scopes": scopes, "prefix": prefix, "allowed_ips": allowed_ips }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "api_key.created",
        json!({ "api_key_id": key.id, "user_id": u.user_id, "scopes": scopes }),
    ));
    Ok(Json(json!({ "key": key, "secret": secret })))
}

#[utoipa::path(delete, path = "/api/v1/api-keys/{id}", responses((status = 204), (status = 404)))]
pub async fn revoke(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    session_only(&u)?;
    if !api_keys::revoke(&s.pool, id, u.user_id, false).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "api_key.revoked",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "api_key.revoked",
        json!({ "api_key_id": id, "user_id": u.user_id, "by_staff": false }),
    ));
    Ok(StatusCode::NO_CONTENT)
}

async fn apply_update(
    s: &AppState,
    u: &CurrentUser,
    id: Uuid,
    owner: Option<Uuid>,
    b: UpdateKey,
) -> ApiResult<Json<Value>> {
    let ips = b.allowed_ips.as_deref().map(normalize_ips).transpose()?;
    let exp = match b.expires_in_days {
        None => None,
        Some(0) => Some(None),
        d => Some(expiry(d)?),
    };
    let key = api_keys::update(&s.pool, id, owner, ips.as_deref(), exp)
        .await?
        .ok_or(AppError::NotFound)?;
    let action = if owner.is_some() {
        "api_key.updated"
    } else {
        "api_key.updated_by_admin"
    };
    audit::record(&s.pool, Some(u.user_id), action, Some(&id.to_string()), u.ip.as_deref(), json!({ "allowed_ips": key.allowed_ips, "expires_at": key.expires_at.map(|t| t.unix_timestamp()) })).await?;
    Ok(Json(json!({ "key": key })))
}

/// Change one of your keys' IP allowlist or expiry (scopes stay fixed).
#[utoipa::path(patch, path = "/api/v1/api-keys/{id}", request_body = UpdateKey, responses((status = 200), (status = 404), (status = 422)))]
pub async fn update(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateKey>,
) -> ApiResult<Json<Value>> {
    session_only(&u)?;
    apply_update(&s, &u, id, Some(u.user_id), b).await
}

/// Staff: change any live key's IP allowlist or expiry.
#[utoipa::path(patch, path = "/api/v1/admin/api-keys/{id}", request_body = UpdateKey, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ApiKeysManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateKey>,
) -> ApiResult<Json<Value>> {
    session_only(&u)?;
    apply_update(&s, &u, id, None, b).await
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AdminKeyQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    /// Only keys that are neither revoked nor expired.
    #[serde(default)]
    pub active: bool,
    pub user_id: Option<Uuid>,
    /// Matches owner email, key name or exact prefix.
    pub q: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/api-keys", params(AdminKeyQuery), responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ApiKeysManage>,
    Query(q): Query<AdminKeyQuery>,
) -> ApiResult<Json<Value>> {
    session_only(&u)?;
    let (limit, offset, page, per) = crate::routes::Page {
        page: q.page,
        per_page: q.per_page,
    }
    .bounds();
    let term = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let (items, total) =
        api_keys::list_all(&s.pool, q.active, q.user_id, term, limit, offset).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(delete, path = "/api/v1/admin/api-keys/{id}", responses((status = 204), (status = 404)))]
pub async fn admin_revoke(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ApiKeysManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    session_only(&u)?;
    let owner = api_keys::owner(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !api_keys::revoke(&s.pool, id, u.user_id, true).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "api_key.revoked_by_admin",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "owner": owner }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "api_key.revoked",
        json!({ "api_key_id": id, "user_id": owner, "by_staff": true }),
    ));
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/api-keys/revoke-all", responses((status = 200), (status = 403)))]
pub async fn admin_revoke_all(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ApiKeysManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    session_only(&u)?;
    let n = api_keys::revoke_all(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "api_key.revoked_all_by_admin",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "revoked": n }),
    )
    .await?;
    Ok(Json(json!({ "revoked": n })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_allowlist_normalisation() {
        let v = |x: &[&str]| normalize_ips(&x.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            v(&[" 203.0.113.7 ", "10.0.0.0/8", "10.0.0.0/8", ""]).unwrap(),
            vec!["10.0.0.0/8", "203.0.113.7"]
        );
        assert_eq!(v(&["2001:DB8::/32"]).unwrap(), vec!["2001:db8::/32"]);
        assert!(v(&["10.0.0.0/33"]).is_err());
        assert!(v(&["example.com"]).is_err());
        assert!(v(&["1.2.3.4/x"]).is_err());
        let many: Vec<String> = (0..21).map(|i| format!("10.0.0.{i}")).collect();
        assert!(normalize_ips(&many).is_err());
    }
}
