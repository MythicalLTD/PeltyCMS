//! Domains: client search/order/manage, admin TLD pricing and domain admin
//! (`services.manage`). Registrar work runs in the `domains.*` jobs.
use crate::routes::customers::customer_for;
use crate::routes::{paged, ListQuery};
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::domains::{self as db, Domain, NewOrder, Tld};
use pelty_database::audit;
use pelty_provisioning::registrar::Contact;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/domains/tlds", get(client_tlds))
        .route("/admin/settings/domain-defaults", get(get_defaults).put(put_defaults))
        .route("/client/domains/check", get(client_check))
        .route("/client/domains", get(client_list).post(client_order))
        .route("/client/domains/{id}", get(client_get))
        .route("/client/domains/{id}/nameservers", put(client_ns))
        .route("/client/domains/{id}/contact", put(client_contact))
        .route("/client/domains/{id}/auto-renew", put(client_auto_renew))
        .route("/client/domains/{id}/renew", post(client_renew))
        .route("/client/domains/{id}/epp", post(client_epp))
        .route("/admin/tlds", get(admin_tlds).put(admin_tld_upsert))
        .route("/admin/tlds/{tld}/{currency}", axum::routing::delete(admin_tld_delete))
        .route("/admin/registrars", get(admin_registrars))
        .route("/admin/domains", get(admin_list))
        .route("/admin/domains/run", post(admin_run))
        .route("/admin/domains/{id}", get(admin_get).patch(admin_update))
        .route("/admin/domains/{id}/sync", post(admin_sync))
        .route("/client/domains/{id}/dns", get(client_dns).put(client_dns_put))
        .route("/client/domains/{id}/addons", put(client_addons))
        .route("/client/domains/bulk", post(client_bulk))
        .route("/admin/domains/{id}/dns", get(admin_dns).put(admin_dns_put))
        .route("/admin/domains/bulk", post(admin_bulk))
        .route("/admin/tlds/bulk-adjust", post(admin_tld_adjust))
        .route("/admin/tlds/copy-currency", post(admin_tld_copy))
        .route("/public/domains/tlds", get(public_tlds))
        .route("/public/domains/check", get(public_check))
        .route("/public/domains/whois", get(public_whois))
        .route("/public/domains/verify-registrant", post(public_verify_registrant))
        .route("/client/domains/{id}/registrant-verification/resend", post(client_rv_resend))
        .route("/admin/domains/{id}/registrant-verification", post(admin_rv_action))
        .route("/admin/settings/registrant-verification", get(admin_rv_settings).put(admin_rv_settings_put))
}

fn validate_contact(c: &Contact) -> ApiResult<Value> {
    let email_ok = c.email.contains('@') && c.email.len() <= 254;
    if c.name.trim().is_empty() || c.name.len() > 200 || !email_ok || c.country.len() > 2 {
        return Err(AppError::Validation("error.invalid_contact").into());
    }
    let long = [&c.organization, &c.phone, &c.address, &c.city, &c.postal_code].iter().any(|f| f.len() > 300);
    if long {
        return Err(AppError::Validation("error.invalid_contact").into());
    }
    Ok(serde_json::to_value(c).map_err(AppError::internal)?)
}

async fn currency_of(s: &AppState, cid: Uuid) -> ApiResult<String> {
    Ok(s.clients().get(cid).await?.ok_or(AppError::NotFound)?.currency)
}

async fn detail(s: &AppState, d: Domain) -> ApiResult<Json<Value>> {
    let renewals = s.domains().renewals(d.id).await?;
    let rv = pelty_database::registrant_verify::get(&s.pool, d.id).await?;
    Ok(Json(json!({ "domain": d, "renewals": renewals, "registrant_verification": rv })))
}

#[utoipa::path(get, path = "/api/v1/client/domains/tlds", responses((status = 200)))]
pub async fn client_tlds(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let cur = currency_of(&s, c).await?;
    Ok(Json(json!({ "items": s.domains().tlds(true, Some(&cur)).await?, "currency": cur })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct CheckQuery {
    pub name: String,
}

#[utoipa::path(get, path = "/api/v1/client/domains/check", params(CheckQuery), responses((status = 200), (status = 422)))]
pub async fn client_check(State(s): State<AppState>, u: CurrentUser, headers: axum::http::HeaderMap, Query(q): Query<CheckQuery>) -> ApiResult<Json<Value>> {
    crate::captcha::require(&s, &headers, "domain_lookup", u.ip.as_deref()).await?;
    let c = customer_for(&s, &u, "domains").await?;
    let cur = currency_of(&s, c).await?;
    let (name, tld) = db::normalize(&q.name)?;
    let Some(t) = s.domains().tld(&tld, &cur).await?.filter(|t| t.active) else {
        return Ok(Json(json!({ "name": name, "tld": tld, "supported": false, "available": false })));
    };
    let reg = crate::addons::registrar(&s, &t.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
    let available = !s.domains().name_taken(&name).await? && reg.available(&name).await.map_err(|_| AppError::Validation("error.registrar_unavailable"))?;
    let premium = if available { premium_for(&s, reg.as_ref(), &name, &cur).await } else { Ok(None) };
    let (premium, premium_error) = match premium {
        Ok(p) => (p, None),
        Err(AppError::Validation(k)) => (None, Some(k)),
        Err(e) => return Err(e.into()),
    };
    Ok(Json(json!({ "name": name, "tld": tld, "supported": true, "available": available && premium_error.is_none(), "pricing": t,
        "premium": premium.map(|(r, n)| json!({ "register_price": r, "renew_price": n })), "premium_error": premium_error })))
}

/// Premium sell prices (register, renew) for `name`, per the premium settings.
async fn premium_for(s: &AppState, reg: &dyn pelty_provisioning::registrar::RegistrarModule, name: &str, cur: &str) -> Result<Option<(rust_decimal::Decimal, rust_decimal::Decimal)>, AppError> {
    let q = reg.premium_price(name).await.map_err(|_| AppError::Validation("error.registrar_unavailable"))?;
    let p = pelty_database::domain_pricing::premium_settings(&s.pool).await?;
    pelty_database::domain_pricing::premium_quote(&p, cur, q.as_ref().map(|q| (q.currency.as_str(), q.register.as_str(), q.renew.as_str())))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OrderBody {
    pub name: String,
    /// `register` or `transfer`.
    pub kind: String,
    #[serde(default = "one")]
    pub years: i32,
    #[serde(default)]
    pub nameservers: Vec<String>,
    #[schema(value_type = Object)]
    pub contact: Contact,
    #[serde(default)]
    pub privacy: bool,
    #[serde(default)]
    pub dns_management: bool,
    /// EPP code (transfers only); sealed at rest.
    #[serde(default)]
    pub auth_code: String,
    /// Promo code valid for domains.
    #[serde(default)]
    pub coupon: Option<String>,
    /// Claim the free domain of this service or pending order item.
    #[serde(default)]
    pub free_service_id: Option<Uuid>,
    #[serde(default)]
    pub free_order_item_id: Option<Uuid>,
    /// Values for the TLD's additional fields.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub extra_fields: Value,
}

fn one() -> i32 {
    1
}

#[utoipa::path(post, path = "/api/v1/client/domains", request_body = OrderBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn client_order(State(s): State<AppState>, u: CurrentUser, Json(b): Json<OrderBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    pelty_database::kyc::ensure_can_order(&s.pool, c).await?;
    if b.kind != "register" && b.kind != "transfer" {
        return Err(AppError::Validation("error.validation").into());
    }
    let cur = currency_of(&s, c).await?;
    let (name, tld) = db::normalize(&b.name)?;
    let t = s.domains().tld(&tld, &cur).await?.filter(|t| t.active).ok_or(AppError::Validation("error.tld_unsupported"))?;
    let defaults = pelty_database::domain_defaults::get(&s.pool).await?;
    let mut ns = db::normalize_ns(&b.nameservers)?;
    if ns.is_empty() {
        ns = defaults.nameservers.clone();
    }
    let contact = validate_contact(&b.contact)?;
    if s.domains().name_taken(&name).await? {
        return Err(AppError::Conflict("error.domain_taken").into());
    }
    let reg = crate::addons::registrar(&s, &t.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
    let sealed = if b.kind == "transfer" {
        let code = b.auth_code.trim();
        if (code.is_empty() && t.epp_required) || code.len() > 64 {
            return Err(AppError::Validation("error.auth_code_required").into());
        }
        let sb = s.secret_box.as_ref().ok_or(AppError::Validation("error.encryption_not_configured"))?;
        Some(sb.seal(code.as_bytes()))
    } else {
        if !reg.available(&name).await.map_err(|_| AppError::Validation("error.registrar_unavailable"))? {
            return Err(AppError::Conflict("error.domain_taken").into());
        }
        None
    };
    let extra = pelty_database::domain_pricing::validate_extra(&pelty_database::domain_pricing::tld_fields(&s.pool, &tld).await?, &b.extra_fields)?;
    let mut t = t;
    let premium_renew = if b.kind == "register" {
        match premium_for(&s, reg.as_ref(), &name, &cur).await? {
            Some((r, n)) => {
                t.register_price = r;
                t.renew_price = n;
                Some(n)
            }
            None => None,
        }
    } else {
        None
    };
    // Cancel-only pre-event: refuse a registration or transfer.
    crate::addon_ext::pre_event(&s, "domain.before_order", json!({ "customer_id": c, "name": name, "tld": t.tld, "kind": b.kind, "years": b.years, "privacy": b.privacy, "dns_management": b.dns_management })).await?;
    let (d, inv) = s.domains().create_order(NewOrder { premium_renew, extra_fields: extra, customer_id: c, name: &name, tld: &t, kind: &b.kind, years: b.years, nameservers: &ns, contact: &contact, privacy: b.privacy, dns_management: b.dns_management, auth_code_sealed: sealed, coupon: b.coupon.as_deref(), free_service: b.free_service_id, free_order_item: b.free_order_item_id }).await?;
    if b.dns_management && !defaults.dns_template.is_empty() {
        s.domains().replace_dns(d.id, &defaults.records_for(&d.name)).await?;
    }
    audit::record(&s.pool, Some(u.user_id), "domain.ordered", Some(&d.id.to_string()), u.ip.as_deref(), json!({ "name": d.name, "kind": d.kind })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.ordered", json!({ "domain_id": d.id, "customer_id": c, "name": d.name, "kind": d.kind, "invoice_id": inv.id })));
    Ok(Json(json!({ "domain": d, "invoice_id": inv.id, "total": inv.total })))
}

#[utoipa::path(get, path = "/api/v1/client/domains", params(ListQuery), responses((status = 200)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = s.domains().list(Some(c), q.status(), q.q(), limit, offset).await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/client/domains/{id}", responses((status = 200), (status = 404)))]
pub async fn client_get(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    detail(&s, d).await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NsBody {
    pub nameservers: Vec<String>,
}

async fn apply_ns(s: &AppState, d: &Domain, ns: &[String]) -> ApiResult<()> {
    if d.status == "active" {
        let reg = crate::addons::registrar(s, &d.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
        reg.set_nameservers(&d.name, ns).await.map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    } else if d.status != "pending" {
        return Err(AppError::Validation("error.domain_not_active").into());
    }
    s.domains().set_nameservers(d.id, ns).await?;
    Ok(())
}

#[utoipa::path(put, path = "/api/v1/client/domains/{id}/nameservers", request_body = NsBody, responses((status = 200), (status = 422)))]
pub async fn client_ns(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, Json(b): Json<NsBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    let ns = db::normalize_ns(&b.nameservers)?;
    // Pre-event: addons may veto or rewrite the nameserver list (re-validated).
    let pre = crate::addon_ext::pre_event(&s, "domain.before_nameservers", json!({ "domain_id": id, "name": d.name, "nameservers": ns, "by_staff": false })).await?;
    let ns = match pre.get("nameservers").and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok()) {
        Some(v) if v != ns => db::normalize_ns(&v)?,
        _ => ns,
    };
    apply_ns(&s, &d, &ns).await?;
    let out = detail(&s, s.domains().get_owned(id, c).await?).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.nameservers_changed", Some(&id.to_string()), u.ip.as_deref(), json!({ "nameservers": ns })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.nameservers_changed", json!({ "domain_id": id, "nameservers": ns })));
    Ok(out)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactBody {
    #[schema(value_type = Object)]
    pub contact: Contact,
    #[serde(default)]
    pub privacy: bool,
}

#[utoipa::path(put, path = "/api/v1/client/domains/{id}/contact", request_body = ContactBody, responses((status = 200), (status = 422)))]
pub async fn client_contact(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, Json(b): Json<ContactBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    let v = validate_contact(&b.contact)?;
    if d.status == "active" {
        let reg = crate::addons::registrar(&s, &d.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
        reg.set_contact(&d.name, &b.contact).await.map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    }
    s.domains().set_contact(id, &v, b.privacy).await?;
    let after = s.domains().get_owned(id, c).await?;
    crate::registrant_verify::contact_changed(&s, &d, &after, time::OffsetDateTime::now_utc().date()).await?;
    let out = detail(&s, s.domains().get_owned(id, c).await?).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.contact_changed", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.contact_changed", json!({ "domain_id": id })));
    Ok(out)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AutoRenewBody {
    pub auto_renew: bool,
}

#[utoipa::path(put, path = "/api/v1/client/domains/{id}/auto-renew", request_body = AutoRenewBody, responses((status = 200)))]
pub async fn client_auto_renew(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, Json(b): Json<AutoRenewBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    s.domains().get_owned(id, c).await?;
    s.domains().set_auto_renew(id, b.auto_renew).await?;
    let out = detail(&s, s.domains().get_owned(id, c).await?).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.auto_renew_set", Some(&id.to_string()), u.ip.as_deref(), json!({ "auto_renew": b.auto_renew })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.auto_renew_changed", json!({ "domain_id": id, "auto_renew": b.auto_renew })));
    Ok(out)
}

/// Reveal the transfer-out (EPP) code. Audited; owner only, active domains.
#[utoipa::path(post, path = "/api/v1/client/domains/{id}/epp", responses((status = 200), (status = 409), (status = 422)))]
pub async fn client_epp(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    if d.status != "active" {
        return Err(AppError::Validation("error.domain_not_active").into());
    }
    let reg = crate::addons::registrar(&s, &d.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
    let code = reg.epp_code(&d.name).await.map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    let code = code.ok_or(AppError::Conflict("error.epp_unsupported"))?;
    audit::record(&s.pool, Some(u.user_id), "domain.epp_revealed", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.epp_revealed", json!({ "domain_id": id, "by": u.user_id })));
    Ok(Json(json!({ "code": code })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RenewBody {
    #[serde(default = "one")]
    pub years: i32,
}

#[utoipa::path(post, path = "/api/v1/client/domains/{id}/renew", request_body = RenewBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn client_renew(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, Json(b): Json<RenewBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    if !db::RENEWABLE.contains(&d.status.as_str()) {
        return Err(AppError::Validation("error.domain_not_active").into());
    }
    let cur = currency_of(&s, c).await?;
    let t = s.domains().tld(&d.tld, &cur).await?.ok_or(AppError::Validation("error.tld_unsupported"))?;
    let inv = s.domains().create_renewal(&d, &t, b.years).await?.ok_or(AppError::Conflict("error.renewal_exists"))?;
    audit::record(&s.pool, Some(u.user_id), "domain.renewal_requested", Some(&id.to_string()), u.ip.as_deref(), json!({ "invoice_id": inv.id, "years": b.years })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.renewal_requested", json!({ "domain_id": id, "invoice_id": inv.id, "years": b.years })));
    Ok(Json(json!({ "invoice_id": inv.id, "total": inv.total })))
}

// ---- admin ----

#[utoipa::path(get, path = "/api/v1/admin/tlds", responses((status = 200)))]
pub async fn admin_tlds(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": s.domains().tlds(false, None).await? })))
}

#[utoipa::path(put, path = "/api/v1/admin/tlds", request_body(content = Object), responses((status = 200), (status = 422)))]
pub async fn admin_tld_upsert(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<Tld>) -> ApiResult<Json<Tld>> {
    if crate::addons::registrar(&s, &b.registrar).await?.is_none() {
        return Err(AppError::Validation("error.registrar_unavailable").into());
    }
    let t = s.domains().upsert_tld(&b).await?;
    audit::record(&s.pool, Some(u.user_id), "tld.saved", Some(&t.tld), u.ip.as_deref(), json!({ "currency": t.currency })).await?;
    Ok(Json(t))
}

#[utoipa::path(delete, path = "/api/v1/admin/tlds/{tld}/{currency}", responses((status = 200), (status = 404)))]
pub async fn admin_tld_delete(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Path((tld, currency)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    s.domains().delete_tld(&tld, &currency.to_ascii_uppercase()).await?;
    audit::record(&s.pool, Some(u.user_id), "tld.deleted", Some(&tld), u.ip.as_deref(), json!({ "currency": currency })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/registrars", responses((status = 200)))]
pub async fn admin_registrars(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Value>> {
    let mut items = vec![];
    for r in s.registrars.list() {
        let enabled = crate::addons::registrar(&s, r.id).await?.is_some();
        items.push(json!({ "id": r.id, "name": r.name, "enabled": enabled, "addon_id": crate::addons::builtin_id("registrar", r.id) }));
    }
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(get, path = "/api/v1/admin/domains", params(ListQuery), responses((status = 200)))]
pub async fn admin_list(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let (limit, offset, page, per) = q.bounds();
    let (items, total) = s.domains().list(None, q.status(), q.q(), limit, offset).await?;
    Ok(Json(paged(items, page, per, total)))
}

#[utoipa::path(get, path = "/api/v1/admin/domains/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_get(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = s.domains().get(id).await?.ok_or(AppError::NotFound)?;
    detail(&s, d).await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AdminUpdate {
    pub status: Option<String>,
    #[schema(value_type = Option<String>)]
    pub expires_on: Option<time::Date>,
    pub auto_renew: Option<bool>,
    pub dns_management: Option<bool>,
    pub registrar_lock: Option<bool>,
    pub privacy: Option<bool>,
    pub admin_notes: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/admin/domains/{id}", request_body = AdminUpdate, responses((status = 200), (status = 422)))]
pub async fn admin_update(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>, Json(b): Json<AdminUpdate>) -> ApiResult<Json<Value>> {
    let d = s.domains().admin_update(id, b.status.as_deref(), b.expires_on, b.auto_renew).await?;
    if b.dns_management.is_some() || b.registrar_lock.is_some() || b.privacy.is_some() {
        s.domains().set_flags(id, b.dns_management, b.registrar_lock, b.privacy).await?;
    }
    if let Some(n) = &b.admin_notes {
        s.domains().set_admin_notes(id, n).await?;
    }
    let d = s.domains().get(d.id).await?.ok_or(AppError::NotFound)?;
    audit::record(&s.pool, Some(u.user_id), "domain.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "status": b.status, "expires_on": b.expires_on })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.updated", json!({ "domain_id": id, "status": b.status, "by": u.user_id })));
    detail(&s, d).await
}

#[utoipa::path(post, path = "/api/v1/admin/domains/{id}/sync", responses((status = 200), (status = 422)))]
pub async fn admin_sync(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = s.domains().get(id).await?.ok_or(AppError::NotFound)?;
    let reg = crate::addons::registrar(&s, &d.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
    let info = reg.info(&d.name).await.map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    let exp = time::Date::parse(&info.expires_on, &time::format_description::well_known::Iso8601::DATE).map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    let status = if db::STATUSES.contains(&info.status.as_str()) { Some(info.status.as_str()) } else { None };
    s.domains().mark_synced(id, exp, status, None).await?;
    if !info.nameservers.is_empty() {
        s.domains().set_nameservers(id, &info.nameservers).await?;
    }
    audit::record(&s.pool, Some(u.user_id), "domain.synced", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.synced", json!({ "domain_id": id })));
    detail(&s, s.domains().get(id).await?.ok_or(AppError::NotFound)?).await
}

#[utoipa::path(post, path = "/api/v1/admin/domains/run", responses((status = 200)))]
pub async fn admin_run(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Value>> {
    let p = crate::domains::process(&s).await?;
    let d = crate::domains::daily(&s, time::OffsetDateTime::now_utc().date()).await?;
    Ok(Json(json!({ "process": p, "daily": d })))
}

// ---- DNS management ----

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DnsBody {
    #[schema(value_type = Vec<Object>)]
    pub records: Vec<db::DnsRecord>,
}

#[utoipa::path(get, path = "/api/v1/client/domains/{id}/dns", responses((status = 200), (status = 409)))]
pub async fn client_dns(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    if !d.dns_management {
        return Err(AppError::Conflict("error.dns_management_disabled").into());
    }
    Ok(Json(json!({ "items": s.domains().dns_records(id).await?, "types": db::DNS_TYPES })))
}

#[utoipa::path(put, path = "/api/v1/client/domains/{id}/dns", request_body = DnsBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn client_dns_put(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, Json(b): Json<DnsBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    if !d.dns_management {
        return Err(AppError::Conflict("error.dns_management_disabled").into());
    }
    if !matches!(d.status.as_str(), "active" | "grace") {
        return Err(AppError::Validation("error.domain_not_active").into());
    }
    let items = s.domains().replace_dns(id, &b.records).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.dns_changed", Some(&id.to_string()), u.ip.as_deref(), json!({ "records": items.len() })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.dns_changed", json!({ "domain_id": id, "records": items.len(), "by_staff": false })));
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(get, path = "/api/v1/admin/domains/{id}/dns", responses((status = 200)))]
pub async fn admin_dns(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    s.domains().get(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "items": s.domains().dns_records(id).await?, "types": db::DNS_TYPES })))
}

#[utoipa::path(put, path = "/api/v1/admin/domains/{id}/dns", request_body = DnsBody, responses((status = 200), (status = 422)))]
pub async fn admin_dns_put(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>, Json(b): Json<DnsBody>) -> ApiResult<Json<Value>> {
    s.domains().get(id).await?.ok_or(AppError::NotFound)?;
    let items = s.domains().replace_dns(id, &b.records).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.dns_changed", Some(&id.to_string()), u.ip.as_deref(), json!({ "records": items.len(), "by": "admin" })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("domain.dns_changed", json!({ "domain_id": id, "records": items.len(), "by_staff": true })));
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddonsBody {
    pub dns_management: Option<bool>,
    pub privacy: Option<bool>,
    pub registrar_lock: Option<bool>,
}

/// Toggle domain add-ons (DNS management, ID protection) and the transfer
/// lock. Paid add-ons are billed with the next renewal.
#[utoipa::path(put, path = "/api/v1/client/domains/{id}/addons", request_body = AddonsBody, responses((status = 200), (status = 422)))]
pub async fn client_addons(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>, Json(b): Json<AddonsBody>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    if !matches!(d.status.as_str(), "active" | "pending" | "grace") {
        return Err(AppError::Validation("error.domain_not_active").into());
    }
    s.domains().set_flags(id, b.dns_management, b.registrar_lock, b.privacy).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.addons_changed", Some(&id.to_string()), u.ip.as_deref(), json!({ "dns_management": b.dns_management, "privacy": b.privacy, "registrar_lock": b.registrar_lock })).await?;
    detail(&s, s.domains().get_owned(id, c).await?).await
}

// ---- bulk ----

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BulkBody {
    pub ids: Vec<Uuid>,
    /// `nameservers` | `auto_renew` | `lock` | `renew` (client); admin also `status`, `sync`.
    pub action: String,
    #[serde(default)]
    pub nameservers: Vec<String>,
    #[serde(default)]
    pub value: Option<bool>,
    #[serde(default)]
    pub status: Option<String>,
}

async fn bulk_one(s: &AppState, d: &Domain, b: &BulkBody, ns: &[String], admin: bool) -> ApiResult<Value> {
    match b.action.as_str() {
        "nameservers" => apply_ns(s, d, ns).await.map(|_| json!(null)),
        "auto_renew" => s.domains().set_auto_renew(d.id, b.value.unwrap_or(true)).await.map(|_| json!(null)).map_err(Into::into),
        "lock" => s.domains().set_flags(d.id, None, Some(b.value.unwrap_or(true)), None).await.map(|_| json!(null)).map_err(Into::into),
        "renew" => {
            if !db::RENEWABLE.contains(&d.status.as_str()) {
                return Err(AppError::Validation("error.domain_not_active").into());
            }
            let cur = currency_of(s, d.customer_id).await?;
            let t = s.domains().tld(&d.tld, &cur).await?.ok_or(AppError::Validation("error.tld_unsupported"))?;
            let inv = s.domains().create_renewal(d, &t, 1).await?.ok_or(AppError::Conflict("error.renewal_exists"))?;
            Ok(json!({ "invoice_id": inv.id }))
        }
        "status" if admin => s.domains().admin_update(d.id, b.status.as_deref(), None, None).await.map(|_| json!(null)).map_err(Into::into),
        "sync" if admin => sync_one(s, d).await.map(|_| json!(null)),
        _ => Err(AppError::Validation("error.validation").into()),
    }
}

async fn bulk(s: &AppState, ds: Vec<Domain>, b: &BulkBody, admin: bool) -> ApiResult<Json<Value>> {
    let ns = if b.action == "nameservers" { db::normalize_ns(&b.nameservers)? } else { vec![] };
    if b.action == "nameservers" && ns.len() < 2 {
        return Err(AppError::Validation("error.nameservers_required").into());
    }
    let mut results = vec![];
    for d in ds {
        let r = bulk_one(s, &d, b, &ns, admin).await;
        results.push(match r {
            Ok(extra) => json!({ "id": d.id, "name": d.name, "ok": true, "result": extra }),
            Err(e) => json!({ "id": d.id, "name": d.name, "ok": false, "error": e.0.to_string() }),
        });
    }
    let ok = results.iter().filter(|r| r["ok"] == true).count();
    Ok(Json(json!({ "ok": ok, "failed": results.len() - ok, "results": results })))
}

fn bulk_ids(b: &BulkBody) -> ApiResult<()> {
    if b.ids.is_empty() || b.ids.len() > 200 {
        return Err(AppError::Validation("error.validation").into());
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/client/domains/bulk", request_body = BulkBody, responses((status = 200), (status = 422)))]
pub async fn client_bulk(State(s): State<AppState>, u: CurrentUser, Json(b): Json<BulkBody>) -> ApiResult<Json<Value>> {
    bulk_ids(&b)?;
    let c = customer_for(&s, &u, "domains").await?;
    let mut ds = vec![];
    for id in &b.ids {
        ds.push(s.domains().get_owned(*id, c).await?);
    }
    let out = bulk(&s, ds, &b, false).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.bulk", None, u.ip.as_deref(), json!({ "action": b.action, "count": b.ids.len() })).await?;
    Ok(out)
}

#[utoipa::path(post, path = "/api/v1/admin/domains/bulk", request_body = BulkBody, responses((status = 200), (status = 422)))]
pub async fn admin_bulk(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<BulkBody>) -> ApiResult<Json<Value>> {
    bulk_ids(&b)?;
    let mut ds = vec![];
    for id in &b.ids {
        ds.push(s.domains().get(*id).await?.ok_or(AppError::NotFound)?);
    }
    let out = bulk(&s, ds, &b, true).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.bulk", None, u.ip.as_deref(), json!({ "action": b.action, "count": b.ids.len(), "by": "admin" })).await?;
    Ok(out)
}

/// Pull the registrar's view (expiry, status, nameservers) into the database.
pub async fn sync_one(s: &AppState, d: &Domain) -> ApiResult<()> {
    let reg = crate::addons::registrar(s, &d.registrar).await?.ok_or(AppError::Validation("error.registrar_unavailable"))?;
    let info = reg.info(&d.name).await.map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    let exp = time::Date::parse(&info.expires_on, &time::format_description::well_known::Iso8601::DATE).map_err(|_| AppError::Validation("error.registrar_rejected"))?;
    // Keep our grace/redemption tracking; only adopt hard registrar states.
    let status = match info.status.as_str() {
        "active" if exp >= time::OffsetDateTime::now_utc().date() => Some("active"),
        "transferring" | "cancelled" => Some(info.status.as_str()),
        _ => None,
    };
    s.domains().mark_synced(d.id, exp, status, None).await?;
    if !info.nameservers.is_empty() {
        s.domains().set_nameservers(d.id, &info.nameservers).await?;
    }
    Ok(())
}

// ---- TLD pricing tools ----

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AdjustBody {
    #[serde(default)]
    pub tlds: Vec<String>,
    pub currency: Option<String>,
    /// Percent change, e.g. `"10"` = +10%, `"-5"` = -5%.
    #[schema(value_type = String)]
    pub percent: rust_decimal::Decimal,
}

#[utoipa::path(post, path = "/api/v1/admin/tlds/bulk-adjust", request_body = AdjustBody, responses((status = 200), (status = 422)))]
pub async fn admin_tld_adjust(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<AdjustBody>) -> ApiResult<Json<Value>> {
    let n = s.domains().bulk_adjust_prices(&b.tlds, b.currency.as_deref(), b.percent).await?;
    audit::record(&s.pool, Some(u.user_id), "tld.bulk_adjusted", None, u.ip.as_deref(), json!({ "percent": b.percent, "tlds": b.tlds, "currency": b.currency, "rows": n })).await?;
    Ok(Json(json!({ "updated": n })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CopyBody {
    pub from: String,
    pub to: String,
    #[schema(value_type = String)]
    pub rate: rust_decimal::Decimal,
    #[serde(default)]
    pub overwrite: bool,
}

#[utoipa::path(post, path = "/api/v1/admin/tlds/copy-currency", request_body = CopyBody, responses((status = 200), (status = 422)))]
pub async fn admin_tld_copy(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Json(b): Json<CopyBody>) -> ApiResult<Json<Value>> {
    let ok = |c: &str| c.len() == 3 && c.bytes().all(|x| x.is_ascii_alphabetic());
    if !ok(&b.from) || !ok(&b.to) || b.from.eq_ignore_ascii_case(&b.to) {
        return Err(AppError::Validation("error.validation").into());
    }
    let n = s.domains().copy_currency(&b.from, &b.to, b.rate, b.overwrite).await?;
    audit::record(&s.pool, Some(u.user_id), "tld.currency_copied", None, u.ip.as_deref(), json!({ "from": b.from, "to": b.to, "rate": b.rate, "rows": n })).await?;
    Ok(Json(json!({ "updated": n })))
}

// ---- public (storefront) ----

fn ip_of(ext: &axum::http::Extensions) -> String {
    ext.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|c| c.0.ip().to_string()).unwrap_or_else(|| "unknown".into())
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PublicTldQuery {
    pub currency: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/public/domains/tlds", params(PublicTldQuery), responses((status = 200)))]
pub async fn public_tlds(State(s): State<AppState>, Query(q): Query<PublicTldQuery>) -> ApiResult<Json<Value>> {
    let cur = q.currency.map(|c| c.to_ascii_uppercase());
    Ok(Json(json!({ "items": s.domains().tlds(true, cur.as_deref()).await? })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PublicCheckQuery {
    pub name: String,
    pub currency: Option<String>,
    /// Also check these TLDs for the same label (suggestions), max 10.
    #[serde(default)]
    pub suggest: bool,
}

/// Guest domain search for the storefront: availability (via the TLD's
/// registrar addon) plus pricing; rate limited per IP.
#[utoipa::path(get, path = "/api/v1/public/domains/check", params(PublicCheckQuery), responses((status = 200), (status = 422), (status = 429)))]
pub async fn public_check(State(s): State<AppState>, ext: axum::http::Extensions, Query(q): Query<PublicCheckQuery>) -> ApiResult<Json<Value>> {
    if !s.auth_limiter.check(&format!("domaincheck:{}", ip_of(&ext))) {
        return Err(AppError::RateLimited.into());
    }
    let cur = q.currency.clone().unwrap_or_else(|| "EUR".into()).to_ascii_uppercase();
    let (name, tld) = db::normalize(&q.name)?;
    let label = name.split('.').next().unwrap_or_default().to_string();
    let all = s.domains().tlds(true, Some(&cur)).await?;
    let mut targets: Vec<(String, db::Tld)> = all.iter().filter(|t| t.tld == tld).map(|t| (name.clone(), t.clone())).collect();
    if q.suggest {
        targets.extend(all.iter().filter(|t| t.tld != tld).take(10).map(|t| (format!("{label}.{}", t.tld), t.clone())));
    }
    let mut results = vec![];
    for (n, t) in targets {
        let available = match crate::addons::registrar(&s, &t.registrar).await? {
            Some(reg) => !s.domains().name_taken(&n).await? && reg.available(&n).await.unwrap_or(false),
            None => false,
        };
        results.push(json!({ "name": n, "tld": t.tld, "available": available, "pricing": t }));
    }
    let supported = results.first().is_some_and(|r| r["tld"] == tld.as_str());
    Ok(Json(json!({ "name": name, "tld": tld, "supported": supported, "currency": cur, "results": results })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct WhoisQuery {
    pub name: String,
}

/// Public WHOIS (RDAP) lookup. The RDAP server is the `domains.rdap_base`
/// setting (default `https://rdap.org`); rate limited per IP.
#[utoipa::path(get, path = "/api/v1/public/domains/whois", params(WhoisQuery), responses((status = 200), (status = 422), (status = 429)))]
pub async fn public_whois(State(s): State<AppState>, ext: axum::http::Extensions, Query(q): Query<WhoisQuery>) -> ApiResult<Json<Value>> {
    if !s.auth_limiter.check(&format!("whois:{}", ip_of(&ext))) {
        return Err(AppError::RateLimited.into());
    }
    let (name, _) = db::normalize(&q.name)?;
    let base = pelty_database::settings::get(&s.pool, "domains.rdap_base")
        .await?
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|b| b.starts_with("https://"))
        .unwrap_or_else(|| "https://rdap.org".into());
    let t = s.registrars.transport();
    let info = pelty_provisioning::registrars::rdap_lookup(t.as_ref(), &base, &name).await.map_err(|_| AppError::Validation("error.whois_unavailable"))?;
    Ok(Json(json!({ "name": name, "whois": info })))
}

#[utoipa::path(get, path = "/api/v1/admin/settings/domain-defaults", responses((status = 200), (status = 403)))]
pub async fn get_defaults(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(pelty_database::domain_defaults::get(&s.pool).await?)))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/domain-defaults", responses((status = 200), (status = 422)))]
pub async fn put_defaults(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<pelty_database::domain_defaults::DomainDefaults>,
) -> ApiResult<Json<Value>> {
    let d = pelty_database::domain_defaults::put(&s.pool, b).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.domain_defaults_updated", None, u.ip.as_deref(), json!({ "nameservers": d.nameservers, "records": d.dns_template.len() })).await?;
    Ok(Json(json!(d)))
}

// ICANN registrant verification

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TokenBody {
    pub token: String,
}

/// E-mail link target: confirms the registrant address (all its pending domains).
#[utoipa::path(post, path = "/api/v1/public/domains/verify-registrant", request_body = TokenBody, responses((status = 200), (status = 422)))]
pub async fn public_verify_registrant(State(s): State<AppState>, Json(b): Json<TokenBody>) -> ApiResult<Json<Value>> {
    let n = crate::registrant_verify::verify(&s, b.token.trim()).await?;
    if n == 0 {
        return Err(AppError::Validation("error.registrant_token_invalid").into());
    }
    Ok(Json(json!({ "verified": n })))
}

#[utoipa::path(post, path = "/api/v1/client/domains/{id}/registrant-verification/resend", responses((status = 200), (status = 409)))]
pub async fn client_rv_resend(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let c = customer_for(&s, &u, "domains").await?;
    let d = s.domains().get_owned(id, c).await?;
    let cur = pelty_database::registrant_verify::get(&s.pool, id).await?.ok_or(AppError::Conflict("error.registrant_nothing_pending"))?;
    if cur.status == "verified" {
        return Err(AppError::Conflict("error.registrant_nothing_pending").into());
    }
    if cur.sent_at > time::OffsetDateTime::now_utc() - time::Duration::minutes(10) {
        return Err(AppError::Conflict("error.registrant_resend_too_soon").into());
    }
    let v = crate::registrant_verify::resend(&s, &d, false).await?;
    audit::record(&s.pool, Some(u.user_id), "domain.registrant_verification_resent", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "registrant_verification": v })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RvAction {
    /// `verify` (staff confirmed it), `resend`, or `restart` (new request, new deadline).
    pub action: String,
}

#[utoipa::path(post, path = "/api/v1/admin/domains/{id}/registrant-verification", request_body = RvAction, responses((status = 200), (status = 422)))]
pub async fn admin_rv_action(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::ServicesManage>, Path(id): Path<Uuid>, Json(b): Json<RvAction>) -> ApiResult<Json<Value>> {
    let d = s.domains().get(id).await?.ok_or(AppError::NotFound)?;
    let today = time::OffsetDateTime::now_utc().date();
    match b.action.as_str() {
        "verify" => {
            pelty_database::registrant_verify::admin_verify(&s.pool, id).await?;
        }
        "resend" => {
            crate::registrant_verify::resend(&s, &d, false).await?;
        }
        "restart" => {
            crate::registrant_verify::require(&s, &d, today, true).await?;
        }
        _ => return Err(AppError::Validation("error.invalid_action").into()),
    }
    audit::record(&s.pool, Some(u.user_id), "domain.registrant_verification_admin", Some(&id.to_string()), u.ip.as_deref(), json!({ "action": b.action })).await?;
    detail(&s, s.domains().get(id).await?.ok_or(AppError::NotFound)?).await
}

#[utoipa::path(get, path = "/api/v1/admin/settings/registrant-verification", responses((status = 200)))]
pub async fn admin_rv_settings(State(s): State<AppState>, _p: RequirePermission<perms::ServicesManage>) -> ApiResult<Json<Value>> {
    let (pending, overdue, verified) = pelty_database::registrant_verify::counts(&s.pool).await?;
    Ok(Json(json!({ "enabled": crate::registrant_verify::enabled(&s).await?, "deadline_days": crate::registrant_verify::DEADLINE_DAYS, "pending": pending, "overdue": overdue, "verified": verified })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RvSettings {
    pub enabled: bool,
}

#[utoipa::path(put, path = "/api/v1/admin/settings/registrant-verification", request_body = RvSettings, responses((status = 200)))]
pub async fn admin_rv_settings_put(State(s): State<AppState>, RequirePermission(u, p): RequirePermission<perms::ServicesManage>, Json(b): Json<RvSettings>) -> ApiResult<Json<Value>> {
    crate::registrant_verify::set_enabled(&s, b.enabled).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.registrant_verification", None, u.ip.as_deref(), json!({ "enabled": b.enabled })).await?;
    admin_rv_settings(State(s), RequirePermission(u, p)).await
}
