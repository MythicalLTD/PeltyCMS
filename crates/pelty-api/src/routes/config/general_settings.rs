//! General Settings (WHMCS Setup → General Settings) and admin-only custom code.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use pelty_database::general_settings::{self as gs, CustomCode, GeneralSettings};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/general-settings",
            get(get_settings).put(put_settings),
        )
        .route("/admin/custom-code", get(get_code).put(put_code))
        .route("/public/general", get(public_general))
}

#[utoipa::path(get, path = "/api/v1/admin/general-settings", responses((status = 200, body = GeneralSettings), (status = 403)))]
pub async fn get_settings(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "settings": gs::load(&s.pool).await?, "date_formats": gs::DATE_FORMATS, "order_forms": gs::ORDER_FORMS }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/general-settings", request_body = GeneralSettings, responses((status = 200), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<GeneralSettings>,
) -> ApiResult<Json<Value>> {
    gs::save(&s.pool, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.general_updated",
        None,
        u.ip.as_deref(),
        json!({ "maintenance": b.maintenance.enabled }),
    )
    .await?;
    Ok(Json(json!({ "settings": b })))
}

#[utoipa::path(get, path = "/api/v1/admin/custom-code", responses((status = 200, body = CustomCode), (status = 403)))]
pub async fn get_code(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::CustomCodeManage>,
) -> ApiResult<Json<CustomCode>> {
    Ok(Json(gs::load_code(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/custom-code", request_body = CustomCode, responses((status = 200), (status = 403), (status = 422)))]
pub async fn put_code(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CustomCodeManage>,
    Json(b): Json<CustomCode>,
) -> ApiResult<Json<CustomCode>> {
    gs::save_code(&s.pool, &b).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.custom_code_updated",
        None,
        u.ip.as_deref(),
        json!({ "css": b.css.len(), "head_js": b.head_js.len(), "footer_js": b.footer_js.len() }),
    )
    .await?;
    Ok(Json(b))
}

/// Public subset: maintenance banner, localisation, ordering flags, social links, custom code.
#[utoipa::path(get, path = "/api/v1/public/general", responses((status = 200)))]
pub async fn public_general(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let g = gs::load(&s.pool).await?;
    let c = gs::load_code(&s.pool).await?;
    Ok(Json(json!({
        "company_name": g.general.company_name, "logo_url": g.general.logo_url,
        "maintenance": g.maintenance, "localisation": g.localisation, "ordering": g.ordering, "support": g.support,
        "urls": { "terms": g.system_urls.terms_url, "privacy": g.system_urls.privacy_url, "support": g.system_urls.support_url },
        "custom_code": c,
        // Server capabilities, so the UI hides what cannot work here.
        "features": {
            "passkeys": crate::routes::passkeys::webauthn(&s.cfg.public_url).is_ok(),
            "totp": s.secret_box.is_some(),
        },
    })))
}

/// Maintenance mode: block new orders (checkout and cart writes) with 503.
/// Admin APIs and read-only client pages keep working.
pub async fn maintenance_guard(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let p = req.uri().path();
    let writes_order = req.method() != Method::GET
        && (p == "/api/v1/client/checkout" || p.starts_with("/api/v1/store/carts"));
    if writes_order {
        if let Ok(g) = gs::load(&s.pool).await {
            if g.maintenance.enabled {
                let msg = if g.maintenance.message.is_empty() {
                    s.i18n.t("en", "error.maintenance").to_string()
                } else {
                    g.maintenance.message
                };
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": { "code": "error.maintenance", "message": msg } })),
                )
                    .into_response();
            }
        }
    }
    next.run(req).await
}
