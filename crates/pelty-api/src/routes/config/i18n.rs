//! Translation HTTP routes: serve UI string bundles per locale to the frontend.
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::AppError;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/i18n", get(languages))
        .route("/i18n/{lang}", get(messages))
}

/// Available languages plus the one negotiated for this request.
#[utoipa::path(get, path = "/api/v1/i18n", responses((status = 200)))]
pub async fn languages(State(s): State<AppState>, headers: HeaderMap) -> Json<Value> {
    let langs: Vec<Value> = s
        .i18n
        .languages()
        .into_iter()
        .map(|l| json!({ "code": l, "name": s.i18n.lang_name(&l) }))
        .collect();
    Json(json!({ "languages": langs, "current": crate::error::request_locale(&s, &headers) }))
}

/// All messages for a language (falls back to `en` per key on the client).
#[utoipa::path(get, path = "/api/v1/i18n/{lang}", responses((status = 200), (status = 404)))]
pub async fn messages(
    State(s): State<AppState>,
    Path(lang): Path<String>,
) -> ApiResult<Json<Value>> {
    let m = s.i18n.messages(&lang).ok_or(AppError::NotFound)?;
    let mut v = json!(m);
    if let Some(o) = v.as_object_mut() {
        o.extend(crate::addon_ext::messages(&s, &lang));
        // Read the DB so every instance serves fresh edits.
        for (k, t) in pelty_database::translations::for_lang(&s.pool, &lang).await? {
            o.insert(k, Value::String(t));
        }
    }
    Ok(Json(v))
}
