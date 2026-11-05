//! Session authentication, CSRF and permission extractors.
use crate::error::ApiError;
use crate::AppState;
use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method};
use pelty_core::AppError;
use std::marker::PhantomData;
use std::net::SocketAddr;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "pelty_session";
pub const CSRF_HEADER: &str = "x-csrf-token";

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
}

pub fn client_ip(parts: &Parts) -> Option<String> {
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
}

/// An authenticated user. Mutating requests must carry a valid CSRF header.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub csrf_token: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub ip: Option<String>,
    /// Set when authenticated with an API key (then `session_id` is the key id).
    pub api_key: bool,
    /// Geo/proxy signals for fraud screening.
    pub signals: crate::trusted_proxy::RequestSignals,
}

impl CurrentUser {
    pub fn has(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }

    /// `perm` is held, or the request is a read (GET/HEAD) and the user
    /// holds the read level of a `<res>.manage` permission.
    pub fn permits(&self, perm: &str, method: &Method) -> bool {
        if self.has(perm) {
            return true;
        }
        match read_level(perm) {
            Some(r) if is_safe(method) => self.has(&r),
            _ => false,
        }
    }
}

/// Resources with a separate `<res>.read` permission (migration 0319).
/// `<res>.manage` implies `<res>.read`.
pub const READ_LEVEL_RESOURCES: &[&str] = &[
    "customers", "catalog", "orders", "invoices", "services", "tickets", "coupons",
    "affiliates", "content", "quotes", "downloads", "client_groups", "campaigns",
];

/// `customers.manage` -> `Some("customers.read")` for read-level resources.
pub fn read_level(perm: &str) -> Option<String> {
    let res = perm.strip_suffix(".manage")?;
    READ_LEVEL_RESOURCES.contains(&res).then(|| format!("{res}.read"))
}

pub fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Add the read levels implied by held `.manage` permissions (sorted, deduped).
pub fn expand_implied(perms: &mut Vec<String>) {
    let extra: Vec<String> = perms.iter().filter_map(|p| read_level(p)).collect();
    perms.extend(extra);
    perms.sort();
    perms.dedup();
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if let Some(u) = parts.extensions.get::<CurrentUser>() {
            return Ok(u.clone());
        }
        if let Some(key) = bearer(&parts.headers) {
            return api_key_user(parts, state, &key).await;
        }
        let tok = cookie(&parts.headers, SESSION_COOKIE).ok_or(AppError::Unauthenticated)?;
        let ctx = pelty_database::sessions::resolve(
            &state.pool,
            &pelty_auth::token::hash(&tok),
            state.auth.idle,
            time::Duration::seconds(60),
        )
        .await?
        .ok_or(AppError::Unauthenticated)?;
        let safe = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if !safe {
            let sent = parts
                .headers
                .get(CSRF_HEADER)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !pelty_auth::token::constant_eq(sent, &ctx.csrf_token) {
                return Err(AppError::Csrf.into());
            }
        }
        let mut permissions = ctx.permissions;
        expand_implied(&mut permissions);
        let cu = CurrentUser {
            user_id: ctx.user_id,
            session_id: ctx.session_id,
            csrf_token: ctx.csrf_token,
            roles: ctx.roles,
            permissions,
            ip: client_ip(parts),
            api_key: false,
            signals: parts.extensions.get().cloned().unwrap_or_default(),
        };
        parts.extensions.insert(cu.clone());
        Ok(cu)
    }
}

pub const API_KEY_PREFIX: &str = "pelty_";

fn bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let t = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))?.trim();
    t.starts_with(API_KEY_PREFIX).then(|| t.to_string())
}

/// Paths an API key may never use: sign-in, sessions, 2FA, password, avatar
/// and key management itself. Those need an interactive session.
fn key_forbidden(path: &str) -> bool {
    let p = path.strip_prefix("/api/v1").unwrap_or(path);
    p.starts_with("/auth/") || p.starts_with("/account/") || p.contains("/api-keys")
}

/// API-key auth. No cookie is involved, so CSRF does not apply.
async fn api_key_user(parts: &mut Parts, state: &AppState, key: &str) -> Result<CurrentUser, ApiError> {
    let path = parts
        .extensions
        .get::<axum::extract::OriginalUri>()
        .map(|u| u.path().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());
    if key_forbidden(&path) {
        return Err(AppError::Forbidden.into());
    }
    let ip = client_ip(parts);
    // Throttle key guessing: only failed lookups count against the IP.
    let rl_key = format!("apikey:{}", ip.as_deref().unwrap_or("unknown"));
    if state.auth_limiter.exceeded(&rl_key) {
        return Err(AppError::RateLimited.into());
    }
    let Some(ctx) = pelty_database::api_keys::resolve(&state.pool, &pelty_auth::token::hash(key), ip.as_deref()).await? else {
        state.auth_limiter.check(&rl_key);
        return Err(AppError::Unauthenticated.into());
    };
    if !ctx.ip_allowed {
        tracing::warn!(key = %ctx.key_id, ip = ?ip, "API key used from an IP outside its allowlist");
        let _ = pelty_database::audit::record(&state.pool, Some(ctx.user_id), "api_key.ip_denied", Some(&ctx.key_id.to_string()), ip.as_deref(), serde_json::json!({})).await;
        return Err(AppError::Denied("error.api_key_ip_denied").into());
    }
    // Key permissions = (owner permissions + implied read levels) ∩ scopes.
    let mut owner = ctx.owner_permissions;
    expand_implied(&mut owner);
    let permissions: Vec<String> = owner.into_iter().filter(|p| ctx.scopes.contains(p)).collect();
    let cu = CurrentUser {
        user_id: ctx.user_id,
        session_id: ctx.key_id,
        csrf_token: String::new(),
        roles: ctx.roles,
        permissions,
        ip,
        api_key: true,
        signals: parts.extensions.get().cloned().unwrap_or_default(),
    };
    parts.extensions.insert(cu.clone());
    Ok(cu)
}

/// Marker trait naming a permission for `RequirePermission`.
pub trait Permission {
    const NAME: &'static str;
}

/// Extractor that rejects with 403 unless the user has permission `P`.
pub struct RequirePermission<P: Permission>(pub CurrentUser, pub PhantomData<P>);

impl<P: Permission + Send + Sync> FromRequestParts<AppState> for RequirePermission<P> {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let u = CurrentUser::from_request_parts(parts, state).await?;
        if !u.permits(P::NAME, &parts.method) {
            return Err(AppError::Forbidden.into());
        }
        // Staff 2FA policy: staff without 2FA keep client access only.
        if P::NAME != "client.access" && u.has("admin.access") && pelty_database::staff::blocked_by_2fa(&state.pool, u.user_id).await? {
            return Err(AppError::Denied("error.mfa_required").into());
        }
        if P::NAME != "client.access" && u.has("admin.access") && !u.api_key {
            let pol = pelty_auth::policy::load(&state.pool).await?;
            let now = time::OffsetDateTime::now_utc();
            if let Some(days) = pol.password_max_age_days {
                let changed = pelty_database::users::password_changed_at(&state.pool, u.user_id).await?;
                if password_expired(changed, days, now) {
                    return Err(AppError::Denied("error.password_expired").into());
                }
            }
            if let Some(mins) = pol.admin_reauth_minutes {
                if is_sensitive(P::NAME, &parts.method) {
                    let last = pelty_database::sessions::last_auth_at(&state.pool, u.session_id).await?;
                    if !recently_authenticated(last, mins, now) {
                        return Err(AppError::Denied("error.reauth_required").into());
                    }
                }
            }
        }
        Ok(RequirePermission(u, PhantomData))
    }
}

/// Permissions whose writes need a recent full authentication (step-up).
pub const SENSITIVE_PERMS: &[&str] = &["settings.manage", "plugins.manage", "gateways.manage", "users.manage", "api_keys.manage", "servers.manage", "webhooks.manage"];

/// Writes guarded by a sensitive permission need step-up re-auth.
pub fn is_sensitive(perm: &str, method: &Method) -> bool {
    SENSITIVE_PERMS.contains(&perm) && !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

pub fn recently_authenticated(last: Option<time::OffsetDateTime>, minutes: i64, now: time::OffsetDateTime) -> bool {
    last.is_some_and(|t| now - t <= time::Duration::minutes(minutes))
}

pub fn password_expired(changed: Option<time::OffsetDateTime>, days: i64, now: time::OffsetDateTime) -> bool {
    changed.is_some_and(|t| now - t > time::Duration::days(days))
}

#[cfg(test)]
mod step_up_tests {
    use super::*;
    #[test]
    fn step_up_rules() {
        assert!(is_sensitive("settings.manage", &Method::PUT));
        assert!(is_sensitive("plugins.manage", &Method::POST));
        assert!(!is_sensitive("settings.manage", &Method::GET));
        assert!(!is_sensitive("tickets.manage", &Method::POST));
        let now = time::OffsetDateTime::now_utc();
        assert!(recently_authenticated(Some(now - time::Duration::minutes(5)), 15, now));
        assert!(!recently_authenticated(Some(now - time::Duration::minutes(16)), 15, now));
        assert!(!recently_authenticated(None, 15, now));
        assert!(password_expired(Some(now - time::Duration::days(91)), 90, now));
        assert!(!password_expired(Some(now - time::Duration::days(10)), 90, now));
        assert!(!password_expired(None, 90, now));
    }

    fn user(perms: &[&str]) -> CurrentUser {
        let mut permissions: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
        expand_implied(&mut permissions);
        CurrentUser { user_id: Uuid::nil(), session_id: Uuid::nil(), csrf_token: String::new(), roles: vec![], permissions, ip: None, api_key: false, signals: Default::default() }
    }

    #[test]
    fn read_levels() {
        assert_eq!(read_level("customers.manage").as_deref(), Some("customers.read"));
        assert_eq!(read_level("settings.manage"), None);
        assert_eq!(read_level("users.manage"), None);
        let ro = user(&["customers.read"]);
        assert!(ro.permits("customers.manage", &Method::GET));
        assert!(!ro.permits("customers.manage", &Method::POST));
        assert!(!ro.permits("invoices.manage", &Method::GET));
        let rw = user(&["invoices.manage"]);
        assert!(rw.has("invoices.read"));
        assert!(rw.permits("invoices.manage", &Method::DELETE));
        // Not a read-level resource: no implied read, no GET fallback.
        let s = user(&["settings.read"]);
        assert!(!s.permits("settings.manage", &Method::GET));
    }
}

pub mod perms {
    use super::Permission;
    macro_rules! perm {
        ($t:ident, $n:literal) => {
            pub struct $t;
            impl Permission for $t {
                const NAME: &'static str = $n;
            }
        };
    }
    perm!(UsersRead, "users.read");
    perm!(UsersManage, "users.manage");
    perm!(AuditRead, "audit.read");
    perm!(AdminAccess, "admin.access");
    perm!(SettingsManage, "settings.manage");
    perm!(PluginsManage, "plugins.manage");
    perm!(GatewaysManage, "gateways.manage");
    perm!(CustomersManage, "customers.manage");
    perm!(CatalogManage, "catalog.manage");
    perm!(OrdersManage, "orders.manage");
    perm!(InvoicesManage, "invoices.manage");
    perm!(ServicesManage, "services.manage");
    perm!(JobsManage, "jobs.manage");
    perm!(TicketsManage, "tickets.manage");
    perm!(CouponsManage, "coupons.manage");
    perm!(ServersManage, "servers.manage");
    perm!(AffiliatesManage, "affiliates.manage");
    perm!(ContentManage, "content.manage");
    perm!(QuotesManage, "quotes.manage");
    perm!(DownloadsManage, "downloads.manage");
    perm!(ClientGroupsManage, "client_groups.manage");
    perm!(ApiKeysManage, "api_keys.manage");
    perm!(ReportsRead, "reports.read");
    perm!(GdprManage, "gdpr.manage");
    perm!(StatusManage, "status.manage");
    perm!(TodosManage, "todos.manage");
    perm!(CampaignsManage, "campaigns.manage");
    perm!(NotificationsManage, "notifications.manage");
    perm!(BackupsManage, "backups.manage");
    perm!(WebhooksManage, "webhooks.manage");
    perm!(CustomCodeManage, "settings.custom_code");
}
