//! Public country / subdivision lists (bundled ISO 3166). No live upstream call.
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subdivision {
    pub code: String,
    pub name: String,
    pub country: String,
}

static COUNTRIES: LazyLock<Vec<Country>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/geo/countries.json")).expect("countries.json")
});

static SUBDIVISIONS: LazyLock<Vec<Subdivision>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/geo/subdivisions.json")).expect("subdivisions.json")
});

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/geo/countries", get(countries))
        .route("/public/geo/states", get(states))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct StatesQuery {
    /// ISO 3166-1 alpha-2 country code (e.g. RO, US).
    pub country: String,
}

/// All countries (ISO 3166-1), sorted by English name.
#[utoipa::path(get, path = "/api/v1/public/geo/countries", responses((status = 200)))]
pub async fn countries(State(_s): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": &*COUNTRIES })))
}

/// First-level subdivisions (states / provinces / counties) for one country.
#[utoipa::path(get, path = "/api/v1/public/geo/states", params(StatesQuery), responses((status = 200), (status = 422)))]
pub async fn states(State(_s): State<AppState>, Query(q): Query<StatesQuery>) -> ApiResult<Json<Value>> {
    let code = q.country.trim().to_uppercase();
    if code.len() != 2 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::Validation("error.invalid_country").into());
    }
    let items: Vec<&Subdivision> = SUBDIVISIONS.iter().filter(|s| s.country == code).collect();
    Ok(Json(json!({ "country": code, "items": items })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_full_iso_lists() {
        assert!(COUNTRIES.len() >= 200, "expected full country list, got {}", COUNTRIES.len());
        assert!(SUBDIVISIONS.len() >= 4000, "expected full subdivision list, got {}", SUBDIVISIONS.len());
        assert!(COUNTRIES.iter().any(|c| c.code == "RO" && c.name.contains("Romania")));
        let ro: Vec<_> = SUBDIVISIONS.iter().filter(|s| s.country == "RO").collect();
        assert!(ro.len() > 30, "Romania should have counties");
        let us: Vec<_> = SUBDIVISIONS.iter().filter(|s| s.country == "US").collect();
        assert!(us.len() >= 50, "US should have states");
    }
}
