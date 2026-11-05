//! Themes, plugins and gateway administration.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use serde::Deserialize;
use serde_json::{json, Value};

pub const ACTIVE_THEME_KEY: &str = "theme.active";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/themes", get(list_themes))
        .route("/themes/active", get(active_theme))
        .route("/themes/{id}/assets/{*file}", get(theme_asset))
        .route("/admin/themes/active", put(set_active_theme))
        .route("/admin/plugins", get(list_plugins))
        .route("/admin/plugins/{id}", put(set_plugin))
        .route(
            "/admin/gateways/{id}/rules",
            get(get_gateway_rules).put(put_gateway_rules),
        )
}

/// Active theme id, served from the in-memory cache (loaded at startup and
/// updated on activation) so hot paths never hit the database.
async fn active_id(s: &AppState) -> ApiResult<String> {
    Ok(s.active_theme())
}

pub async fn load_active_id(s: &AppState) -> pelty_core::AppResult<String> {
    let v = pelty_database::settings::get(&s.pool, ACTIVE_THEME_KEY).await?;
    let id = v
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "default".into());
    Ok(if s.themes.get(&id).is_some() {
        id
    } else {
        "default".into()
    })
}

#[utoipa::path(get, path = "/api/v1/themes", responses((status = 200)))]
pub async fn list_themes(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": s.themes.list(), "active": active_id(&s).await? }),
    ))
}

/// Active theme, sanitized: whitelisted tokens with validated CSS values.
#[utoipa::path(get, path = "/api/v1/themes/active", responses((status = 200)))]
pub async fn active_theme(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let id = active_id(&s).await?;
    Ok(Json(json!(s.themes.get(&id))))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ActivateTheme {
    pub id: String,
}

#[utoipa::path(put, path = "/api/v1/admin/themes/active", request_body = ActivateTheme, responses((status = 204), (status = 403), (status = 422)))]
pub async fn set_active_theme(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<ActivateTheme>,
) -> ApiResult<StatusCode> {
    if s.themes.get(&b.id).is_none() {
        return Err(AppError::Validation("error.unknown_theme").into());
    }
    pelty_database::settings::set(&s.pool, ACTIVE_THEME_KEY, json!(b.id)).await?;
    s.set_active_theme(&b.id);
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "theme.activated",
        Some(&b.id),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn theme_asset(
    State(s): State<AppState>,
    Path((id, file)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> ApiResult<Response> {
    let path = s.themes.asset_path(&id, &file).ok_or(AppError::NotFound)?;
    let mut res =
        crate::static_files::serve_file(&path, &headers, "public, max-age=3600, must-revalidate")
            .await
            .ok_or(AppError::NotFound)?;
    // SVGs must not run script even if opened directly.
    res.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; sandbox",
        ),
    );
    Ok(res)
}

#[utoipa::path(get, path = "/api/v1/admin/plugins", responses((status = 200), (status = 403)))]
pub async fn list_plugins(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
) -> Json<Value> {
    Json(json!({ "items": s.plugins.list(), "event_logger_recent": s.event_logger.recent().len() }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetPlugin {
    pub enabled: bool,
}

#[utoipa::path(put, path = "/api/v1/admin/plugins/{id}", request_body = SetPlugin, responses((status = 204), (status = 404)))]
pub async fn set_plugin(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Json(b): Json<SetPlugin>,
) -> ApiResult<StatusCode> {
    if !s.plugins.exists(&id) {
        return Err(AppError::NotFound.into());
    }
    pelty_database::extensions::set_plugin_enabled(&s.pool, &id, b.enabled).await?;
    s.plugins.set_enabled(&id, b.enabled);
    let action = if b.enabled {
        "plugin.enabled"
    } else {
        "plugin.disabled"
    };
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        action,
        Some(&id),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn gateway_known(s: &AppState, id: &str) -> ApiResult<()> {
    if s.gateways.descriptor(id).is_some()
        || pelty_database::extensions::gateway(&s.pool, id)
            .await?
            .is_some()
    {
        Ok(())
    } else {
        Err(AppError::NotFound.into())
    }
}

/// Fee, amount-limit and currency rules for one gateway.
#[utoipa::path(get, path = "/api/v1/admin/gateways/{id}/rules", responses((status = 200, body = pelty_database::gateway_rules::GatewayRules), (status = 403), (status = 404)))]
pub async fn get_gateway_rules(
    State(s): State<AppState>,
    _p: RequirePermission<perms::GatewaysManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<pelty_database::gateway_rules::GatewayRules>> {
    gateway_known(&s, &id).await?;
    Ok(Json(
        pelty_database::gateway_rules::get(&s.pool, &id).await?,
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/gateways/{id}/rules", request_body = pelty_database::gateway_rules::GatewayRules, responses((status = 200, body = pelty_database::gateway_rules::GatewayRules), (status = 403), (status = 404), (status = 422)))]
pub async fn put_gateway_rules(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GatewaysManage>,
    Path(id): Path<String>,
    Json(b): Json<pelty_database::gateway_rules::GatewayRules>,
) -> ApiResult<Json<pelty_database::gateway_rules::GatewayRules>> {
    gateway_known(&s, &id).await?;
    let r = b.normalized().map_err(AppError::Validation)?;
    pelty_database::gateway_rules::put(&s.pool, &id, &r).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "gateway.rules_updated",
        Some(&id),
        u.ip.as_deref(),
        json!(r),
    )
    .await?;
    Ok(Json(r))
}
