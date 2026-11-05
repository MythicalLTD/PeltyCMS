//! Cloudflare Turnstile captcha addon (server-side siteverify).
//! The engine lives in [`turnstile`]; the shared siteverify code in `captcha-common`.
pub mod turnstile;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_captcha_common::captcha_addon!(turnstile::ENGINE);
