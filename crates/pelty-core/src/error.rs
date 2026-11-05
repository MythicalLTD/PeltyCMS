//! Typed application errors. Every error has a stable message code that is
//! translated via locale files; internal details are logged, never returned.
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation failed: {0}")]
    Validation(&'static str),
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("forbidden")]
    Forbidden,
    /// 403 with a specific message code (e.g. a policy requirement).
    #[error("forbidden: {0}")]
    Denied(&'static str),
    #[error("csrf check failed")]
    Csrf,
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(&'static str),
    #[error("rate limited")]
    RateLimited,
    #[error("bad request: {0}")]
    BadRequest(&'static str),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Stable message code (also the translation key).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Validation(c) | AppError::Conflict(c) | AppError::BadRequest(c) | AppError::Denied(c) => c,
            AppError::InvalidCredentials => "error.invalid_credentials",
            AppError::Unauthenticated => "error.unauthenticated",
            AppError::Forbidden => "error.forbidden",
            AppError::Csrf => "error.csrf",
            AppError::NotFound => "error.not_found",
            AppError::RateLimited => "error.rate_limited",
            AppError::Internal(_) => "error.internal",
        }
    }

    /// HTTP status code.
    pub fn status(&self) -> u16 {
        match self {
            AppError::Validation(_) => 422,
            AppError::InvalidCredentials | AppError::Unauthenticated => 401,
            AppError::Forbidden | AppError::Denied(_) | AppError::Csrf => 403,
            AppError::NotFound => 404,
            AppError::Conflict(_) => 409,
            AppError::RateLimited => 429,
            AppError::BadRequest(_) => 400,
            AppError::Internal(_) => 500,
        }
    }

    pub fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }
}
