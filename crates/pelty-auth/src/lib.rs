//! Authentication: Argon2id passwords, session tokens, rate limiting.
pub mod crypto;
pub mod password;
pub mod policy;
pub mod rate_limit;
pub mod service;
pub mod token;
pub mod totp;

pub use service::{AuthService, LoginInput, RegisterInput};
