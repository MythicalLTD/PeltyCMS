//! Digital products (module `digital_products`): admin settings, versions and
//! licenses (`catalog.manage`); customer licenses and expiring download links;
//! the public license validation API used by the sold software; and the
//! purchase/refund hooks that issue and revoke licenses and asset grants.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::routes::downloads::sniff;
use crate::routes::ticket_attachments::{clean_name, serve};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::digital::{self as dg, Check, LicenseFilter, Settings, Verdict};
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use uuid::Uuid;

pub const MODULE: &str = "digital_products";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/digital/products/{id}/versions",
            get(admin_versions).post(upload_version),
        )
        .layer(DefaultBodyLimit::max(
            pelty_core::config::upload_limits().download + 1024,
        ))
        .route("/admin/digital/products", get(admin_products))
        .route(
            "/admin/digital/products/{id}",
            get(admin_settings)
                .put(put_settings)
                .delete(delete_settings),
        )
        .route("/admin/digital/versions/{id}", delete(delete_version))
        .route(
            "/admin/digital/licenses",
            get(admin_licenses).post(create_license),
        )
        .route("/admin/digital/licenses/{id}", get(admin_license))
        .route("/admin/digital/licenses/{id}/status", post(license_status))
        .route("/admin/digital/licenses/{id}/reset", post(admin_reset))
        .route("/admin/digital/grants", get(admin_grants))
        .route("/admin/digital/grants/{id}/granted", post(grant_done))
        .route("/client/digital/products", get(my_products))
        .route("/client/digital/licenses", get(my_licenses))
        .route("/client/digital/licenses/{id}/reset", post(my_reset))
        .route("/client/digital/versions/{id}/link", post(my_link))
        .route("/public/digital/download/{token}", get(download))
        .route("/public/digital/products/{id}/changelog", get(changelog))
        .route("/public/licenses/validate", post(validate))
        .route("/public/licenses/deactivate", post(deactivate))
}

fn reveal(s: &AppState, sealed: &[u8]) -> Option<String> {
    let sb = s.secret_box.as_ref()?;
    String::from_utf8(sb.open(sealed)?).ok()
}

fn seal(s: &AppState, key: &str) -> ApiResult<Vec<u8>> {
    Ok(s.secret_box
        .as_ref()
        .ok_or(AppError::Conflict("error.encryption_key_missing"))?
        .seal(key.as_bytes()))
}

// ------------------------------------------------------------------ admin

#[utoipa::path(get, path = "/api/v1/admin/digital/products", responses((status = 200, body = Vec<dg::ProductRow>), (status = 403)))]
pub async fn admin_products(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": dg::products(&s.pool).await? })))
}

/// Digital settings of a product (defaults when it is not digital yet).
#[utoipa::path(get, path = "/api/v1/admin/digital/products/{id}", responses((status = 200, body = Settings), (status = 403)))]
pub async fn admin_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let cur = dg::settings(&s.pool, id).await?;
    let enabled = cur.is_some();
    let mut v = cur.unwrap_or_default();
    v.product_id = id;
    Ok(Json(json!({ "enabled": enabled, "settings": v })))
}

/// Make a product digital (or update it): licensing, binding, link lifetime, escrow asset.
#[utoipa::path(put, path = "/api/v1/admin/digital/products/{id}", request_body = Settings, responses((status = 200, body = Settings), (status = 404), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<Settings>,
) -> ApiResult<Json<Settings>> {
    let r = dg::save_settings(&s.pool, id, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "digital_product.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "license": r.license_enabled, "max_activations": r.max_activations }),
    )
    .await?;
    Ok(Json(r))
}

#[utoipa::path(delete, path = "/api/v1/admin/digital/products/{id}", responses((status = 200), (status = 403)))]
pub async fn delete_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    dg::delete_settings(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "digital_product.removed",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/digital/products/{id}/versions", responses((status = 200, body = Vec<dg::Version>)))]
pub async fn admin_versions(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": dg::versions(&s.pool, id).await? })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct VersionQuery {
    pub version: String,
    pub filename: String,
    #[serde(default)]
    pub changelog: String,
}

/// Upload a new version (raw body, same types and size limit as Downloads).
#[utoipa::path(post, path = "/api/v1/admin/digital/products/{id}/versions", params(VersionQuery), request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses((status = 200, body = dg::Version), (status = 404), (status = 409), (status = 422)))]
pub async fn upload_version(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Query(q): Query<VersionQuery>,
    body: Bytes,
) -> ApiResult<Json<dg::Version>> {
    let ct = sniff(&body).map_err(|c| {
        if c == "error.download.too_large" {
            AppError::BadRequest(c)
        } else {
            AppError::Validation(c)
        }
    })?;
    let name = clean_name(&q.filename);
    let v = dg::add_version(
        &s.pool,
        dg::NewVersion {
            product_id: id,
            version: q.version.trim(),
            changelog: &q.changelog,
            filename: &name,
            content_type: ct,
            data: &body,
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "digital_version.created",
        Some(&v.id.to_string()),
        u.ip.as_deref(),
        json!({ "product_id": id, "version": v.version, "size": body.len() }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "digital.version_released",
        json!({ "product_id": id, "version_id": v.id, "version": v.version }),
    ));
    Ok(Json(v))
}

#[utoipa::path(delete, path = "/api/v1/admin/digital/versions/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_version(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    dg::delete_version(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "digital_version.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/digital/licenses", params(LicenseFilter), responses((status = 200, body = Vec<dg::License>)))]
pub async fn admin_licenses(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Query(f): Query<LicenseFilter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": dg::licenses(&s.pool, &f, 500).await? }),
    ))
}

/// One license with its key and activations.
#[utoipa::path(get, path = "/api/v1/admin/digital/licenses/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_license(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let l = dg::license(&s.pool, id).await?;
    let key = reveal(&s, &l.key_sealed);
    Ok(Json(
        json!({ "license": l, "key": key, "activations": dg::activations(&s.pool, id).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewLicenseBody {
    pub product_id: Uuid,
    pub customer_id: Uuid,
    /// Defaults to the product's limit.
    pub max_activations: Option<i32>,
    pub days: Option<i32>,
    #[serde(default)]
    pub note: String,
}

/// Issue a license by hand (gifts, migrations, support).
#[utoipa::path(post, path = "/api/v1/admin/digital/licenses", request_body = NewLicenseBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn create_license(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Json(b): Json<NewLicenseBody>,
) -> ApiResult<Json<Value>> {
    let set = dg::settings(&s.pool, b.product_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if b.days.is_some_and(|d| !(1..=36500).contains(&d)) {
        return Err(AppError::Validation("error.digital_settings_invalid").into());
    }
    let key = dg::generate_key();
    let sealed = seal(&s, &key)?;
    let expires = b
        .days
        .or(set.license_days)
        .map(|d| time::OffsetDateTime::now_utc() + time::Duration::days(d.into()));
    let id = dg::issue(
        &s.pool,
        dg::NewLicense {
            product_id: b.product_id,
            customer_id: b.customer_id,
            service_id: None,
            key: &key,
            key_sealed: &sealed,
            max_activations: b.max_activations.unwrap_or(set.max_activations),
            expires_at: expires,
            note: b.note.trim(),
        },
    )
    .await?
    .ok_or(AppError::Conflict("error.conflict"))?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "license.issued",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "product_id": b.product_id, "customer_id": b.customer_id }),
    )
    .await?;
    s.emit(DomainEvent::new(
        "license.issued",
        json!({ "license_id": id, "product_id": b.product_id, "customer_id": b.customer_id }),
    ));
    Ok(Json(json!({ "id": id, "key": key })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StatusBody {
    /// active, suspended or revoked.
    pub status: String,
}

#[utoipa::path(post, path = "/api/v1/admin/digital/licenses/{id}/status", request_body = StatusBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn license_status(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    dg::set_status(&s.pool, id, &b.status).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "license.status",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": b.status }),
    )
    .await?;
    if b.status == "revoked" {
        s.emit(DomainEvent::new(
            "license.revoked",
            json!({ "license_id": id }),
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/digital/licenses/{id}/reset", responses((status = 200), (status = 404)))]
pub async fn admin_reset(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let n = dg::reset_activations(&s.pool, id, None).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "license.reset",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "removed": n }),
    )
    .await?;
    Ok(Json(json!({ "removed": n })))
}

#[utoipa::path(get, path = "/api/v1/admin/digital/grants", responses((status = 200, body = Vec<dg::Grant>)))]
pub async fn admin_grants(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": dg::grants(&s.pool, 200).await? })))
}

/// Confirm a pending external asset grant was fulfilled (by hand or by an integration).
#[utoipa::path(post, path = "/api/v1/admin/digital/grants/{id}/granted", responses((status = 200), (status = 409)))]
pub async fn grant_done(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    dg::mark_granted(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "digital_grant.granted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ client

/// Digital products the customer owns, with their versions and changelogs.
#[utoipa::path(get, path = "/api/v1/client/digital/products", responses((status = 200)))]
pub async fn my_products(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let mut items = vec![];
    for p in dg::owned(&s.pool, c).await? {
        let versions = dg::versions(&s.pool, p.product_id).await?;
        items.push(json!({ "product_id": p.product_id, "name": p.name, "versions": versions }));
    }
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(get, path = "/api/v1/client/digital/licenses", responses((status = 200)))]
pub async fn my_licenses(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let f = LicenseFilter {
        customer_id: Some(c),
        ..Default::default()
    };
    let mut items = vec![];
    for l in dg::licenses(&s.pool, &f, 200).await? {
        let key = reveal(&s, &l.key_sealed);
        let acts = dg::activations(&s.pool, l.id).await?;
        items.push(json!({ "license": l, "key": key, "activations": acts }));
    }
    Ok(Json(json!({ "items": items })))
}

/// Free every activation of the customer's own license (moving servers).
#[utoipa::path(post, path = "/api/v1/client/digital/licenses/{id}/reset", responses((status = 200), (status = 404)))]
pub async fn my_reset(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    if !s
        .auth_limiter
        .check(&format!("license-reset:{}", u.user_id))
    {
        return Err(AppError::RateLimited.into());
    }
    let n = dg::reset_activations(&s.pool, id, Some(c)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "license.reset",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "removed": n, "by_customer": true }),
    )
    .await?;
    Ok(Json(json!({ "removed": n })))
}

/// A one-off download link that expires (product setting, default 15 minutes).
#[utoipa::path(post, path = "/api/v1/client/digital/versions/{id}/link", responses((status = 200), (status = 404)))]
pub async fn my_link(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    if !dg::entitled(&s.pool, c, id).await? {
        return Err(AppError::NotFound.into());
    }
    let token = pelty_auth::token::generate();
    let expires = dg::create_link(&s.pool, &pelty_auth::token::hash(&token), id, c).await?;
    Ok(Json(json!({
        "url": format!("/api/v1/public/digital/download/{token}"),
        "expires_at": expires.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
    })))
}

// ------------------------------------------------------------------ public

/// Serve a file for a valid link (expires, limited uses, ownership re-checked).
#[utoipa::path(get, path = "/api/v1/public/digital/download/{token}", responses((status = 200), (status = 404)))]
pub async fn download(State(s): State<AppState>, Path(token): Path<String>) -> ApiResult<Response> {
    if token.len() > 200 {
        return Err(AppError::NotFound.into());
    }
    let (n, ct, d) = dg::redeem_link(&s.pool, &pelty_auth::token::hash(&token)).await?;
    serve(n, ct, d)
}

/// Public changelog (versions without files) of a digital product.
#[utoipa::path(get, path = "/api/v1/public/digital/products/{id}/changelog", responses((status = 200), (status = 404)))]
pub async fn changelog(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    dg::settings(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let items: Vec<Value> = dg::versions(&s.pool, id)
        .await?
        .into_iter()
        .map(|v| json!({ "version": v.version, "changelog": v.changelog, "size_bytes": v.size_bytes, "sha256": v.sha256, "created_at": v.created_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default() }))
        .collect();
    Ok(Json(json!({ "items": items })))
}

fn peer(ext: &axum::http::Extensions) -> Option<String> {
    ext.get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
}

/// License check for sold software. Bound fields (domain, IP, HWID) must be
/// sent when the product binds them; a new identity takes an activation slot.
/// IP falls back to the caller's address. Unknown keys are rate limited.
#[utoipa::path(post, path = "/api/v1/public/licenses/validate", request_body = Check, responses((status = 200, body = Verdict), (status = 429)))]
pub async fn validate(
    State(s): State<AppState>,
    ext: axum::http::Extensions,
    Json(b): Json<Check>,
) -> ApiResult<Json<Verdict>> {
    let ip = peer(&ext);
    let v = dg::validate(&s.pool, &b, ip.as_deref()).await?;
    if v.reason == Some("not_found")
        && !s
            .auth_limiter
            .check(&format!("license:{}", ip.as_deref().unwrap_or("unknown")))
    {
        return Err(AppError::RateLimited.into());
    }
    Ok(Json(v))
}

/// Free the activation of this identity (uninstall, server move).
#[utoipa::path(post, path = "/api/v1/public/licenses/deactivate", request_body = Check, responses((status = 200)))]
pub async fn deactivate(
    State(s): State<AppState>,
    ext: axum::http::Extensions,
    Json(b): Json<Check>,
) -> ApiResult<Json<Value>> {
    let ip = peer(&ext);
    let removed = dg::deactivate(&s.pool, &b, ip.as_deref()).await?;
    if !removed
        && !s
            .auth_limiter
            .check(&format!("license:{}", ip.as_deref().unwrap_or("unknown")))
    {
        return Err(AppError::RateLimited.into());
    }
    Ok(Json(json!({ "removed": removed })))
}

// ------------------------------------------------------------------ hooks

/// Event bus hook: issue on purchase, follow suspend/unsuspend, revoke on
/// termination, refund and chargeback.
pub async fn on_event(s: &AppState, e: &DomainEvent) {
    if !matches!(
        e.name,
        "service.created"
            | "service.suspended"
            | "service.unsuspended"
            | "service.terminated"
            | "service.cancelled"
            | "order.refunded"
            | "invoice.refunded"
            | "payment.disputed"
    ) {
        return;
    }
    match pelty_database::feature_modules::is_enabled(&s.pool, MODULE).await {
        Ok(true) => {}
        _ => return,
    }
    if let Err(err) = handle_event(s, e).await {
        tracing::warn!(event = e.name, error = %err, "digital products hook failed");
    }
}

async fn handle_event(s: &AppState, e: &DomainEvent) -> pelty_core::AppResult<()> {
    use pelty_database::game_delivery as gd;
    let id = |k: &str| e.payload[k].as_str().and_then(|v| v.parse::<Uuid>().ok());
    let services = match e.name {
        "order.refunded" => match id("order_id") {
            Some(o) => gd::services_for(&s.pool, "order", o).await?,
            None => vec![],
        },
        "invoice.refunded" => match id("invoice_id") {
            Some(i) => gd::services_for(&s.pool, "invoice", i).await?,
            None => vec![],
        },
        "payment.disputed" => match id("payment_id") {
            Some(p) => gd::services_for(&s.pool, "payment", p).await?,
            None => vec![],
        },
        _ => id("service_id").into_iter().collect(),
    };
    for sid in services {
        match e.name {
            "service.created" => issue_for_service(s, sid).await?,
            "service.suspended" => {
                dg::set_status_for_service(&s.pool, sid, "suspended").await?;
            }
            "service.unsuspended" => {
                dg::set_status_for_service(&s.pool, sid, "active").await?;
            }
            _ => {
                if dg::set_status_for_service(&s.pool, sid, "revoked").await? > 0 {
                    s.emit(DomainEvent::new(
                        "license.revoked",
                        json!({ "service_id": sid, "reason": e.name }),
                    ));
                }
                for g in dg::revoke_grants(&s.pool, sid).await? {
                    s.emit(DomainEvent::new("digital.asset_revoked", json!({ "grant_id": g.id, "asset": g.asset, "identity": g.identity, "customer_id": g.customer_id, "service_id": sid })));
                }
            }
        }
    }
    Ok(())
}

async fn issue_for_service(s: &AppState, sid: Uuid) -> pelty_core::AppResult<()> {
    let Some(ctx) = pelty_database::game_delivery::service_ctx(&s.pool, sid).await? else {
        return Ok(());
    };
    let Some(set) = dg::settings(&s.pool, ctx.product_id).await? else {
        return Ok(());
    };
    if set.license_enabled {
        if let Some(sb) = s.secret_box.as_ref() {
            let key = dg::generate_key();
            let expires = set
                .license_days
                .map(|d| time::OffsetDateTime::now_utc() + time::Duration::days(d.into()));
            let sealed = sb.seal(key.as_bytes());
            if let Some(lid) = dg::issue(
                &s.pool,
                dg::NewLicense {
                    product_id: ctx.product_id,
                    customer_id: ctx.customer_id,
                    service_id: Some(sid),
                    key: &key,
                    key_sealed: &sealed,
                    max_activations: set.max_activations,
                    expires_at: expires,
                    note: "",
                },
            )
            .await?
            {
                s.emit(DomainEvent::new("license.issued", json!({ "license_id": lid, "product_id": ctx.product_id, "customer_id": ctx.customer_id, "service_id": sid })));
            }
        } else {
            tracing::warn!(service = %sid, "license not issued: encryption key missing");
        }
    }
    if !set.escrow_asset.is_empty() {
        // FiveM escrow: the Cfx.re license identifier the customer linked, if any.
        let identity = pelty_database::games::identity(&s.pool, ctx.customer_id, "fivem")
            .await?
            .remove("license")
            .unwrap_or_default();
        if let Some(gid) = dg::add_grant(
            &s.pool,
            ctx.product_id,
            ctx.customer_id,
            sid,
            &set.escrow_asset,
            &identity,
        )
        .await?
        {
            s.emit(DomainEvent::new("digital.asset_granted", json!({ "grant_id": gid, "asset": set.escrow_asset, "identity": identity, "customer_id": ctx.customer_id, "service_id": sid })));
        }
    }
    Ok(())
}
