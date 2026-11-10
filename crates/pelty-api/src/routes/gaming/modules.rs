//! Feature module toggles (`settings.manage`) and the request gate that
//! hides a disabled module's API. The SPA reads `/public/modules` to hide
//! navigation and pages of disabled modules.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::feature_modules::{self as fm, ModuleState, ModuleUpdate};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/modules", get(public_modules))
        .route("/admin/modules", get(admin_list))
        .route("/admin/modules/{id}", put(admin_set))
}

/// API path prefixes (below `/api/v1`) owned by a module. A request whose
/// path starts with one of these answers 404 while the module is off.
pub const GATES: &[(&str, &str)] = &[
    ("/public/game", "game_profiles"),
    ("/client/game", "game_profiles"),
    ("/admin/game", "game_profiles"),
];

/// Module owning `path` (with or without the `/api/v1` prefix), if any.
pub fn module_for(path: &str) -> Option<&'static str> {
    let p = path.strip_prefix("/api/v1").unwrap_or(path);
    GATES
        .iter()
        .filter(|(prefix, _)| {
            p == *prefix
                || p.strip_prefix(prefix)
                    .is_some_and(|r| r.starts_with('/') || prefix.ends_with('/'))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, m)| *m)
}

/// 404 unless the module is enabled. Use inside handlers that live outside
/// a gated prefix (e.g. hooks in shared checkout code).
pub async fn require(s: &AppState, id: &str) -> ApiResult<()> {
    if fm::is_enabled(&s.pool, id).await? {
        Ok(())
    } else {
        Err(AppError::NotFound.into())
    }
}

/// Middleware: 404 for routes of a disabled module.
pub async fn gate(State(s): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(m) = module_for(req.uri().path()) {
        match fm::is_enabled(&s.pool, m).await {
            Ok(true) => {}
            Ok(false) => return crate::error::ApiError::from(AppError::NotFound).into_response(),
            Err(e) => return crate::error::ApiError::from(e).into_response(),
        }
    }
    next.run(req).await
}

/// Enabled module ids, for the SPA (no login).
#[utoipa::path(get, path = "/api/v1/public/modules", responses((status = 200)))]
pub async fn public_modules(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "enabled": fm::enabled(&s.pool).await? })))
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ModuleList {
    pub items: Vec<ModuleState>,
}

/// Every optional module with its state, dependencies and settings.
#[utoipa::path(get, path = "/api/v1/admin/modules", responses((status = 200, body = ModuleList), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<ModuleList>> {
    Ok(Json(ModuleList {
        items: fm::list(&s.pool).await?,
    }))
}

/// Enable or disable a module (and optionally replace its settings).
#[utoipa::path(put, path = "/api/v1/admin/modules/{id}", request_body = ModuleUpdate, responses((status = 200, body = ModuleState), (status = 403), (status = 404), (status = 409), (status = 422)))]
pub async fn admin_set(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(id): Path<String>,
    Json(b): Json<ModuleUpdate>,
) -> ApiResult<Json<ModuleState>> {
    let m = fm::set(&s.pool, &id, &b, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "module.updated",
        Some(&id),
        u.ip.as_deref(),
        json!({ "enabled": m.enabled }),
    )
    .await?;
    Ok(Json(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gates_match_prefixes_only() {
        for (p, _) in GATES {
            assert!(p.starts_with('/'), "{p}");
            let m = module_for(p).unwrap();
            assert!(fm::def(m).is_some(), "{p} gated by unknown module {m}");
        }
        assert_eq!(module_for("/api/v1/admin/modules"), None);
        assert_eq!(module_for("/public/modules"), None);
    }
}
