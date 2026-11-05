//! Google reCAPTCHA v2 captcha addon (server-side siteverify).
//! The engine lives in [`recaptcha_v2`]; the shared siteverify code in `captcha-common`.
pub mod recaptcha_v2;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_captcha_common::captcha_addon!(recaptcha_v2::ENGINE);
