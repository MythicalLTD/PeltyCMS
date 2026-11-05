//! Health check routes (`/health`, readiness) mounted at the root without the API prefix.
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
}

/// Liveness: process is up.
#[utoipa::path(get, path = "/healthz", responses((status = 200)))]
pub async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: database reachable.
#[utoipa::path(get, path = "/readyz", responses((status = 200), (status = 503)))]
pub async fn readyz(State(s): State<AppState>) -> (StatusCode, Json<Value>) {
    if pelty_database::ping(&s.pool).await {
        (
            StatusCode::OK,
            Json(json!({ "status": "ready", "database": "ok" })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unavailable", "database": "down" })),
        )
    }
}
