//! Core domain primitives shared by all PeltyCMS crates.
pub mod config;
pub mod error;
pub mod i18n;
pub mod money;

pub use error::{AppError, AppResult};
