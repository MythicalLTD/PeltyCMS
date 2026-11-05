//! Error → HTTP. Handlers return `ApiError`; the `localize_errors` middleware
//! turns the stable code into a translated JSON body with the request id.
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pelty_core::AppError;
use serde_json::json;

use crate::AppState;

pub struct ApiError(pub AppError);
pub type ApiResult<T> = Result<T, ApiError>;

impl<E: Into<AppError>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

#[derive(Clone, Copy)]
pub struct ErrorCode(pub &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let AppError::Internal(detail) = &self.0 {
            tracing::error!(%detail, "internal error");
        }
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let code = self.0.code();
        let mut r = (
            status,
            Json(json!({ "error": { "code": code, "message": code } })),
        )
            .into_response();
        r.extensions_mut().insert(ErrorCode(code));
        r
    }
}

/// Locale: `pelty_lang` cookie, then Accept-Language, then default.
pub fn request_locale(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(l) = crate::extract::cookie(headers, "pelty_lang") {
        if state.i18n.has(&l) {
            return l;
        }
    }
    state
        .i18n
        .negotiate(headers.get("accept-language").and_then(|v| v.to_str().ok()))
}

pub async fn localize_errors(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let lang = request_locale(&state, req.headers());
    let rid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut res = next.run(req).await;
    let code = res
        .extensions()
        .get::<ErrorCode>()
        .map(|c| c.0)
        .or_else(|| match res.status() {
            // Framework rejections (bad JSON, 404 route, 405...) get generic codes.
            StatusCode::NOT_FOUND => Some("error.not_found"),
            StatusCode::METHOD_NOT_ALLOWED => Some("error.method_not_allowed"),
            StatusCode::UNPROCESSABLE_ENTITY
            | StatusCode::BAD_REQUEST
            | StatusCode::UNSUPPORTED_MEDIA_TYPE
                if !is_json(&res) =>
            {
                Some("error.bad_request")
            }
            _ => None,
        });
    if let Some(code) = code {
        let status = res.status();
        let body = json!({ "error": { "code": code, "message": state.i18n.t(&lang, code), "request_id": rid } });
        let mut new = (status, Json(body)).into_response();
        for (k, v) in res.headers() {
            if k != "content-type" && k != "content-length" {
                new.headers_mut().insert(k.clone(), v.clone());
            }
        }
        res = new;
    }
    res
}

fn is_json(r: &Response) -> bool {
    r.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}
