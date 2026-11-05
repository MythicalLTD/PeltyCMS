//! GitHub sign-in addon (OAuth 2.0 code exchange + userinfo).
//! The provider lives in [`github`]; the shared code exchange in `sso-common`.
pub mod github;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sso_common::sso_addon!(github::PROVIDER);
