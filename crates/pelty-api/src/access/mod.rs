//! Access and security: OAuth login, the OIDC provider, SAML, Duo, captcha, security policy, security headers, trusted proxies and fraud checks.
//!
//! Modules are re-exported at the crate root (`pelty_api::<module>`), so existing paths keep working.
//! Covered by the pelty-api unit tests in these files and the integration suite in tests/integration.

pub mod captcha;
pub mod duo;
pub mod fraud;
pub mod geoip;
pub mod oauth;
pub mod oidc_provider;
pub mod saml;
pub mod security;
pub mod security_headers;
pub mod trusted_proxy;
