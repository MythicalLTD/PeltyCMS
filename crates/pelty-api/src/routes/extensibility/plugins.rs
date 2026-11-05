//! Plugin extension points over HTTP:
//! - `GET  /plugins/ui`                      UI slot entries for the current user
//! - `GET  /admin/plugins/{id}/settings`     schema + current values
//! - `PUT  /admin/plugins/{id}/settings`     validated against the schema
//! - `/plugins/{id}/...`                     routes of built-in modules, only
//!   reachable while the plugin is enabled (see [`modules`]).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::AppError;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    let mut r = Router::new()
        .route("/plugins/ui", get(ui_entries))
        .route(
            "/admin/plugins/{id}/settings",
            get(get_settings).put(put_settings),
        )
        .route(
            "/admin/plugins/{id}/webhook",
            get(get_webhook).put(put_webhook),
        )
        .route(
            "/admin/plugins/{id}/webhook/secret",
            axum::routing::post(rotate_secret),
        )
        .route("/admin/plugins/{id}/deliveries", get(list_deliveries))
        .route(
            "/admin/plugins/{id}/deliveries/{did}/redeliver",
            axum::routing::post(redeliver),
        );
    for (id, module) in modules() {
        r = r.nest(&format!("/plugins/{id}"), module);
    }
    r
}

/// Built-in plugin modules and their API routes. To add one: implement the
/// `Plugin` trait in `pelty-extensions`, register it in `AppState::new`, and
/// return its router here. Routes are mounted at `/api/v1/plugins/<id>/`.
pub fn modules() -> Vec<(&'static str, Router<AppState>)> {
    vec![("event-logger", event_logger_routes())]
}

/// Module handlers must call this first: disabled plugins answer 404.
pub fn ensure_enabled(s: &AppState, id: &str) -> Result<(), AppError> {
    if s.plugins.enabled(id) {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

#[utoipa::path(get, path = "/api/v1/plugins/ui", responses((status = 200), (status = 401)))]
pub async fn ui_entries(State(s): State<AppState>, u: CurrentUser) -> Json<Value> {
    let items: Vec<Value> = s
        .plugins
        .ui_entries()
        .into_iter()
        .filter(|(_, _, e)| e.permission.as_deref().is_none_or(|p| u.has(p)))
        .map(|(id, widget, e)| {
            json!({ "plugin": id, "kind": if widget { "widget" } else { "menu" }, "slot": e.slot, "label": e.label, "path": e.path })
        })
        .collect();
    // Declarative pages and slot contributions of enabled plugins. Admin
    // pages also need `admin.access`; the data they load still goes through
    // the normal per-endpoint permission checks with the viewer's session.
    let can = |area: Option<&str>, perm: &Option<String>| {
        (area != Some("admin") || u.has("admin.access")) && perm.as_deref().is_none_or(|p| u.has(p))
    };
    let mut pages = Vec::new();
    let mut slots = Vec::new();
    for p in s.plugins.list().into_iter().filter(|p| p.enabled) {
        let id = p.manifest.id.clone();
        for pg in p
            .manifest
            .ui
            .pages
            .into_iter()
            .filter(|pg| can(Some(&pg.area), &pg.permission))
        {
            pages.push(json!({ "plugin": id, "id": pg.id, "area": pg.area, "title": pg.title, "icon": pg.icon, "nav": pg.nav, "data": pg.data, "body": pg.body }));
        }
        for sl in p
            .manifest
            .ui
            .slots
            .into_iter()
            .filter(|sl| can(sl.slot.split('.').next(), &sl.permission))
        {
            slots.push(json!({ "plugin": id, "slot": sl.slot, "data": sl.data, "body": sl.body }));
        }
    }
    // WASM addons contribute pages/slots in the same schema (addon_ext).
    let (ap, asl) = crate::addon_ext::ui_contributions(&s, can);
    let overrides = crate::addon_ext::ui_overrides(&s, can);
    pages.extend(ap);
    slots.extend(asl);
    let mut ui = json!({ "pages": pages, "slots": slots });
    if crate::addon_filters::has(&s, "ui.contributions") {
        let staff = u.has("admin.access");
        let out = crate::addon_filters::apply_with(
            &s,
            "ui.contributions",
            ui.clone(),
            json!({ "staff": staff }),
            false,
        )
        .await;
        ui = crate::addon_filters::restrict_ui(&ui, &out);
    }
    // An override only counts when its page survived the filter.
    let mut overrides = overrides;
    if let Some(list) = overrides["pages"].as_array_mut() {
        let shown = ui["pages"].as_array().cloned().unwrap_or_default();
        list.retain(|o| {
            shown
                .iter()
                .any(|p| p["plugin"] == o["plugin"] && p["id"] == o["page"])
        });
    }
    Json(
        json!({ "items": items, "pages": ui["pages"], "slots": ui["slots"], "overrides": overrides }),
    )
}

#[utoipa::path(get, path = "/api/v1/admin/plugins/{id}/settings", responses((status = 200), (status = 403), (status = 404)))]
pub async fn get_settings(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let m = s.plugins.manifest(&id).ok_or(AppError::NotFound)?;
    let values = pelty_database::extensions::plugin_settings(&s.pool, &id).await?;
    Ok(Json(
        json!({ "schema": m.settings_schema, "values": values }),
    ))
}

#[utoipa::path(put, path = "/api/v1/admin/plugins/{id}/settings", request_body = Object, responses((status = 204), (status = 403), (status = 404), (status = 422)))]
pub async fn put_settings(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> ApiResult<axum::http::StatusCode> {
    let m = s.plugins.manifest(&id).ok_or(AppError::NotFound)?;
    let schema = m
        .settings_schema
        .ok_or(AppError::Validation("error.plugin_settings_invalid"))?;
    pelty_extensions::plugins::validate_settings(&schema, &b)
        .map_err(|_| AppError::Validation("error.plugin_settings_invalid"))?;
    pelty_database::extensions::set_plugin_settings(&s.pool, &id, &b).await?;
    let keys: Vec<&String> = b
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "plugin.settings_updated",
        Some(&id),
        u.ip.as_deref(),
        json!({ "keys": keys }),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn event_logger_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/summary",
            get(|State(s): State<AppState>, _p: RequirePermission<perms::PluginsManage>| async move {
                ensure_enabled(&s, "event-logger")?;
                ApiResult::Ok(Json(json!({ "title": "plugin.event-logger.widget", "value": s.event_logger.recent().len().to_string() })))
            }),
        )
        .route(
            "/recent",
            get(|State(s): State<AppState>, _p: RequirePermission<perms::PluginsManage>| async move {
                ensure_enabled(&s, "event-logger")?;
                let items: Vec<Value> =
                    s.event_logger.recent().into_iter().rev().map(|e| json!({ "name": e.name, "payload": e.payload })).collect();
                ApiResult::Ok(Json(json!({ "items": items })))
            }),
        )
}

fn external_webhook(
    s: &AppState,
    id: &str,
) -> Result<pelty_extensions::plugins::PluginManifest, AppError> {
    s.plugins
        .manifest(id)
        .filter(|m| m.webhook_url.is_some())
        .ok_or(AppError::NotFound)
}

/// Accepted override URLs: https anywhere, plain http only to loopback (dev).
pub fn valid_webhook_url(u: &str) -> bool {
    let Ok(url) = reqwest_free_parse(u) else {
        return false;
    };
    match url.0.as_str() {
        "https" => !url.1.is_empty(),
        "http" => matches!(url.1.as_str(), "localhost" | "127.0.0.1" | "[::1]"),
        _ => false,
    }
}

/// (scheme, host) without pulling an HTTP client into the API crate.
fn reqwest_free_parse(u: &str) -> Result<(String, String), ()> {
    let (scheme, rest) = u.split_once("://").ok_or(())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@')
        || authority.is_empty()
        || u.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(());
    }
    let host = if authority.starts_with('[') {
        authority
            .split_inclusive(']')
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        authority.split(':').next().unwrap_or("").to_string()
    };
    Ok((scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
}

#[utoipa::path(get, path = "/api/v1/admin/plugins/{id}/webhook", responses((status = 200), (status = 403), (status = 404)))]
pub async fn get_webhook(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let m = external_webhook(&s, &id)?;
    let (o, sealed) = pelty_database::plugin_webhooks::endpoint(&s.pool, &id).await?;
    Ok(Json(
        json!({ "manifest_url": m.webhook_url, "url_override": o, "secret_set": sealed.is_some(), "hooks": m.hooks, "enabled": s.plugins.enabled(&id) }),
    ))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct WebhookBody {
    /// Empty or null clears the override.
    pub url: Option<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/plugins/{id}/webhook", request_body = WebhookBody, responses((status = 204), (status = 403), (status = 404), (status = 422)))]
pub async fn put_webhook(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Json(b): Json<WebhookBody>,
) -> ApiResult<axum::http::StatusCode> {
    external_webhook(&s, &id)?;
    let url = b.url.as_deref().map(str::trim).filter(|x| !x.is_empty());
    if let Some(x) = url {
        if x.len() > 2000 || !valid_webhook_url(x) {
            return Err(AppError::Validation("error.plugin_webhook_url").into());
        }
    }
    pelty_database::plugin_webhooks::set_url(&s.pool, &id, url).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "plugin.webhook_updated",
        Some(&id),
        u.ip.as_deref(),
        json!({ "override": url.is_some() }),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Generate a new signing secret and return it once.
#[utoipa::path(post, path = "/api/v1/admin/plugins/{id}/webhook/secret", responses((status = 200), (status = 403), (status = 404), (status = 409)))]
pub async fn rotate_secret(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    external_webhook(&s, &id)?;
    let sb = s
        .secret_box
        .as_ref()
        .ok_or(AppError::Conflict("error.encryption_not_configured"))?;
    let secret = crate::plugin_hooks::new_secret();
    pelty_database::plugin_webhooks::set_secret(&s.pool, &id, &sb.seal(secret.as_bytes())).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "plugin.webhook_secret_rotated",
        Some(&id),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "secret": secret })))
}

#[utoipa::path(get, path = "/api/v1/admin/plugins/{id}/deliveries", params(crate::routes::ListQuery), responses((status = 200), (status = 403), (status = 404)))]
pub async fn list_deliveries(
    State(s): State<AppState>,
    _p: RequirePermission<perms::PluginsManage>,
    Path(id): Path<String>,
    Query(q): Query<crate::routes::ListQuery>,
) -> ApiResult<Json<Value>> {
    if !s.plugins.exists(&id) {
        return Err(AppError::NotFound.into());
    }
    let status = match q.status() {
        Some(x) if !["pending", "delivered", "failed"].contains(&x) => {
            return Err(AppError::Validation("error.invalid_status").into())
        }
        x => x,
    };
    let (limit, offset, page, per) = q.bounds();
    let (items, total) =
        pelty_database::plugin_webhooks::list(&s.pool, &id, status, limit, offset).await?;
    Ok(Json(crate::routes::paged(items, page, per, total)))
}

#[utoipa::path(post, path = "/api/v1/admin/plugins/{id}/deliveries/{did}/redeliver", responses((status = 204), (status = 403), (status = 404)))]
pub async fn redeliver(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::PluginsManage>,
    Path((id, did)): Path<(String, uuid::Uuid)>,
) -> ApiResult<axum::http::StatusCode> {
    let d = pelty_database::plugin_webhooks::get(&s.pool, did)
        .await?
        .filter(|d| d.plugin_id == id)
        .ok_or(AppError::NotFound)?;
    crate::plugin_hooks::redeliver(&s, d.id).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "plugin.webhook_redelivered",
        Some(&id),
        u.ip.as_deref(),
        json!({ "delivery_id": did }),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::valid_webhook_url as v;

    #[test]
    fn webhook_urls() {
        assert!(v("https://hooks.example.com/pelty"));
        assert!(v("http://127.0.0.1:9000/x"));
        assert!(v("http://localhost/x"));
        assert!(!v("http://example.com/x"));
        assert!(!v("https://user:pw@example.com/"));
        assert!(!v("ftp://example.com"));
        assert!(!v("https:///nohost"));
    }
}
