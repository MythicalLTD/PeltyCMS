//! Identity repositories: users, staff, sessions, MFA, passkeys, API keys, OAuth, SAML/SSO and security settings.
//!
//! Re-exported at the crate root. Covered by the auth_* integration tests.

pub mod users;
pub mod staff;
pub mod sessions;
pub mod mfa;
pub mod passkeys;
pub mod api_keys;
pub mod user_tokens;
pub mod oauth_provider;
pub mod saml;
pub mod sso;
pub mod security;
pub mod security_questions;
