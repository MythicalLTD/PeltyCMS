//! hCaptcha captcha addon (server-side siteverify).
//! The engine lives in [`hcaptcha`]; the shared siteverify code in `captcha-common`.
pub mod hcaptcha;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_captcha_common::captcha_addon!(hcaptcha::ENGINE);
