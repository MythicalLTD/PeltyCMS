//! Admin security tools: configurable security policy (password rules,
//! session lifetimes, login rate limit), IP ban list + admin IP allow-list
//! (middleware), "Login as client / user" impersonation with audit and a
//! return path, and client close / merge.
use crate::error::{ApiError, ApiResult};
use crate::extract::{perms, CurrentUser, RequirePermission, SESSION_COOKIE};
use crate::routes::auth::{cookie_header, AuthGuard};
use crate::AppState;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use ipnet::IpNet;
use pelty_auth::policy::{self, SecurityPolicy};
use pelty_core::AppError;
use pelty_database::{audit, customers, security as db, settings, users};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;

pub const RETURN_COOKIE: &str = "pelty_imp_return";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/security/policy", get(get_policy).put(put_policy))
        .route("/admin/security/ip-rules", get(list_rules).post(add_rule))
        .route("/admin/security/trusted-proxies", get(crate::trusted_proxy::get_cfg).put(crate::trusted_proxy::put_cfg))
        .route("/admin/security/geoip", get(get_geoip))
        .route("/admin/security/ip-rules/{id}", delete(delete_rule))
        .route("/admin/customers/{id}/login-as", post(login_as_customer))
        .route("/admin/users/{id}/login-as", post(login_as_user))
        .route("/auth/impersonation", get(impersonation_status))
        .route("/auth/impersonation/stop", post(stop_impersonation))
        .route("/admin/customers/{id}/close", post(close_customer))
        .route("/admin/customers/{id}/merge", post(merge_customer))
        .route("/admin/security/tos", get(list_tos).post(publish_tos))
        .route("/account/tos", get(my_tos))
        .route("/account/tos/accept", post(accept_tos))
}

// ----- Terms of Service versions + acceptance (GDPR consent record) -----

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TosInput {
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub summary: String,
}

#[utoipa::path(get, path = "/api/v1/admin/security/tos", responses((status = 200), (status = 403)))]
pub async fn list_tos(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = db::list_tos(&s.pool).await?.into_iter().map(|(v, n)| json!({ "version": v, "acceptances": n })).collect();
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(post, path = "/api/v1/admin/security/tos", request_body = TosInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn publish_tos(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<TosInput>) -> ApiResult<Json<Value>> {
    let version = b.version.trim();
    let url = b.url.trim();
    if version.is_empty() || version.chars().count() > 40 {
        return Err(AppError::Validation("error.invalid_version").into());
    }
    if (!url.is_empty() && !(url.starts_with("https://") || url.starts_with('/'))) || url.len() > 500 {
        return Err(AppError::Validation("error.invalid_url").into());
    }
    if b.summary.chars().count() > 2000 {
        return Err(AppError::Validation("error.too_long").into());
    }
    let v = db::publish_tos(&s.pool, version, url, b.summary.trim()).await?;
    audit::record(&s.pool, Some(u.user_id), "security.tos_published", Some(&v.id.to_string()), u.ip.as_deref(), json!({ "version": v.version })).await?;
    Ok(Json(json!({ "version": v })))
}

#[utoipa::path(get, path = "/api/v1/account/tos", responses((status = 200), (status = 401)))]
pub async fn my_tos(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cur = db::current_tos(&s.pool).await?;
    let accepted_at = match &cur {
        Some(v) => db::accepted_tos(&s.pool, u.user_id, v.id).await?,
        None => None,
    };
    let history: Vec<Value> = db::tos_history(&s.pool, u.user_id).await?.into_iter().map(|(v, at)| json!({ "version": v, "accepted_at": fmt_ts(at) })).collect();
    Ok(Json(json!({
        "current": cur,
        "accepted_at": accepted_at.map(fmt_ts),
        "pending": cur.is_some() && accepted_at.is_none(),
        "history": history,
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptTos {
    pub version_id: Uuid,
}

#[utoipa::path(post, path = "/api/v1/account/tos/accept", request_body = AcceptTos, responses((status = 200), (status = 409)))]
pub async fn accept_tos(State(s): State<AppState>, u: CurrentUser, Json(b): Json<AcceptTos>) -> ApiResult<Json<Value>> {
    // Only the current version can be accepted: accepting a superseded text
    // would record consent to terms that no longer apply.
    let cur = db::current_tos(&s.pool).await?.ok_or(AppError::NotFound)?;
    if cur.id != b.version_id {
        return Err(AppError::Conflict("error.tos_outdated").into());
    }
    db::accept_tos(&s.pool, u.user_id, cur.id, u.ip.as_deref()).await?;
    audit::record(&s.pool, Some(u.user_id), "account.tos_accepted", Some(&cur.id.to_string()), u.ip.as_deref(), json!({ "version": cur.version })).await?;
    Ok(Json(json!({ "ok": true })))
}

fn fmt_ts(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// Apply the stored login rate limit to the in-process limiter (startup).
pub async fn init(s: &AppState) -> pelty_core::AppResult<()> {
    if let Some(r) = policy::load(&s.pool).await?.login_rate_per_minute {
        s.auth_limiter.set_limit(r);
    }
    Ok(())
}

// ----- policy -----

#[utoipa::path(get, path = "/api/v1/admin/security/policy", responses((status = 200), (status = 403)))]
pub async fn get_policy(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    let p = policy::load(&s.pool).await?;
    Ok(Json(json!({ "policy": p.to_value(), "defaults": { "session_idle_minutes": s.cfg.session_idle_minutes, "session_absolute_hours": s.cfg.session_absolute_hours, "login_rate_per_minute": s.cfg.auth_rate_per_minute } })))
}

#[utoipa::path(get, path = "/api/v1/admin/security/geoip", responses((status = 200), (status = 403)))]
pub async fn get_geoip(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(s.geoip.status()))
}

#[utoipa::path(put, path = "/api/v1/admin/security/policy", responses((status = 200), (status = 422)))]
pub async fn put_policy(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let p = SecurityPolicy::from_value(&b)?;
    settings::set(&s.pool, policy::SETTING_KEY, p.to_value()).await?;
    policy::invalidate();
    s.auth_limiter.set_limit(p.login_rate_per_minute.unwrap_or(s.cfg.auth_rate_per_minute));
    audit::record(&s.pool, Some(u.user_id), "security.policy_updated", None, u.ip.as_deref(), p.to_value()).await?;
    Ok(Json(json!({ "policy": p.to_value() })))
}

// ----- IP rules -----

/// (loaded at, [(network, kind)]).
type RuleCache = Option<(Instant, Vec<(IpNet, String)>)>;
static RULES: Mutex<RuleCache> = Mutex::new(None);

fn invalidate_rules() {
    if let Ok(mut g) = RULES.lock() {
        *g = None;
    }
}

/// Drop the cached IP rules (another instance changed them).
pub fn invalidate_caches() {
    invalidate_rules();
}

/// Parse `1.2.3.4`, `1.2.3.0/24`, `2001:db8::/32`.
pub fn parse_cidr(s: &str) -> Option<IpNet> {
    let s = s.trim();
    s.parse::<IpNet>().ok().or_else(|| s.parse::<IpAddr>().ok().map(IpNet::from)).map(|n| n.trunc())
}

/// Decision for one request: `Some(code)` = refuse.
pub fn decide(rules: &[(IpNet, String)], ip: IpAddr, admin_path: bool) -> Option<&'static str> {
    if rules.iter().any(|(n, k)| k == "ban" && n.contains(&ip)) {
        return Some("error.ip_banned");
    }
    if admin_path {
        let allow: Vec<_> = rules.iter().filter(|(_, k)| k == "admin_allow").collect();
        if !allow.is_empty() && !allow.iter().any(|(n, _)| n.contains(&ip)) {
            return Some("error.ip_not_allowed");
        }
    }
    None
}

/// The admin allow-list check for routes outside `/api/v1/admin` that are
/// admin-only (addon routes with `access: "admin"`). `None` IP (no
/// ConnectInfo, e.g. in-process tests) is allowed like in [`ip_guard`].
pub async fn admin_ip_allowed(s: &AppState, ip: Option<IpAddr>) -> Result<(), &'static str> {
    match ip {
        Some(ip) => match decide(&cached_rules(s).await, ip, true) {
            Some(code) => Err(code),
            None => Ok(()),
        },
        None => Ok(()),
    }
}

async fn cached_rules(s: &AppState) -> Vec<(IpNet, String)> {
    if let Ok(g) = RULES.lock() {
        if let Some((t, r)) = g.as_ref() {
            if t.elapsed() < Duration::from_secs(10) {
                return r.clone();
            }
        }
    }
    let rows = match db::active_ip_rules(&s.pool).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "ip rules unavailable");
            return vec![];
        }
    };
    let parsed: Vec<_> = rows.into_iter().filter_map(|r| parse_cidr(&r.cidr).map(|n| (n, r.kind))).collect();
    if let Ok(mut g) = RULES.lock() {
        *g = Some((Instant::now(), parsed.clone()));
    }
    parsed
}

/// Middleware: banned IPs get 403 everywhere; when an admin allow-list
/// exists, `/api/v1/admin/*` only answers to listed networks.
pub async fn ip_guard(State(s): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(ip) = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip()) {
        let admin = req.uri().path().starts_with("/api/v1/admin");
        let rules = cached_rules(&s).await;
        if let Some(code) = decide(&rules, ip, admin) {
            return ApiError(AppError::Denied(code)).into_response();
        }
    }
    next.run(req).await
}

#[utoipa::path(get, path = "/api/v1/admin/security/ip-rules", responses((status = 200), (status = 403)))]
pub async fn list_rules(State(s): State<AppState>, u: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "rules": db::list_ip_rules(&s.pool).await?, "your_ip": u.0.ip })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RuleBody {
    pub cidr: String,
    /// ban | admin_allow
    pub kind: String,
    #[serde(default)]
    pub reason: String,
    /// Optional expiry in hours (bans).
    #[serde(default)]
    pub hours: Option<i64>,
}

#[utoipa::path(post, path = "/api/v1/admin/security/ip-rules", request_body = RuleBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn add_rule(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<RuleBody>) -> ApiResult<Json<Value>> {
    let net = parse_cidr(&b.cidr).ok_or(AppError::Validation("error.invalid_cidr"))?;
    if !["ban", "admin_allow"].contains(&b.kind.as_str()) || b.reason.chars().count() > 500 || b.hours.is_some_and(|h| !(1..=24 * 3650).contains(&h)) {
        return Err(AppError::Validation("error.invalid_ip_rule").into());
    }
    let me: Option<IpAddr> = u.ip.as_deref().and_then(|i| i.parse().ok());
    if let Some(me) = me {
        // Never let an admin lock themselves out.
        if b.kind == "ban" && net.contains(&me) {
            return Err(AppError::Validation("error.ip_rule_self_ban").into());
        }
        if b.kind == "admin_allow" {
            let rules = cached_rules(&s).await;
            let already = rules.iter().any(|(n, k)| k == "admin_allow" && n.contains(&me));
            if !already && !net.contains(&me) {
                return Err(AppError::Validation("error.ip_rule_self_lockout").into());
            }
        }
    }
    let exp = b.hours.map(|h| OffsetDateTime::now_utc() + time::Duration::hours(h));
    let r = db::add_ip_rule(&s.pool, &net.to_string(), &b.kind, b.reason.trim(), exp, u.user_id).await?;
    invalidate_rules();
    audit::record(&s.pool, Some(u.user_id), "security.ip_rule_added", Some(&r.id.to_string()), u.ip.as_deref(), json!({ "cidr": r.cidr, "kind": r.kind })).await?;
    Ok(Json(json!(r)))
}

#[utoipa::path(delete, path = "/api/v1/admin/security/ip-rules/{id}", params(("id" = Uuid, Path)), responses((status = 204), (status = 404)))]
pub async fn delete_rule(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    if !db::delete_ip_rule(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    invalidate_rules();
    audit::record(&s.pool, Some(u.user_id), "security.ip_rule_deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- impersonation -----

async fn impersonate(s: &AppState, admin: &CurrentUser, g: &AuthGuard, headers: &HeaderMap, target: Uuid, customer: Option<Uuid>) -> ApiResult<Response> {
    if admin.api_key {
        return Err(AppError::Denied("error.impersonation_api_key").into());
    }
    if target == admin.user_id {
        return Err(AppError::Validation("error.impersonation_self").into());
    }
    if db::impersonation_of(&s.pool, admin.session_id).await?.is_some() {
        return Err(AppError::Validation("error.impersonation_nested").into());
    }
    let user = users::find(&s.pool, target).await?.ok_or(AppError::NotFound)?;
    if !user.is_active {
        return Err(AppError::Validation("error.user_inactive").into());
    }
    if users::roles(&s.pool, target).await?.iter().any(|r| r != "customer") {
        return Err(AppError::Denied("error.impersonation_staff").into());
    }
    let admin_token = crate::extract::cookie(headers, SESSION_COOKIE).ok_or(AppError::Unauthenticated)?;
    let issued = s.auth.issue_session(user, g.ip(), Some("impersonation")).await?;
    db::mark_impersonation(&s.pool, issued.session.id, admin.user_id, admin.session_id).await?;
    audit::record(
        &s.pool,
        Some(admin.user_id),
        "admin.impersonation_started",
        Some(&target.to_string()),
        admin.ip.as_deref(),
        json!({ "target_user": target, "customer_id": customer, "session": issued.session.id }),
    )
    .await?;
    let max_age = 4 * 3600;
    let mut r = Json(json!({ "user_id": target, "csrf_token": issued.csrf_token, "redirect": "/dashboard" })).into_response();
    r.headers_mut().append(header::SET_COOKIE, cookie_header(s, SESSION_COOKIE, &issued.token, max_age, true));
    r.headers_mut().append(header::SET_COOKIE, cookie_header(s, RETURN_COOKIE, &admin_token, max_age, true));
    Ok(r)
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/login-as", params(("id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404)))]
pub async fn login_as_customer(State(s): State<AppState>, g: AuthGuard, headers: HeaderMap, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let c = customers::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    if c.status == "closed" {
        return Err(AppError::Validation("error.customer_closed").into());
    }
    let owner = c.owner_user_id.ok_or(AppError::Validation("error.customer_no_login"))?;
    impersonate(&s, &u, &g, &headers, owner, Some(id)).await
}

/// "Login as user", any client-side user, including sub-account users.
#[utoipa::path(post, path = "/api/v1/admin/users/{id}/login-as", params(("id" = Uuid, Path)), responses((status = 200), (status = 403), (status = 404)))]
pub async fn login_as_user(State(s): State<AppState>, g: AuthGuard, headers: HeaderMap, RequirePermission(u, _): RequirePermission<perms::UsersManage>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    impersonate(&s, &u, &g, &headers, id, None).await
}

#[utoipa::path(get, path = "/api/v1/auth/impersonation", responses((status = 200), (status = 401)))]
pub async fn impersonation_status(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    match db::impersonation_of(&s.pool, u.session_id).await? {
        Some((admin, _)) => {
            let a = users::find(&s.pool, admin).await?;
            Ok(Json(json!({ "active": true, "admin_id": admin, "admin_email": a.map(|a| a.email) })))
        }
        None => Ok(Json(json!({ "active": false }))),
    }
}

#[utoipa::path(post, path = "/api/v1/auth/impersonation/stop", responses((status = 200), (status = 409)))]
pub async fn stop_impersonation(State(s): State<AppState>, u: CurrentUser, headers: HeaderMap) -> ApiResult<Response> {
    let (admin, parent) = db::impersonation_of(&s.pool, u.session_id).await?.ok_or(AppError::Conflict("error.not_impersonating"))?;
    s.auth.revoke(u.user_id, u.session_id, u.ip.as_deref(), "auth.logout").await?;
    audit::record(&s.pool, Some(admin), "admin.impersonation_ended", Some(&u.user_id.to_string()), u.ip.as_deref(), json!({ "target_user": u.user_id })).await?;
    let mut r = Json(json!({ "restored": false, "redirect": "/login" })).into_response();
    if let (Some(tok), Some(parent)) = (crate::extract::cookie(&headers, RETURN_COOKIE), parent) {
        if let Some(sess) = s.auth.authenticate(&tok).await? {
            if sess.id == parent && sess.user_id == admin {
                r = Json(json!({ "restored": true, "redirect": "/admin/customers" })).into_response();
                r.headers_mut().append(header::SET_COOKIE, cookie_header(&s, SESSION_COOKIE, &tok, s.cfg.session_absolute_hours * 3600, true));
            }
        }
    }
    r.headers_mut().append(header::SET_COOKIE, cookie_header(&s, RETURN_COOKIE, "", 0, true));
    Ok(r)
}

// ----- client close / merge -----

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/close", params(("id" = Uuid, Path)), responses((status = 200), (status = 404)))]
pub async fn close_customer(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let owner = db::close_customer(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "customer.closed", Some(&id.to_string()), u.ip.as_deref(), json!({ "owner": owner })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("customer.closed", json!({ "customer_id": id })));
    Ok(Json(json!({ "id": id, "status": "closed" })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct MergeBody {
    /// Customer that keeps everything.
    pub into: Uuid,
}

#[utoipa::path(post, path = "/api/v1/admin/customers/{id}/merge", params(("id" = Uuid, Path)), request_body = MergeBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn merge_customer(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::CustomersManage>, Path(id): Path<Uuid>, Json(b): Json<MergeBody>) -> ApiResult<Json<Value>> {
    let moved = db::merge_customers(&s.pool, id, b.into).await?;
    audit::record(&s.pool, Some(u.user_id), "customer.merged", Some(&id.to_string()), u.ip.as_deref(), json!({ "into": b.into, "rows": moved })).await?;
    Ok(Json(json!({ "from": id, "into": b.into, "moved_rows": moved })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ip_decisions() {
        let rules = vec![(parse_cidr("10.0.0.0/8").unwrap(), "ban".to_string()), (parse_cidr("192.168.1.5").unwrap(), "admin_allow".to_string())];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert_eq!(decide(&rules, ip("10.2.3.4"), false), Some("error.ip_banned"));
        assert_eq!(decide(&rules, ip("8.8.8.8"), false), None);
        assert_eq!(decide(&rules, ip("8.8.8.8"), true), Some("error.ip_not_allowed"));
        assert_eq!(decide(&rules, ip("192.168.1.5"), true), None);
        assert_eq!(parse_cidr("10.1.2.3/8").unwrap().to_string(), "10.0.0.0/8");
        assert!(parse_cidr("nope").is_none());
        assert_eq!(parse_cidr("2001:db8::1").unwrap().to_string(), "2001:db8::1/128");
    }
}
