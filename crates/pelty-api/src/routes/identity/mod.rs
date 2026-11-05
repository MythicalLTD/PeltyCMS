//! Identity HTTP routes: login/register, MFA, passkeys, SSO, API keys, staff and security questions.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod admin_security;
pub mod api_keys;
pub mod auth;
pub mod mfa;
pub mod passkeys;
pub mod security_questions;
pub mod sso;
pub mod staff;
