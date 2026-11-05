//! Google sign-in addon (OAuth 2.0 code exchange + userinfo).
//! The provider lives in [`google`]; the shared code exchange in `sso-common`.
pub mod google;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sso_common::sso_addon!(google::PROVIDER);
